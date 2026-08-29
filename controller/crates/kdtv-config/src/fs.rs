//! The filesystem questions validation asks, behind a trait.
//!
//! Validation has to know three things it cannot know from the file alone:
//! whether a port path resolves to a device, what two port paths resolve *to*
//! (two `by-id` names can symlink the same `ttyUSB`), and the permission bits on
//! the API token file.
//!
//! Those questions go through [`FsView`] rather than `std::fs` directly, for the
//! same reason time is a parameter elsewhere in this workspace: a validator that
//! reads the real filesystem can only be tested on a machine that has the real
//! devices. [`MapFs`] answers from a table, so every refusal in this crate has a
//! test.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// What validation may ask of the filesystem. Two questions, no more.
pub trait FsView: fmt::Debug {
    /// The path with symlinks resolved, or `None` when nothing is there.
    ///
    /// This is the identity two zones are compared on. Comparing the configured
    /// strings alone would accept `by-id/A` and `by-id/B` both pointing at
    /// `ttyUSB0`, which is the exact confusion the `by-id` rule exists to
    /// prevent.
    fn canonicalize(&self, path: &Path) -> Option<PathBuf>;

    /// The Unix permission bits, or `None` when nothing is there.
    fn mode(&self, path: &Path) -> Option<u32>;
}

/// The real filesystem.
///
/// Unix only. The daemon targets `aarch64-unknown-linux-gnu` and the permission
/// check is the point of the type, so there is no portable fallback that returns
/// a mode it did not read — a validator that cannot see the token file's
/// permissions must not pretend they are safe.
#[cfg(unix)]
#[derive(Copy, Clone, Debug, Default)]
pub struct RealFs;

#[cfg(unix)]
impl FsView for RealFs {
    fn canonicalize(&self, path: &Path) -> Option<PathBuf> {
        std::fs::canonicalize(path).ok()
    }

    fn mode(&self, path: &Path) -> Option<u32> {
        use std::os::unix::fs::PermissionsExt as _;
        // `metadata` follows symlinks, which is what the credential path needs:
        // the bits that matter are the ones on the file that is actually read.
        std::fs::metadata(path)
            .ok()
            .map(|m| m.permissions().mode() & 0o7777)
    }
}

/// One entry in a [`MapFs`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FsEntry {
    /// What the path resolves to. Give two entries the same target to model two
    /// `by-id` names symlinked to one device.
    pub resolves_to: PathBuf,
    /// Unix permission bits.
    pub mode: u32,
}

impl FsEntry {
    /// A path that resolves to itself, mode `0o600`.
    pub fn own(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self {
            resolves_to: path,
            mode: 0o600,
        }
    }

    /// A path that resolves to `target`, mode `0o600`.
    pub fn link(target: impl Into<PathBuf>) -> Self {
        Self {
            resolves_to: target.into(),
            mode: 0o600,
        }
    }

    #[must_use]
    pub fn with_mode(mut self, mode: u32) -> Self {
        self.mode = mode;
        self
    }
}

/// A filesystem described by a table.
///
/// Used by this crate's tests and by any downstream crate that needs to validate
/// a configuration against a filesystem it does not have — the emulator rig, for
/// one. A path absent from the table does not exist.
#[derive(Clone, Debug, Default)]
pub struct MapFs {
    entries: BTreeMap<PathBuf, FsEntry>,
}

impl MapFs {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with(mut self, path: impl Into<PathBuf>, entry: FsEntry) -> Self {
        self.entries.insert(path.into(), entry);
        self
    }

    /// Adds a path that resolves to itself with mode `0o600`.
    #[must_use]
    pub fn with_device(self, path: impl Into<PathBuf> + Clone) -> Self {
        let entry = FsEntry::own(path.clone());
        self.with(path, entry)
    }

    /// Removes a path, so the next lookup finds nothing.
    #[must_use]
    pub fn without(mut self, path: impl AsRef<Path>) -> Self {
        self.entries.remove(path.as_ref());
        self
    }
}

impl FsView for MapFs {
    fn canonicalize(&self, path: &Path) -> Option<PathBuf> {
        self.entries.get(path).map(|e| e.resolves_to.clone())
    }

    fn mode(&self, path: &Path) -> Option<u32> {
        self.entries.get(path).map(|e| e.mode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_map_fs_answers_only_what_it_was_given() {
        let fs = MapFs::new()
            .with_device("/dev/serial/by-id/one")
            .with("/etc/token", FsEntry::own("/etc/token").with_mode(0o400));
        assert_eq!(
            fs.canonicalize(Path::new("/dev/serial/by-id/one")),
            Some(PathBuf::from("/dev/serial/by-id/one"))
        );
        assert_eq!(fs.canonicalize(Path::new("/dev/serial/by-id/two")), None);
        assert_eq!(fs.mode(Path::new("/etc/token")), Some(0o400));
        assert_eq!(fs.mode(Path::new("/etc/missing")), None);
    }

    #[test]
    fn two_names_can_resolve_to_one_device() {
        let fs = MapFs::new()
            .with("/dev/serial/by-id/a", FsEntry::link("/dev/ttyUSB0"))
            .with("/dev/serial/by-id/b", FsEntry::link("/dev/ttyUSB0"));
        assert_eq!(
            fs.canonicalize(Path::new("/dev/serial/by-id/a")),
            fs.canonicalize(Path::new("/dev/serial/by-id/b"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_real_filesystem_reads_back_a_mode_it_was_given() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token");
        std::fs::write(&path, b"secret").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o604)).unwrap();
        assert_eq!(RealFs.mode(&path), Some(0o604));
        assert!(RealFs.canonicalize(&path).is_some());
        assert_eq!(RealFs.canonicalize(&dir.path().join("absent")), None);
    }
}
