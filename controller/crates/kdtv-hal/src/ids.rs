//! Identifiers that are durable before they are used.
//!
//! # The property
//!
//! **A crash can only skip ids forward. It can never reuse one.**
//!
//! Every authorisation to open water is bound to the boot id that minted it, so
//! a restart invalidates outstanding tokens and a start cannot be replayed. That
//! only holds if a boot id is never handed out twice. The same goes for command
//! ids inside a boot: two different commands wearing one id makes a frame log
//! unreadable exactly when it is needed.
//!
//! So the counter is written and `fsync`ed **before** the value is issued, not
//! after. The order matters: persist-then-issue can lose the persist and skip
//! ids, which is harmless; issue-then-persist can lose the persist and reissue
//! one, which is not.
//!
//! # Reservation, and why command ids are not fsynced one at a time
//!
//! An `fsync` per command id would put a disk flush in the command path. Instead
//! the store reserves a block of [`RESERVATION_BLOCK`] ids, `fsync`s the block's
//! **end**, and issues from memory until the block runs out. A crash therefore
//! skips at most one block. Skipping is what the property permits; a gap in the
//! command ids in a log is a visible, explicable thing, and it is what says a
//! restart happened.
//!
//! Boot ids are reserved one at a time — there is one per boot, so the flush is
//! not on any hot path.
//!
//! # What is not here
//!
//! No water state. The state directory holds a counter and, elsewhere, the
//! commissioning offset curve. Nothing that could let a restart resume a
//! session, and no `ExecStartPre` that restores one.

use std::fmt;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use kdtv_units::{BootId, CommandId, PiBootId};

/// How many command ids are reserved per `fsync`.
///
/// A crash skips at most this many. 256 at a 525 ms tick is far more than a
/// session's worth of commands, so in practice one flush covers a boot.
pub const RESERVATION_BLOCK: u64 = 256;

/// The file the counters live in, inside the state directory.
const COUNTER_FILE: &str = "ids";
/// Where the kernel publishes the boot id.
const PROC_BOOT_ID: &str = "/proc/sys/kernel/random/boot_id";

/// Where an id came from and where the next one is coming from.
pub trait IdStore: Send + Sync + fmt::Debug {
    /// Mints this service boot's id. Durable before it returns.
    fn begin_boot(&self) -> Result<BootId, IdError>;

    /// Mints the next command id. Durable before it returns — see the module
    /// docs for what "durable" costs and what it buys.
    fn next_command(&self) -> Result<CommandId, IdError>;

    /// The Linux kernel's boot id, so a log can tell a service restart from a
    /// reboot.
    fn pi_boot_id(&self) -> Result<PiBootId, IdError>;
}

#[derive(Debug, thiserror::Error)]
pub enum IdError {
    #[error("cannot read the id counter at {}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot durably write the id counter at {}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the id counter at {} is not two decimal counters: {found:?}", path.display())]
    Corrupt { path: PathBuf, found: String },
    #[error("the {which} counter has reached its maximum")]
    Exhausted { which: &'static str },
    #[error("cannot read the kernel boot id at {}", path.display())]
    PiBootId {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// The two counters, as they sit on disk.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
struct Counters {
    /// The highest boot id that has been made durable.
    boot: u64,
    /// The highest command id that has been made durable. Ids up to and
    /// including this may already have been issued.
    command: u64,
}

impl Counters {
    fn render(self) -> String {
        format!("boot={}\ncommand={}\n", self.boot, self.command)
    }

    fn parse(text: &str, path: &Path) -> Result<Self, IdError> {
        let mut out = Self::default();
        let mut seen_boot = false;
        let mut seen_command = false;
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let Some((key, value)) = line.split_once('=') else {
                return Err(IdError::Corrupt {
                    path: path.to_path_buf(),
                    found: text.to_owned(),
                });
            };
            let Ok(n) = value.trim().parse::<u64>() else {
                return Err(IdError::Corrupt {
                    path: path.to_path_buf(),
                    found: text.to_owned(),
                });
            };
            match key.trim() {
                "boot" => {
                    out.boot = n;
                    seen_boot = true;
                }
                "command" => {
                    out.command = n;
                    seen_command = true;
                }
                _ => {
                    return Err(IdError::Corrupt {
                        path: path.to_path_buf(),
                        found: text.to_owned(),
                    });
                }
            }
        }
        if seen_boot && seen_command {
            Ok(out)
        } else {
            Err(IdError::Corrupt {
                path: path.to_path_buf(),
                found: text.to_owned(),
            })
        }
    }
}

/// In-memory state: what is durable, and what is left of the current
/// reservation.
#[derive(Copy, Clone, Debug)]
struct Reservation {
    durable: Counters,
    /// The next command id to issue. Always `<= durable.command`.
    next_command: u64,
}

/// An [`IdStore`] backed by a file in the service's state directory.
#[derive(Debug)]
pub struct FileIdStore {
    path: PathBuf,
    proc_boot_id: PathBuf,
    state: Mutex<Reservation>,
}

impl FileIdStore {
    /// Opens or creates the counter file in `state_dir`.
    ///
    /// A missing file is a first start and begins at zero. A **corrupt** file is
    /// not: it refuses rather than starting over, because starting over is
    /// exactly how an id gets reused.
    pub fn open(state_dir: impl AsRef<Path>) -> Result<Self, IdError> {
        Self::open_with(state_dir, PROC_BOOT_ID)
    }

    /// The same, against a different `boot_id` path. For tests.
    pub fn open_with(
        state_dir: impl AsRef<Path>,
        proc_boot_id: impl Into<PathBuf>,
    ) -> Result<Self, IdError> {
        let path = state_dir.as_ref().join(COUNTER_FILE);
        let durable = match std::fs::read_to_string(&path) {
            Ok(text) => Counters::parse(&text, &path)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Counters::default(),
            Err(source) => {
                return Err(IdError::Read {
                    path: path.clone(),
                    source,
                });
            }
        };
        Ok(Self {
            path,
            proc_boot_id: proc_boot_id.into(),
            // Nothing in the previous block may be reissued, so the first
            // command id of this process starts past whatever was made durable.
            state: Mutex::new(Reservation {
                durable,
                next_command: durable.command,
            }),
        })
    }

    /// Writes the counters and `fsync`s the file **and its directory**, so the
    /// rename is durable and not just the bytes.
    fn persist(&self, counters: Counters) -> Result<(), IdError> {
        let dir = self.path.parent().unwrap_or(Path::new("."));
        let tmp = self.path.with_extension("tmp");

        let write = || -> std::io::Result<()> {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(counters.render().as_bytes())?;
            f.sync_all()?;
            drop(f);
            std::fs::rename(&tmp, &self.path)?;
            // A rename is not durable until the directory entry is too. Without
            // this a power cut can leave the old counter in place, which is the
            // reuse this module exists to prevent.
            std::fs::File::open(dir)?.sync_all()?;
            Ok(())
        };

        write().map_err(|source| IdError::Write {
            path: self.path.clone(),
            source,
        })
    }
}

impl IdStore for FileIdStore {
    fn begin_boot(&self) -> Result<BootId, IdError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let next = state
            .durable
            .boot
            .checked_add(1)
            .ok_or(IdError::Exhausted { which: "boot" })?;
        let counters = Counters {
            boot: next,
            ..state.durable
        };
        // Durable first, issued second.
        self.persist(counters)?;
        state.durable = counters;
        Ok(BootId(next))
    }

    fn next_command(&self) -> Result<CommandId, IdError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let candidate = state
            .next_command
            .checked_add(1)
            .ok_or(IdError::Exhausted { which: "command" })?;

        if candidate > state.durable.command {
            let end = state
                .durable
                .command
                .checked_add(RESERVATION_BLOCK)
                .ok_or(IdError::Exhausted { which: "command" })?;
            let counters = Counters {
                command: end,
                ..state.durable
            };
            self.persist(counters)?;
            state.durable = counters;
        }

        state.next_command = candidate;
        Ok(CommandId(candidate))
    }

    fn pi_boot_id(&self) -> Result<PiBootId, IdError> {
        std::fs::read_to_string(&self.proc_boot_id)
            .map(|s| PiBootId(s.trim().to_owned()))
            .map_err(|source| IdError::PiBootId {
                path: self.proc_boot_id.clone(),
                source,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(dir: &Path) -> FileIdStore {
        let boot_id = dir.join("kernel_boot_id");
        if !boot_id.exists() {
            std::fs::write(&boot_id, "3f2c9b1a-0000-4d3e-8f11-aabbccddeeff\n").unwrap();
        }
        FileIdStore::open_with(dir, boot_id).unwrap()
    }

    #[test]
    fn boot_ids_increase_across_restarts_and_are_never_reused() {
        let dir = tempfile::tempdir().unwrap();
        let mut seen = Vec::new();
        for _ in 0..5 {
            // A new store each time is a new process.
            seen.push(store(dir.path()).begin_boot().unwrap());
        }
        assert_eq!(
            seen,
            vec![BootId(1), BootId(2), BootId(3), BootId(4), BootId(5)]
        );
    }

    /// The property, stated as the crash it is about: kill the process at any
    /// point and the next one never issues an id the last one did.
    #[test]
    fn a_crash_mid_block_skips_ids_forward_and_never_back() {
        let dir = tempfile::tempdir().unwrap();
        let issued: Vec<CommandId> = {
            let s = store(dir.path());
            s.begin_boot().unwrap();
            (0..3).map(|_| s.next_command().unwrap()).collect()
        };
        assert_eq!(issued, vec![CommandId(1), CommandId(2), CommandId(3)]);

        // The process dies here, having flushed a block of 256.
        let after: Vec<CommandId> = {
            let s = store(dir.path());
            s.begin_boot().unwrap();
            (0..3).map(|_| s.next_command().unwrap()).collect()
        };
        assert_eq!(
            after,
            vec![
                CommandId(RESERVATION_BLOCK + 1),
                CommandId(RESERVATION_BLOCK + 2),
                CommandId(RESERVATION_BLOCK + 3),
            ]
        );
        for id in &issued {
            assert!(!after.contains(id), "{id:?} was reissued");
        }
    }

    #[test]
    fn one_flush_covers_a_whole_block() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        for _ in 0..RESERVATION_BLOCK {
            s.next_command().unwrap();
        }
        // Still inside the first block: the durable value is the block end.
        let text = std::fs::read_to_string(dir.path().join(COUNTER_FILE)).unwrap();
        assert!(
            text.contains(&format!("command={RESERVATION_BLOCK}")),
            "{text}"
        );
        // One more crosses into the next block.
        assert_eq!(s.next_command().unwrap(), CommandId(RESERVATION_BLOCK + 1));
        let text = std::fs::read_to_string(dir.path().join(COUNTER_FILE)).unwrap();
        assert!(
            text.contains(&format!("command={}", RESERVATION_BLOCK * 2)),
            "{text}"
        );
    }

    /// Starting over from zero is how an id gets reused, so a file that cannot
    /// be read is a refusal, not a fresh start.
    #[test]
    fn a_corrupt_counter_file_refuses_rather_than_restarting_at_zero() {
        let dir = tempfile::tempdir().unwrap();
        for bad in ["boot=1\n", "boot=x\ncommand=1\n", "nonsense\n", "who=1\n"] {
            std::fs::write(dir.path().join(COUNTER_FILE), bad).unwrap();
            let err = FileIdStore::open_with(dir.path(), "/dev/null").unwrap_err();
            assert!(matches!(err, IdError::Corrupt { .. }), "{bad:?} -> {err:?}");
        }
    }

    #[test]
    fn a_first_start_creates_the_file_and_begins_at_one() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!dir.path().join(COUNTER_FILE).exists());
        let s = store(dir.path());
        assert_eq!(s.begin_boot().unwrap(), BootId(1));
        assert_eq!(s.next_command().unwrap(), CommandId(1));
        assert!(dir.path().join(COUNTER_FILE).exists());
    }

    #[test]
    fn the_kernel_boot_id_is_read_as_written() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        assert_eq!(
            s.pi_boot_id().unwrap(),
            PiBootId("3f2c9b1a-0000-4d3e-8f11-aabbccddeeff".to_owned())
        );

        let missing = FileIdStore::open_with(dir.path(), dir.path().join("absent")).unwrap();
        assert!(matches!(
            missing.pi_boot_id(),
            Err(IdError::PiBootId { .. })
        ));
    }

    #[test]
    fn counters_round_trip_through_the_file_format() {
        let path = Path::new("/tmp/does-not-matter");
        let c = Counters {
            boot: 7,
            command: 512,
        };
        assert_eq!(Counters::parse(&c.render(), path).unwrap(), c);
    }

    /// Two callers, one counter. Ids are unique whichever order they interleave.
    #[test]
    fn concurrent_callers_never_share_an_id() {
        use std::sync::Arc;
        let dir = tempfile::tempdir().unwrap();
        let s = Arc::new(store(dir.path()));
        let mut handles = Vec::new();
        for _ in 0..4 {
            let s = Arc::clone(&s);
            handles.push(std::thread::spawn(move || {
                (0..50)
                    .map(|_| s.next_command().unwrap())
                    .collect::<Vec<_>>()
            }));
        }
        let mut all: Vec<CommandId> = handles
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect();
        all.sort();
        let count = all.len();
        all.dedup();
        assert_eq!(all.len(), count);
    }
}
