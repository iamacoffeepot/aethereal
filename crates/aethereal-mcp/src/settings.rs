//! The MCP capability's settings (ADR-0028, ADR-0037).

use aethereal_data::Plain;

/// The MCP capability's section, `mcp`: its port, its limits, and the origins it takes (ADR-0037). They are the root's
/// config. Where it binds is `Tcp`'s setting.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.mcp.settings")]
#[settings]
pub struct McpSettings {
    /// The port served, 0 for any the host picks.
    #[flag]
    pub port: u16,
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
        Self { port: 8890, most_tools: 256, most_calls: 64, origins: Vec::new() }
    }
}
