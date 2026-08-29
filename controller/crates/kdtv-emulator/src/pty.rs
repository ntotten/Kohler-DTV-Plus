//! Virtual serial ports, from `posix_openpt(3)`.
//!
//! The daemon opens a real character device and configures it with the same
//! `tokio_serial::SerialStream::open` call it uses on the Pi. Nothing in the
//! service knows it is talking to a pseudo-terminal.
//!
//! Why a PTY rather than `socat`: lifetimes are deterministic, there is no child
//! process to reap, and a hangup is a real `close(2)` — which is what makes it a
//! faithful model of a USB converter being unplugged.
//!
//! What a PTY cannot give is baud rate, framing errors or bus collisions. Those
//! are modelled on the leader side by the wire simulator, which lands with the
//! device models — the kernel moves bytes instantly and perfectly, and a
//! 9600 baud half-duplex bus does neither.

use nix::fcntl::OFlag;
use nix::pty::{PtyMaster, grantpt, posix_openpt, ptsname_r, unlockpt};
use nix::sys::termios::{self, SetArg};
use std::io;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// One end of a virtual serial link: the leader file descriptor we read and
/// write, and the follower path the daemon opens.
#[derive(Debug)]
pub struct PtyPair {
    leader: PtyMaster,
    follower: PathBuf,
}

impl PtyPair {
    /// Allocate a pair.
    ///
    /// The leader is opened non-blocking so a read can be polled without a
    /// dedicated thread per link, and the line discipline is put into raw mode
    /// immediately.
    ///
    /// Raw mode is not a detail. A pseudo-terminal starts in canonical mode with
    /// echo on, which would (a) hold bytes until a newline arrives — and a
    /// Saturn frame contains no newline — and (b) echo everything the emulator
    /// writes straight back at it. Both would be invisible corruption of a
    /// transport that is supposed to be a dumb pipe. A real RS-485 converter has
    /// no line discipline, so neither should this.
    pub fn open() -> io::Result<Self> {
        let leader = posix_openpt(OFlag::O_RDWR | OFlag::O_NOCTTY | OFlag::O_NONBLOCK)
            .map_err(io::Error::from)?;
        grantpt(&leader).map_err(io::Error::from)?;
        unlockpt(&leader).map_err(io::Error::from)?;
        // The pair shares one termios, so setting it from the leader configures
        // the link however the follower is later opened.
        let mut t = termios::tcgetattr(&leader).map_err(io::Error::from)?;
        termios::cfmakeraw(&mut t);
        termios::tcsetattr(&leader, SetArg::TCSANOW, &t).map_err(io::Error::from)?;
        let follower = ptsname_r(&leader).map_err(io::Error::from)?;
        Ok(Self {
            leader,
            follower: PathBuf::from(follower),
        })
    }

    /// Read until `buf` is full or the deadline passes.
    ///
    /// Tests use this rather than a bare loop so a regression fails in a second
    /// instead of hanging a CI job for its whole timeout.
    #[expect(
        clippy::disallowed_methods,
        reason = "the ban on Instant::now exists to keep state machines deterministic; \
                  this is a real wall-clock deadline on real I/O in the test harness, \
                  which is the case it is not aimed at"
    )]
    pub fn read_exact_before(&self, buf: &mut [u8], deadline: Duration) -> io::Result<usize> {
        let start = Instant::now();
        let mut got = 0;
        while got < buf.len() {
            if start.elapsed() > deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("read {got} of {} bytes before the deadline", buf.len()),
                ));
            }
            match buf.get_mut(got..) {
                Some(rest) => got += self.read_available(rest)?,
                None => break,
            }
            if got < buf.len() {
                std::thread::yield_now();
            }
        }
        Ok(got)
    }

    /// The path to hand the daemon, e.g. `/dev/pts/7`.
    #[must_use]
    pub fn follower_path(&self) -> &std::path::Path {
        &self.follower
    }

    #[must_use]
    pub fn leader_fd(&self) -> std::os::fd::RawFd {
        self.leader.as_raw_fd()
    }

    /// Read whatever the daemon has transmitted, without blocking.
    ///
    /// `Ok(0)` means nothing was waiting. A closed follower surfaces as `EIO` on
    /// Linux rather than end-of-file, and is reported as `Ok(0)` here because
    /// the daemon closing its port is not an error for the emulator.
    pub fn read_available(&self, buf: &mut [u8]) -> io::Result<usize> {
        match nix::unistd::read(&self.leader, buf) {
            Ok(n) => Ok(n),
            Err(nix::errno::Errno::EAGAIN | nix::errno::Errno::EIO) => Ok(0),
            Err(e) => Err(io::Error::from(e)),
        }
    }

    /// Write bytes towards the daemon.
    pub fn write_all(&self, mut buf: &[u8]) -> io::Result<()> {
        while !buf.is_empty() {
            match nix::unistd::write(&self.leader, buf) {
                Ok(0) => return Err(io::Error::other("pty leader accepted no bytes")),
                Ok(n) => buf = buf.get(n..).unwrap_or(&[]),
                Err(nix::errno::Errno::EAGAIN) => std::thread::yield_now(),
                Err(e) => return Err(io::Error::from(e)),
            }
        }
        Ok(())
    }

    /// Drop the leader, which hangs the follower up.
    ///
    /// This is the model for USB enumeration loss: the daemon's next read gets
    /// an error rather than silence, and the service must treat it as a link
    /// fault rather than as a retryable timeout.
    pub fn hangup(self) -> OwnedFd {
        let Self { leader, .. } = self;
        OwnedFd::from(leader)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;
    use std::io::{Read, Write};

    #[test]
    fn a_pair_round_trips_bytes_in_both_directions() {
        let pty = PtyPair::open().expect("allocate a pty");
        let mut follower = OpenOptions::new()
            .read(true)
            .write(true)
            .open(pty.follower_path())
            .expect("open the follower");

        pty.write_all(b"\xAA\x55\x03\x02\x00\xFB")
            .expect("write towards the daemon");
        let mut got = [0u8; 6];
        follower.read_exact(&mut got).expect("read at the follower");
        assert_eq!(got, [0xAA, 0x55, 0x03, 0x02, 0x00, 0xFB]);
        // Nothing came back at the leader: echo is off, so the emulator does not
        // see its own transmission. A converter with automatic direction control
        // presents no echo either, which is why the codec must not expect one.
        let mut echo = [0u8; 8];
        assert_eq!(pty.read_available(&mut echo).expect("no echo"), 0);

        follower
            .write_all(b"\xAA\x55\x00\x02\x01\x1E\xDF")
            .expect("write at the follower");
        follower.flush().ok();
        let mut buf = [0u8; 7];
        pty.read_exact_before(&mut buf, Duration::from_secs(5))
            .expect("reply reaches the leader");
        assert_eq!(&buf, b"\xAA\x55\x00\x02\x01\x1E\xDF");
    }

    #[test]
    fn the_follower_path_is_a_real_device_the_daemon_can_open() {
        let pty = PtyPair::open().expect("allocate a pty");
        assert!(
            pty.follower_path().starts_with("/dev/pts/"),
            "{:?}",
            pty.follower_path()
        );
        assert!(pty.follower_path().exists());
        assert!(
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(pty.follower_path())
                .is_ok()
        );
    }

    #[test]
    fn nothing_waiting_reads_as_zero_rather_than_blocking() {
        let pty = PtyPair::open().expect("allocate a pty");
        let _follower = OpenOptions::new()
            .read(true)
            .write(true)
            .open(pty.follower_path())
            .unwrap();
        let mut buf = [0u8; 8];
        assert_eq!(pty.read_available(&mut buf).expect("non-blocking read"), 0);
    }
}
