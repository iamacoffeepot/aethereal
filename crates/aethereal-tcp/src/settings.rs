//! The TCP capability's settings (ADR-0028, ADR-0033).

use crate::TcpConfig;
use aethereal_data::Plain;

/// The TCP capability's section, `tcp`: where listeners bind, its limits, and how its host dials (ADR-0033).
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.tcp.settings")]
#[settings]
pub struct TcpSettings {
    /// The address listeners bind: loopback unless set otherwise.
    #[flag]
    pub interface: String,
    /// The most listeners the root holds.
    #[flag]
    pub most_listeners: u32,
    /// The most connections dialed out the root holds, and the most each listener holds of what it accepted.
    #[flag]
    pub most_connections: u32,
    /// The most bytes a connection holds unsent, past which a write is answered `Full`.
    #[flag]
    pub most_unsent_bytes: u32,
    /// How many threads dial.
    #[flag]
    pub dialers: u32,
    /// The milliseconds a dial tries each address.
    #[flag]
    pub dial_within_millis: u32,
}

impl Default for TcpSettings {
    fn default() -> Self {
        Self {
            interface: String::from("127.0.0.1"),
            most_listeners: 16,
            most_connections: 256,
            most_unsent_bytes: 1 << 20,
            dialers: 2,
            dial_within_millis: 5000,
        }
    }
}

impl TcpSettings {
    /// What the root is made from: the settings that are its, and not its host's.
    #[must_use]
    pub fn config(&self) -> TcpConfig {
        TcpConfig {
            interface: self.interface.clone(),
            most_listeners: self.most_listeners,
            most_connections: self.most_connections,
            most_unsent_bytes: self.most_unsent_bytes,
        }
    }
}
