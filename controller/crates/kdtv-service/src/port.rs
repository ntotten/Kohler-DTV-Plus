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
//! supervisor has something to send. That requires `Link::read` to be
//! cancel-safe — a cancelled read must not have consumed bytes — which
//! [`kdtv_hal::Link::read`] states as part of its own contract and justifies
//! there for `tokio-serial`. The pipes below hold to the same rule. It is
//! restated here because this module is the caller that depends on it: the
//! cancellation happens in the select below and nowhere else.
//!
//! # A failing read is backed off, and gives up
//!
//! A [`LinkIoError`] that is not [`LinkIoError::is_disconnected`] is one attempt
//! inside a retry budget, so the pump reads again — but not immediately and not
//! forever. [`RETRY_BACKOFF`] separates attempts and
//! [`MAX_CONSECUTIVE_FAILURES`] ends the pump, because a read that has failed
//! for longer than a whole Saturn message window is not a link this service
//! should keep driving water on. Without both, an fd that fails without
//! blocking spins at scheduler speed, fills the report channel every pump
//! shares, and writes one platform record per iteration.
//!
//! # `Pipe` rather than `Link`
//!
//! The pump is generic over the read/write/close subset of [`kdtv_hal::Link`],
//! with a blanket implementation for `Box<dyn Link>`. The pump needs three
//! methods and never needs a [`kdtv_hal::LinkDescriptor`], which is the whole
//! of the rest of the trait; narrowing it here is what lets the tests put a
//! scripted valve behind a pump without going through the link factory. A
//! foreign `Link` *can* now be built — `LinkDescriptor::emulated` is public and
//! routes through the same transmit gate — so this is a convenience, not a gap
//! in `kdtv-hal`.

use std::fmt;
use std::io;
use std::time::Duration;

use kdtv_hal::{BoxedFuture, Link, LinkIoError};
use kdtv_units::LinkKind;
use tokio::sync::mpsc;

/// How many bytes one read may return.
///
/// A Saturn frame is at most 20 bytes and a DTV+ frame at most 64; a 128-byte
/// chunk holds either with room for the garbage in front of it, and matches
/// [`kdtv_proto::dtv::RX_CAPACITY`].
const CHUNK: usize = 128;

/// How long the pump waits before reading again after a retryable failure.
///
/// Short against the 320 ms Saturn message window, so a link that recovers
/// loses no frame it could have caught, and long enough that an fd failing
/// without blocking cannot spin.
const RETRY_BACKOFF: Duration = Duration::from_millis(20);

/// How many consecutive retryable failures end the pump.
///
/// Sixteen at [`RETRY_BACKOFF`] is 320 ms — one whole
/// [`kdtv_proto::saturn::Timings`] message window with nothing read. Past that
/// the link is reported as gone, so the supervisor escalates and the zone fails
/// off, rather than the pump retrying a device that is not answering.
const MAX_CONSECUTIVE_FAILURES: u32 = 16;

/// How long a pump stays alive after reporting a terminal failure, waiting for
/// the escalation's all-off.
///
/// The supervisor escalates in the pass that receives the report, so this is one
/// pass of the control loop and change. Short: the pump is holding an fd that
/// has probably gone, and the shutdown path waits on it.
const TERMINAL_GRACE: Duration = Duration::from_millis(100);

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
    /// The bytes reached the wire, at this moment.
    ///
    /// This is when the frame is **recorded** (`LOG-05`). It is deliberately
    /// *not* when the response deadline starts: the supervisor arms
    /// `awaiting_until` when the order is queued, because
    /// `kdtv_engine::ZoneMachine` stamps `sent_at` at the same instant and the
    /// two must agree about what a late response is. Starting the window early
    /// errs towards declaring a timeout, which is the safe direction; the
    /// difference is the pump's scheduling delay plus the write, and it is
    /// unmeasured.
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

/// What happened to a frame handed to [`PortHandle::transmit`].
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Transmitted {
    /// Queued for the pump, which writes it in order.
    Queued,
    /// The port is closed. Expected: a latched zone is still stepped, and the
    /// steps it produces have nowhere to go.
    PortClosed,
    /// The port believed itself open and the pump was not there. A fault in this
    /// service, not on the bus.
    PumpGone,
}

/// The supervisor's end of one pump.
#[derive(Debug)]
pub(crate) struct PortHandle {
    orders: mpsc::UnboundedSender<LinkOrder>,
    open: bool,
    /// Held rather than dropped so that a pump which ended without saying so —
    /// a panic inside a `Link` implementation — is visible. A detached task
    /// that unwinds sends no [`LinkReport::Closed`], and the shared report
    /// channel keeps its other senders, so nothing else would ever notice.
    task: tokio::task::JoinHandle<()>,
}

impl PortHandle {
    /// True until [`PortHandle::close`] has been issued, the pump has reported
    /// itself gone, or the pump's task has ended on its own.
    pub(crate) fn is_open(&self) -> bool {
        self.open && !self.task.is_finished()
    }

    /// Queue a frame, and say what became of it.
    ///
    /// Never blocks: the queue is unbounded because the supervisor issues at
    /// most one transmission per link per tick, and a bounded queue would give
    /// the pump a way to stall the control loop.
    ///
    /// The answer separates the two ways bytes go nowhere. A closed port is
    /// ordinary — a latched zone still gets stepped. A pump that has gone
    /// without closing is a fault in this service, and it used to be invisible:
    /// the send error was discarded, the port went on reporting itself open, and
    /// the zone showed a response timeout with nothing saying why.
    pub(crate) fn transmit(&mut self, bytes: Vec<u8>) -> Transmitted {
        if !self.open {
            return Transmitted::PortClosed;
        }
        if self.task.is_finished() || self.orders.send(LinkOrder::Transmit(bytes)).is_err() {
            self.open = false;
            return Transmitted::PumpGone;
        }
        Transmitted::Queued
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

    /// The pump task has not ended yet.
    ///
    /// Distinct from [`PortHandle::is_open`], which is about whether this
    /// service will send anything more. Orders already queued are written by the
    /// pump *after* a close is issued — that ordering is the whole reason an
    /// all-off reaches the valve ahead of its port closing — so a shutdown that
    /// stopped at "closed" would return with the last stop still in the queue,
    /// and the runtime would drop it unwritten.
    pub(crate) fn is_running(&self) -> bool {
        !self.task.is_finished()
    }
}

/// Start a pump for one link.
pub(crate) fn spawn(
    link: LinkKind,
    pipe: Box<dyn Pipe>,
    reports: mpsc::Sender<LinkReport>,
) -> PortHandle {
    let (orders_tx, orders_rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(pump(link, pipe, orders_rx, reports));
    PortHandle {
        orders: orders_tx,
        open: true,
        task,
    }
}

/// What woke the pump.
enum Woke {
    Order(Option<LinkOrder>),
    Read(Result<Vec<u8>, LinkIoError>),
    /// The backoff after a retryable failure has elapsed; read again.
    Retry,
}

/// The link has failed too many times in a row to keep reading it.
///
/// Reported as a disconnection rather than as one more retryable error, because
/// that is the class the supervisor escalates on: all-off, close the port,
/// latch the zone. A pump that simply exited would leave the zone waiting on
/// its own response timeout with nothing saying the link had gone.
fn gave_up(link: LinkKind) -> LinkIoError {
    LinkIoError::classify(
        link,
        io::Error::new(
            io::ErrorKind::NotConnected,
            format!(
                "{MAX_CONSECUTIVE_FAILURES} consecutive link errors, {} ms apart; \
                 treating the link as gone",
                RETRY_BACKOFF.as_millis()
            ),
        ),
    )
}

async fn pump(
    link: LinkKind,
    mut pipe: Box<dyn Pipe>,
    mut orders: mpsc::UnboundedReceiver<LinkOrder>,
    reports: mpsc::Sender<LinkReport>,
) {
    // Consecutive failures with no successful read between them. A link that
    // fails and recovers is inside its retry budget; a link that only fails is
    // not a link any more.
    let mut failures: u32 = 0;
    let mut backing_off = false;
    loop {
        let woke = if backing_off {
            // The read is not attempted again yet, but an order still wins:
            // backing off must not delay an all-off.
            tokio::select! {
                biased;
                order = orders.recv() => Woke::Order(order),
                () = tokio::time::sleep(RETRY_BACKOFF) => Woke::Retry,
            }
        } else {
            tokio::select! {
                biased;
                order = orders.recv() => Woke::Order(order),
                read = read_chunk(&mut pipe) => Woke::Read(read),
            }
        };

        match woke {
            Woke::Retry => backing_off = false,
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
                    failures = failures.saturating_add(1);
                    backing_off = true;
                    let spent = failures >= MAX_CONSECUTIVE_FAILURES;
                    if reports
                        .send(LinkReport::Failed { link, error })
                        .await
                        .is_err()
                    {
                        break;
                    }
                    if terminal || spent {
                        last_words(&mut pipe, &mut orders, link, &reports).await;
                        break;
                    }
                }
            },
            Woke::Read(Ok(bytes)) => {
                failures = 0;
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
                failures = failures.saturating_add(1);
                backing_off = true;
                let spent = failures >= MAX_CONSECUTIVE_FAILURES;
                if reports
                    .send(LinkReport::Failed { link, error })
                    .await
                    .is_err()
                {
                    break;
                }
                if spent && !terminal {
                    // One last report, as a disconnection, so the supervisor
                    // escalates rather than waiting on a response that is not
                    // coming.
                    let _ = reports
                        .send(LinkReport::Failed {
                            link,
                            error: gave_up(link),
                        })
                        .await;
                }
                if terminal || spent {
                    last_words(&mut pipe, &mut orders, link, &reports).await;
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

/// Stay long enough to write what the escalation is about to ask for.
///
/// The supervisor's answer to a terminal link error is all-off, close the port,
/// latch the zone — and that all-off is queued *after* the failure report it is
/// a reaction to. A pump that exited on the report would take the order channel
/// with it and discard the one frame that stops water, and the log would show a
/// response timeout with nothing saying the pump had gone.
///
/// The write is attempted because a read failure does not prove the transmit
/// direction has gone: RS-485 is two pairs, and a receive pair knocked loose
/// leaves a bus that can still carry a stop. If it cannot, the write fails and
/// the valve's own communication-loss shutdown is the backstop, which is what it
/// is for.
///
/// Bounded by [`TERMINAL_GRACE`] so a device that is genuinely gone cannot hold
/// the pump open, and ended by the `Close` the same escalation sends.
async fn last_words(
    pipe: &mut Box<dyn Pipe>,
    orders: &mut mpsc::UnboundedReceiver<LinkOrder>,
    link: LinkKind,
    reports: &mpsc::Sender<LinkReport>,
) {
    let deadline = tokio::time::sleep(TERMINAL_GRACE);
    tokio::pin!(deadline);
    loop {
        let order = tokio::select! {
            biased;
            order = orders.recv() => order,
            () = &mut deadline => return,
        };
        let Some(LinkOrder::Transmit(bytes)) = order else {
            return;
        };
        let written = tokio::select! {
            written = pipe.write_all(&bytes) => written,
            () = &mut deadline => return,
        };
        if written.is_ok() {
            let _ = reports.send(LinkReport::Sent { link, bytes }).await;
        }
    }
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
        let mut handle = spawn(Z1, Box::new(FakePipe::new(Z1, script)), reports_tx);

        assert_eq!(handle.transmit(vec![0xAA, 0x55, 0x03]), Transmitted::Queued);
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

        // A transmit after a close reaches nothing, and says so.
        assert_eq!(handle.transmit(vec![0x01]), Transmitted::PortClosed);
        assert!(watch.written().is_empty());
    }

    #[tokio::test]
    async fn a_transmit_to_a_pump_that_has_gone_reports_failure_rather_than_dropping_the_frame() {
        let (script, watch) = PipeScript::new();
        let (reports_tx, mut reports) = mpsc::channel(8);
        let mut handle = spawn(Z1, Box::new(FakePipe::new(Z1, script)), reports_tx);

        // Ending the pump without going through `close` is what a panicking
        // `Link` implementation looks like from here: the handle still believes
        // the port is open until something is sent on it.
        handle.task.abort();
        let _ = reports.recv().await;
        while !handle.task.is_finished() {
            tokio::task::yield_now().await;
        }

        assert_eq!(handle.transmit(vec![0x01]), Transmitted::PumpGone);
        assert!(
            !handle.is_open(),
            "a pump that has gone is not an open port"
        );
        assert!(watch.written().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn a_read_that_keeps_failing_is_backed_off_and_then_given_up_on() {
        let (script, _watch) = PipeScript::new();
        for _ in 0..(MAX_CONSECUTIVE_FAILURES + 4) {
            script.push_read_error(LinkIoError::Retryable {
                link: Z1,
                source: io::Error::from(io::ErrorKind::WouldBlock),
            });
        }
        let (reports_tx, mut reports) = mpsc::channel(64);
        let _handle = spawn(Z1, Box::new(FakePipe::new(Z1, script)), reports_tx);

        let mut failures = 0_u32;
        let mut terminal = None;
        let mut closed = false;
        while let Some(report) = reports.recv().await {
            match report {
                LinkReport::Failed { error, .. } => {
                    if error.is_disconnected() {
                        terminal = Some(error);
                    } else {
                        failures += 1;
                    }
                }
                LinkReport::Closed { .. } => {
                    closed = true;
                    break;
                }
                other => panic!("unexpected report {other:?}"),
            }
        }

        assert_eq!(
            failures, MAX_CONSECUTIVE_FAILURES,
            "the pump must stop retrying rather than spin"
        );
        assert!(
            terminal.is_some(),
            "giving up must be reported as a disconnection so the zone fails off"
        );
        assert!(closed, "the pump must close the port on its way out");
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
