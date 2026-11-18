//! The MCP capability's settings (ADR-0028, ADR-0037).

use aethereal_data::Plain;

/// The MCP capability's section, `mcp`: its limits, how long a tool may take, and the origins it takes (ADR-0037). They
/// are the root's config. Where it is served is its stacks' settings, each a section of its own (ADR-0035).
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.mcp.settings")]
#[settings]
pub struct McpSettings {
    /// The most tools the root holds.
    #[flag]
    pub most_tools: u32,
    /// The most calls out at a time; past it, a call is a result with `isError` true.
    #[flag]
    pub most_calls: u32,
    /// The most bytes of a request's body it reads, past which the request is answered `413` unread.
    pub most_body_bytes: u32,
    /// The milliseconds a tool may take to answer a call, past which the call is a result with `isError` true and its
    /// slot of `most_calls` comes back. The stack that asked the request times its own answer apart.
    pub call_within_millis: u32,
    /// Origins allowed besides this machine's, each compared exactly.
    pub origins: Vec<String>,
}

impl Default for McpSettings {
    fn default() -> Self {
        Self {
            most_tools: 256,
            most_calls: 64,
            most_body_bytes: 8 * 1024 * 1024,
            call_within_millis: 300_000,
            origins: Vec::new(),
        }
    }
}
