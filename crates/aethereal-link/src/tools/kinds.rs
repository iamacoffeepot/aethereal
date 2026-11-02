//! What the agent writes to the hub's tools, and what it reads back, where the link's own kinds will not do
//! (ADR-0037, ADR-0038): `list_engines` takes and answers the link's own `ListEngines` and `EngineList`.

use aethereal_data::{Json, Plain};

/// The engine a tool is asked of, by its id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.engine_id")]
pub struct EngineId {
    pub engine: u32,
}

/// An engine's tools, as it last sent them, each written for the agent.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.engine_sheets")]
pub struct EngineSheets {
    pub tools: Vec<JsonSheet>,
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
