//! The MCP capability's settings (ADR-0028, ADR-0037).

use aethereal_data::Plain;

/// The MCP capability's section, `mcp`: its port, the sessions' route, its limits, and the origins it takes (ADR-0037,
/// ADR-0057). They are the root's config. Its own port is bound on the loopback, whatever `Tcp`'s setting, so only this
/// machine's programs reach it; the route is bound where its setting says.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.mcp.settings")]
#[settings]
pub struct McpSettings {
    /// The port served on the loopback, 0 for any the host picks: the owner's.
    #[flag]
    pub port: u16,
    /// The port of the sessions' route, 0 for any the host picks: every call on it bears a bearer the hub minted for
    /// one session (ADR-0057).
    #[flag]
    pub route_port: u16,
    /// The address the route is bound on: where the sessions' containers reach this machine, the gateway of Docker's
    /// default bridge unless set otherwise. A route that cannot be bound there is logged, and the owner's port is
    /// served without it.
    #[flag]
    pub route_interface: String,
    /// The most tools the root holds.
    #[flag]
    pub most_tools: u32,
    /// The most calls out at a time; past it, a call is a result with `isError` true.
    #[flag]
    pub most_calls: u32,
    /// Origins allowed besides this machine's, each compared exactly.
    pub origins: Vec<String>,
}

impl Default for McpSettings {
    fn default() -> Self {
        Self {
            port: 8890,
            route_port: 8893,
            route_interface: String::from("172.17.0.1"),
            most_tools: 256,
            most_calls: 64,
            origins: Vec::new(),
        }
    }
}
