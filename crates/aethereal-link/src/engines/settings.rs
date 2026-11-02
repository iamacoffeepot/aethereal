//! The hub end's settings (ADR-0028, ADR-0038).

use crate::PORT;
use aethereal_data::Plain;

/// The hub end's section, `engines`: the port engines dial, and its limits (ADR-0038). They are the root's config.
/// Where it listens is `Tcp`'s setting, loopback unless set otherwise.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.link.engines_settings")]
#[settings]
pub struct EnginesSettings {
    /// The port listened on, 0 for any the host picks.
    #[flag]
    pub port: u16,
    /// The most engines linked at once; past it, a connection is shut as it is accepted.
    #[flag]
    pub most_engines: u32,
    /// The most calls each engine has out at a time; past it, a call is refused at once.
    #[flag]
    pub most_calls: u32,
    /// The most bytes of a frame, each way: past it, a call is refused without being written.
    #[flag]
    pub most_bytes: u32,
}

impl Default for EnginesSettings {
    fn default() -> Self {
        Self { port: PORT, most_engines: 64, most_calls: 64, most_bytes: 1 << 20 }
    }
}
