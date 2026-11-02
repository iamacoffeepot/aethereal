//! The process capability's settings (ADR-0028, ADR-0040).

use crate::ProcessesConfig;
use aethereal_data::Plain;

/// The process capability's section, `processes`: how long a program asked to stop is given, and how much of its
/// stderr is kept (ADR-0040).
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.process.settings")]
#[settings]
pub struct ProcessesSettings {
    /// The milliseconds a program asked to stop is given to end once its group is sent SIGTERM, before the group is
    /// sent SIGKILL.
    #[flag]
    pub stop_within_millis: u32,
    /// The most bytes of a program's stderr its host keeps: the last it wrote.
    #[flag]
    pub most_stderr_bytes: u32,
}

impl Default for ProcessesSettings {
    fn default() -> Self {
        Self { stop_within_millis: 2000, most_stderr_bytes: 4096 }
    }
}

impl ProcessesSettings {
    /// What the root is made from: the settings that are its, and not its host's.
    #[must_use]
    pub const fn config(&self) -> ProcessesConfig {
        ProcessesConfig { stop_within_millis: self.stop_within_millis }
    }
}
