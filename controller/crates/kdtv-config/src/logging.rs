//! Where the log goes and how large it may get.
//!
//! Raw RX/TX frame bytes with monotonic and wall-clock stamps are large, and
//! they are the thing you will want after an unexplained event. The budget is
//! bounded so a frame log cannot fill the card and take the service down with
//! it.

use crate::error::ConfigError;
use std::path::{Path, PathBuf};

/// The validated logging configuration.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LoggingConfig {
    directory: PathBuf,
    frames: bool,
    max_total_mb: u64,
}

impl LoggingConfig {
    pub(crate) fn build(
        directory: &str,
        frames: bool,
        max_total_mb: u64,
    ) -> Result<Self, ConfigError> {
        if directory.trim().is_empty() {
            return Err(ConfigError::LoggingDirectoryEmpty);
        }
        if max_total_mb == 0 {
            return Err(ConfigError::LoggingBudgetZero);
        }
        Ok(Self {
            directory: PathBuf::from(directory),
            frames,
            max_total_mb,
        })
    }

    /// Not required to exist at validation time: the daemon creates it, and the
    /// emulated rig points at a relative path inside its own working tree.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Whether raw frame bytes are logged.
    #[must_use]
    pub const fn frames(&self) -> bool {
        self.frames
    }

    #[must_use]
    pub const fn max_total_mb(&self) -> u64 {
        self.max_total_mb
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_contract_values_build() {
        let l = LoggingConfig::build("/var/log/kdtvd", true, 512).unwrap();
        assert_eq!(l.directory(), Path::new("/var/log/kdtvd"));
        assert!(l.frames());
        assert_eq!(l.max_total_mb(), 512);
    }

    #[test]
    fn a_relative_directory_is_accepted_for_the_rig() {
        assert!(LoggingConfig::build(".e2e/logs", true, 512).is_ok());
    }

    #[test]
    fn an_empty_directory_or_a_zero_budget_is_refused() {
        assert!(matches!(
            LoggingConfig::build("   ", true, 512),
            Err(ConfigError::LoggingDirectoryEmpty)
        ));
        assert!(matches!(
            LoggingConfig::build("/var/log/kdtvd", true, 0),
            Err(ConfigError::LoggingBudgetZero)
        ));
    }
}
