//! The process capability's settings (ADR-0028, ADR-0040).

use crate::ProcessesConfig;
use aethereal_data::Plain;

/// The process capability's section, `processes`: how long a program asked to stop is given (ADR-0040).
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.process.settings")]
#[settings]
pub struct ProcessesSettings {
    /// The milliseconds a program asked to stop is given to end once its group is sent SIGTERM, before the group is
    /// sent SIGKILL.
    #[flag]
    pub kill_after_millis: u32,
}

impl Default for ProcessesSettings {
    fn default() -> Self {
        Self { kill_after_millis: 2000 }
    }
}

impl ProcessesSettings {
    /// What the root is made from: the settings that are its, and not its host's.
    #[must_use]
    pub const fn config(&self) -> ProcessesConfig {
        ProcessesConfig { kill_after_millis: self.kill_after_millis }
    }
}
