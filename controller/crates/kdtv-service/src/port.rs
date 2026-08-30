//! The byte pump: one task per link, and nothing in it that knows a protocol.
//!
//! A pump owns its link by value, writes what it is told to write, forwards
//! what it reads, and closes when it is told to. It parses nothing, retries
//! nothing and decides nothing. Every protocol decision belongs to the
//! supervisor, which owns the one [`kdtv_safety::SafetyKernel`] the decisions
//! are made against.
//!
//! # Why the link is not shared
//!
//! [`kdtv_hal::Link`] takes `&mut self` for both directions, so a supervisor
//! that wrote while a pump read would need a lock — and a lock held across a
//! blocking read on a bus that may say nothing for 320 ms is a deadlock. Giving
//! the link to one task and talking to it over channels removes the question.
//!
//! It also makes [`kdtv_safety::Effect::ClosePort`] real rather than advisory.
//! `Link::close` consumes the link, so after [`LinkOrder::Close`] the pump has
//! no value left to write to and exits. There is no state in which the port is
//! closed and the link is still driveable.
//!
//! # Ordering
//!
//! Orders arrive on one channel and are handled in order, so a step that
//! transmits an all-off and then closes the port produces exactly that on the
//! wire: the write completes before the close. That ordering is the whole reason
//! the escalation path is `AllOff`, `ClosePort`, `Latch` and not some other
//! sequence.
//!
//! # The read is cancelled when an order arrives
//!
//! The select is `biased` towards orders, so a pending read is dropped when the
//! supervisor has something to send. **That requires `Link::read` to be
//! cancel-safe** — a cancelled read must not have consumed bytes. It holds for
//! `tokio-serial`, whose `read` is one `AsyncRead::poll_read` behind a readiness
//! check, and for the pipes below. The trait does not say so, which is worth
//! saying here: an implementation that buffered internally and dropped that
//! buffer on cancellation would lose frames, and nothing would report it.
//!
//! # `Pipe` rather than `Link`
//!
//! The pump is generic over the read/write/close subset of
//! [`kdtv_hal::Link`], with a blanket implementation for `Box<dyn Link>`. The
//! reason is testability and it is a real limitation of `kdtv-hal`:
//! `LinkDescriptor::new` is crate-private, so a fake `Link` cannot be built
//! outside `kdtv-hal` at all. The descriptor is read once at open time, in the
//! composition root, where a real link is still in hand.

use std::fmt;

use kdtv_hal::{BoxedFuture, Link, LinkIoError};
use kdtv_units::LinkKind;
use tokio::sync::mpsc;

/// How many bytes one read may return.
///
/// A Saturn frame is at most 20 bytes and a DTV+ frame at most 64; a 128-byte
/// chunk holds either with room for the garbage in front of it, and matches
/// [`kdtv_proto::dtv::RX_CAPACITY`].
const CHUNK: usize = 128;

/// The read, write and close half of [`kdtv_hal::Link`].
pub(crate) trait Pipe: Send + fmt::Debug + 'static {
    fn write_all<'a>(&'a mut self, buf: &'a [u8]) -> BoxedFuture<'a, Result<(), LinkIoError>>;
    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> BoxedFuture<'a, Result<usize, LinkIoError>>;
    fn close(self: Box<Self>) -> BoxedFuture<'static, Result<(), LinkIoError>>;
}

impl Pipe for Box<dyn Link> {
    fn write_all<'a>(&'a mut self, buf: &'a [u8]) -> BoxedFuture<'a, Result<(), LinkIoError>> {
        (**self).write_all(buf)
    }

    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> BoxedFuture<'a, Result<usize, LinkIoError>> {
        (**self).read(buf)
    }

    fn close(self: Box<Self>) -> BoxedFuture<'static, Result<(), LinkIoError>> {
        let inner: Box<dyn Link> = *self;
        inner.close()
    }
}

/// What the supervisor asks a pump to do.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum LinkOrder {
    /// Put these bytes on the wire, then say so.
    Transmit(Vec<u8>),
    /// Close the port and exit. The link is dropped, which is the close.
    Close,
}

/// What a pump tells the supervisor.
#[derive(Debug)]
pub(crate) enum LinkReport {
    /// The bytes reached the wire, at this moment. The response deadline is
    /// measured from here, not from when the order was queued.
    Sent { link: LinkKind, bytes: Vec<u8> },
    /// Bytes came off the wire.
    Received { link: LinkKind, bytes: Vec<u8> },
    /// The link faulted. [`LinkIoError::is_disconnected`] is the difference
    /// between one attempt inside a retry budget and a zone that must latch.
    Failed { link: LinkKind, error: LinkIoError },
    /// The port is closed and the pump has exited.
    Closed { link: LinkKind },
}

impl LinkReport {
    pub(crate) const fn link(&self) -> LinkKind {
        match self {
            Self::Sent { link, .. }
            | Self::Received { link, .. }
            | Self::Failed { link, .. }
            | Self::Closed { link } => *link,
        }
    }
}

/// The supervisor's end of one pump.
#[derive(Debug)]
pub(crate) struct PortHandle {
    orders: mpsc::UnboundedSender<LinkOrder>,
    open: bool,
}

impl PortHandle {
    /// True until [`PortHandle::close`] has been issued or the pump has
    /// reported itself gone.
    pub(crate) const fn is_open(&self) -> bool {
        self.open
    }

    /// Queue a frame. Never blocks: the queue is unbounded because the
    /// supervisor issues at most one transmission per link per tick, and a
    /// bounded queue would give the pump a way to stall the control loop.
    pub(crate) fn transmit(&self, bytes: Vec<u8>) {
        if self.open {
            let _ = self.orders.send(LinkOrder::Transmit(bytes));
        }
    }

    /// Close the port. Idempotent, and ordered behind anything already queued —
    /// which is what lets an all-off go out ahead of the close that follows it.
    pub(crate) fn close(&mut self) {
        if self.open {
            self.open = false;
            let _ = self.orders.send(LinkOrder::Close);
        }
    }

    /// Record that the pump has gone, without asking it to.
    pub(crate) const fn mark_closed(&mut self) {
        self.open = false;
    }
}

/// Start a pump for one link.
pub(crate) fn spawn(
    link: LinkKind,
    pipe: Box<dyn Pipe>,
    reports: mpsc::Sender<LinkReport>,
) -> PortHandle {
    let (orders_tx, orders_rx) = mpsc::unbounded_channel();
    tokio::spawn(pump(link, pipe, orders_rx, reports));
    PortHandle {
        orders: orders_tx,
        open: true,
    }
}

/// What woke the pump.
enum Woke {
    Order(Option<LinkOrder>),
    Read(Result<Vec<u8>, LinkIoError>),
}

async fn pump(
    link: LinkKind,
    mut pipe: Box<dyn Pipe>,
    mut orders: mpsc::UnboundedReceiver<LinkOrder>,
    reports: mpsc::Sender<LinkReport>,
) {
    loop {
        let woke = tokio::select! {
            biased;
            order = orders.recv() => Woke::Order(order),
            read = read_chunk(&mut pipe) => Woke::Read(read),
        };

        match woke {
            // The supervisor dropped its handle. Nothing else will ever ask
            // this link for anything, so the port closes rather than lingering
            // open behind a dead controller.
            Woke::Order(None | Some(LinkOrder::Close)) => break,
            Woke::Order(Some(LinkOrder::Transmit(bytes))) => match pipe.write_all(&bytes).await {
                Ok(()) => {
                    if reports
                        .send(LinkReport::Sent { link, bytes })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(error) => {
                    let terminal = error.is_disconnected();
                    if reports
                        .send(LinkReport::Failed { link, error })
                        .await
                        .is_err()
                        || terminal
                    {
                        break;
                    }
                }
            },
            Woke::Read(Ok(bytes)) => {
                if reports
                    .send(LinkReport::Received { link, bytes })
                    .await
                    .is_err()
                {
                    break;
                }
            }
            Woke::Read(Err(error)) => {
                let terminal = error.is_disconnected();
                if reports
                    .send(LinkReport::Failed { link, error })
                    .await
                    .is_err()
                    || terminal
                {
                    break;
                }
            }
        }
    }

    // Whatever ended the loop, the fd goes. `close` consumes the pipe, so there
    // is nothing left here that could write.
    let _ = pipe.close().await;
    let _ = reports.send(LinkReport::Closed { link }).await;
}

/// One read, with the buffer owned by the future so cancelling it borrows
/// nothing the caller still holds.
async fn read_chunk(pipe: &mut Box<dyn Pipe>) -> Result<Vec<u8>, LinkIoError> {
    let mut buf = [0u8; CHUNK];
    let n = pipe.read(&mut buf).await?;
    Ok(buf.get(..n).unwrap_or_default().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::fakes::{FakePipe, PipeScript};
    use kdtv_units::ZoneId;

    const Z1: LinkKind = LinkKind::Zone(ZoneId::Zone1);

    #[tokio::test]
    async fn a_transmission_is_reported_only_once_it_has_reached_the_wire() {
        let (script, watch) = PipeScript::new();
        let (reports_tx, mut reports) = mpsc::channel(8);
        let handle = spawn(Z1, Box::new(FakePipe::new(Z1, script)), reports_tx);

        handle.transmit(vec![0xAA, 0x55, 0x03]);
        let report = reports.recv().await.expect("a report");
        let LinkReport::Sent { bytes, .. } = report else {
            panic!("expected Sent, got {report:?}");
        };
        assert_eq!(bytes, vec![0xAA, 0x55, 0x03]);
        assert_eq!(watch.written(), vec![0xAA, 0x55, 0x03]);
    }

    #[tokio::test]
    async fn closing_consumes_the_pipe_and_the_pump_exits() {
        let (script, watch) = PipeScript::new();
        let (reports_tx, mut reports) = mpsc::channel(8);
        let mut handle = spawn(Z1, Box::new(FakePipe::new(Z1, script)), reports_tx);

        handle.close();
        handle.close(); // idempotent
        assert!(!handle.is_open());
        let report = reports.recv().await.expect("a report");
        assert!(matches!(report, LinkReport::Closed { .. }), "{report:?}");
        assert!(watch.is_closed());

        // A transmit after a close reaches nothing.
        handle.transmit(vec![0x01]);
        assert!(watch.written().is_empty());
    }

    #[tokio::test]
    async fn a_disconnected_read_ends_the_pump_rather_than_being_retried_forever() {
        let (script, _watch) = PipeScript::new();
        script.push_read_error(LinkIoError::eof(Z1));
        let (reports_tx, mut reports) = mpsc::channel(8);
        let _handle = spawn(Z1, Box::new(FakePipe::new(Z1, script)), reports_tx);

        let first = reports.recv().await.expect("a failure");
        assert!(matches!(first, LinkReport::Failed { .. }), "{first:?}");
        let second = reports.recv().await.expect("a close");
        assert!(matches!(second, LinkReport::Closed { .. }), "{second:?}");
    }

    #[tokio::test]
    async fn bytes_read_are_forwarded_verbatim() {
        let (script, _watch) = PipeScript::new();
        script.push_read(vec![0xAA, 0x55, 0x00, 0x02, 0x01, 0x1E, 0xDF]);
        let (reports_tx, mut reports) = mpsc::channel(8);
        let _handle = spawn(Z1, Box::new(FakePipe::new(Z1, script)), reports_tx);

        let report = reports.recv().await.expect("bytes");
        let LinkReport::Received { bytes, .. } = report else {
            panic!("expected Received, got {report:?}");
        };
        assert_eq!(bytes.len(), 7);
        assert_eq!(bytes.first(), Some(&0xAA));
    }
}
