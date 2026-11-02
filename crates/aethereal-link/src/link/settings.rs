//! The engine end's settings (ADR-0028, ADR-0038).

use crate::PORT;
use aethereal_data::Plain;

/// The engine end's section, `link`: where its hub is, the name the hub shows, and its limits (ADR-0038). They are the
/// root's config.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.link.settings")]
#[settings]
pub struct LinkSettings {
    /// The hub's host: a name, or an address written out.
    #[flag]
    pub hub: String,
    /// The hub's link port.
    #[flag]
    pub port: u16,
    /// What the hub shows the agent beside this engine; not an identity.
    #[flag]
    pub name: String,
    /// The most tools the root holds.
    #[flag]
    pub most_tools: u32,
    /// The most bytes of a frame, each way: past it, an offer is refused and an answer is written as a refusal.
    #[flag]
    pub most_bytes: u32,
    /// The milliseconds of the first wait before dialing again, and of the longest: each wait doubles the last.
    pub redial_millis: (u64, u64),
}

impl Default for LinkSettings {
    fn default() -> Self {
        Self {
            hub: String::from("127.0.0.1"),
            port: PORT,
            name: String::from("engine"),
            most_tools: 256,
            most_bytes: 1 << 20,
            redial_millis: (250, 10_000),
        }
    }
}
