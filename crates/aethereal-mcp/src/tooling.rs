//! What crosses between the root and a tool's worker (ADR-0037).

use aethereal_actor::{Carries, Protocol, ProtocolRef};
use aethereal_data::{Bound, Plain, SchemaNodes};

/// What a tool's worker speaks: each [`Call`], asked of it and answered with [`Called`].
pub struct Tooling;

impl Protocol for Tooling {
    type Kinds = (Call,);
}

impl Carries<Call> for Tooling {
    type Answers = (Called,);
}

/// A call of `tool`, named as one worker may serve many, with `input` a value of the tool's input schema.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.mcp.call")]
pub struct Call {
    pub tool: String,
    pub input: Vec<u8>,
}

/// What a call came to.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.mcp.called")]
pub enum Called {
    /// A value of the tool's output schema.
    Output(Vec<u8>),
    /// The tool could not do it, and says why, for the agent to read.
    Refused(String),
}

/// One tool as a worker offers it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.mcp.tool_sheet")]
pub struct ToolSheet {
    pub name: String,
    pub description: String,
    pub input: SchemaNodes,
    pub output: SchemaNodes,
}

/// Tools to hold, all or none, and the worker each call of them is asked of.
#[derive(Debug, Bound)]
#[kind(name = "aether.mcp.offer")]
pub struct Offer {
    pub tools: Vec<ToolSheet>,
    pub worker: ProtocolRef<Tooling>,
}

/// What an offer came to. Anything but `Yes` holds none of its tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.mcp.offered")]
pub enum Offered {
    /// Held.
    Yes,
    /// A name is already held, or is in the offer twice.
    Taken,
    /// It would take the root past its most tools.
    Full,
    /// An input's JSON form is not an object, as a call's arguments are.
    NotAnObject,
    /// A name is outside what MCP asks of one: 1 to 128 of ASCII letters, digits, `_`, `-`, and `.`.
    BadName,
    /// A schema has no JSON Schema: it is too deep or too large, or a map's key is not a key shape.
    Unwritable,
}

/// A tool to stop holding, when `worker` is the worker that holds it: another's of the same name stays.
#[derive(Debug, Clone, Bound)]
#[kind(name = "aether.mcp.withdraw")]
pub struct Withdraw {
    pub tool: String,
    pub worker: ProtocolRef<Tooling>,
}
