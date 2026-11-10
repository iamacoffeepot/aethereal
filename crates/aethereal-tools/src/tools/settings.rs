//! The tools root's settings (ADR-0028, ADR-0053).

use aethereal_data::Plain;

/// The tools root's section, `tools`: its limit. It is the root's config.
///
/// `most_tools` has no flag of its own, as `Mcp`'s setting of the same name has one and a hub may list both: it is set
/// as `--set tools.most_tools=<count>`.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.tools.settings")]
#[settings]
pub struct ToolsSettings {
    /// The most tools the root holds.
    pub most_tools: u32,
}

impl Default for ToolsSettings {
    fn default() -> Self {
        Self { most_tools: 256 }
    }
}
