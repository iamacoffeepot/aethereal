//! What the agent writes to the hub's tools, and what it reads back, where the link's own kinds will not do
//! (ADR-0037, ADR-0038): `list_engines` takes and answers the link's own `ListEngines` and `EngineList`, `mint_token`
//! its `MintToken` and `Minted`, `spawn_engine` its `SpawnEngine`, and `terminate_engine` its `TerminateEngine`
//! (ADR-0040).

use aethereal_data::{Json, Plain};

/// Which of an engine's tools `describe_engine` shows, and how much of each.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.describe_tools")]
pub struct DescribeTools {
    pub engine: u32,
    pub show: ToolsShown,
}

/// What `describe_engine` shows of an engine's tools.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.tools_shown")]
pub enum ToolsShown {
    /// Every tool, by its name and the first sentence of its description.
    Names,
    /// The full sheets of the tools named, each of which the engine must have.
    Sheets(Vec<String>),
}

/// An engine's tools, as it last sent them, written for the agent as `describe_engine` was asked to show them.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.engine_description")]
pub struct EngineDescription {
    pub tools: DescribedTools,
}

/// An engine's tools as `describe_engine` shows them: each by its name, or the sheets asked for in full.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.described_tools")]
pub enum DescribedTools {
    /// Every tool, by its name and the first sentence of its description.
    Names(Vec<ToolSummary>),
    /// The sheets asked for, in the order the engine lists them.
    Sheets(Vec<JsonSheet>),
}

/// A tool as `describe_engine` lists it by name: its name and the first sentence of its description, without its
/// schemas.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.tool_summary")]
pub struct ToolSummary {
    pub name: String,
    pub description: String,
}

/// A tool's sheet with its input and output written as JSON Schema: a `ToolSheet`'s schemas have no JSON form of
/// their own.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.json_sheet")]
pub struct JsonSheet {
    pub name: String,
    pub description: String,
    pub input: Json,
    pub output: Json,
}

/// A call of an engine's tool: the engine, its tool, and the tool's input as JSON of its input schema.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.engine_call")]
pub struct EngineCall {
    pub engine: u32,
    pub tool: String,
    pub input: Json,
}

/// What an engine's tool answered, as JSON of its output schema.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.engine_answer")]
pub struct EngineAnswer {
    pub output: Json,
}

/// An engine the hub spawned, linked: its id, as `list_engines` shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.spawned_engine")]
pub struct SpawnedEngine {
    pub engine: u32,
}
