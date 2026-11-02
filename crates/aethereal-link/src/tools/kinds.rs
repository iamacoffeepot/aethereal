//! What the agent writes to the hub's tools, and what it reads back (ADR-0038).
//!
//! An engine's id is a `u32` here, where the link counts in `u64`: a 64-bit integer's JSON form is a string
//! (ADR-0007), and an agent names engine `3` as it is listed, not `"3"`.

use aethereal_data::{Json, Plain};

/// The input of `list_engines`: nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.list_engines_input")]
pub struct ListEnginesInput;

/// The output of `list_engines`: every engine linked that has said `Hello`, in the order of their ids.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.engines_listed")]
pub struct EnginesListed {
    pub engines: Vec<EngineListed>,
}

/// One engine as `list_engines` shows it: its id, the name it gave, which two engines may share, and how many tools
/// it holds.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.engine_listed")]
pub struct EngineListed {
    pub engine: u32,
    pub name: String,
    pub tools: u32,
}

/// The input of `describe_engine`: the engine's id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.describe_engine_input")]
pub struct DescribeEngineInput {
    pub engine: u32,
}

/// The output of `describe_engine`: the engine's tools, as it last sent them.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.engine_described")]
pub struct EngineDescribed {
    pub tools: Vec<ToolDescribed>,
}

/// One tool of an engine: its name, what it does, and its input and output, each written as JSON Schema.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.tool_described")]
pub struct ToolDescribed {
    pub name: String,
    pub description: String,
    pub input: Json,
    pub output: Json,
}

/// The input of `call_tool`: the engine, its tool, and the tool's input as JSON of its input schema.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.call_tool_input")]
pub struct CallToolInput {
    pub engine: u32,
    pub tool: String,
    pub input: Json,
}

/// The output of `call_tool`: the tool's output, as JSON of its output schema.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.call_tool_output")]
pub struct CallToolOutput {
    pub output: Json,
}
