//! The hub end's settings (ADR-0028, ADR-0038, ADR-0040).

use crate::read_limit;
use crate::{PORT, ReadLimitTooLow};
use aethereal_data::Plain;

/// The hub end's section, `engines`: the port engines dial, the host they dial it on, its tokens, its spawns, and its
/// limits
/// (ADR-0038, ADR-0040). They are the root's config. Where it listens is `Tcp`'s setting, loopback unless set
/// otherwise.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.link.engines_settings")]
#[settings]
pub struct EnginesSettings {
    /// The port listened on, 0 for any the host picks.
    #[flag]
    pub port: u16,
    /// The host an engine dials to reach this hub, as the arguments a token is minted with name it: where `Tcp`
    /// listens.
    #[flag]
    pub host: String,
    /// The most engines linked at once; past it, a connection is shut as it is accepted.
    #[flag]
    pub most_engines: u32,
    /// The most calls each engine has out at a time; past it, a call is refused at once.
    #[flag]
    pub most_calls: u32,
    /// The most bytes of a frame, each way: past it, a call is refused without being written.
    #[flag]
    pub most_bytes: u32,
    /// The milliseconds a token minted for an engine started outside the hub is good for.
    #[flag]
    pub token_within_millis: u64,
    /// The most tokens for engines started outside the hub kept at once; past it, the oldest is dropped as another is
    /// minted.
    #[flag]
    pub most_tokens: u32,
    /// The milliseconds an engine the hub spawned is given to link, from when its program starts; past it, its spawn
    /// is answered `Unlinked`, and its program stopped.
    #[flag]
    pub link_within_millis: u64,
    /// The most engines the hub spawned that have ended kept to list, with how each exited; past it, the oldest is
    /// dropped as another ends.
    #[flag]
    pub most_exited: u32,
}

impl Default for EnginesSettings {
    fn default() -> Self {
        Self {
            port: PORT,
            host: String::from("127.0.0.1"),
            most_engines: 64,
            most_calls: 64,
            most_bytes: 1 << 20,
            token_within_millis: 600_000,
            most_tokens: 64,
            link_within_millis: 30_000,
            most_exited: 16,
        }
    }
}

impl EnginesSettings {
    /// Refuses a hub whose frames of `most_bytes` could not be read under `read_limit`: a frame that could not be read
    /// would fail an engine's turn and end it, so a peer could end it with one large frame. Each host's bootstrap
    /// checks it.
    pub fn check_read_limit(&self, read_limit: usize) -> Result<(), ReadLimitTooLow> {
        read_limit::check("engines", read_limit, self.most_bytes)
    }
}
