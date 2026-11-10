//! What an actor asks of the engine's tools root, and what it hears back (ADR-0053).

use crate::ToolSheet;
use aethereal_actor::Bytes;
use aethereal_data::{Bound, Plain};

/// Asks the tools root for every tool it holds: answered with [`Sheets`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.tools.list_tools")]
pub struct ListTools;

/// Every tool the tools root holds, in the order of their names, as the set of `revision`: a count the root raises
/// with every change to what it holds, and never resets.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.tools.sheets")]
pub struct Sheets {
    pub revision: u64,
    pub tools: Vec<ToolSheet>,
}

/// Asks the tools root to send this actor the [`Sheets`] as they stand, and again after every change to what it
/// holds: the sender must handle `Sheets`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.tools.subscribe")]
pub struct Subscribe;

/// A call of the tool named `tool`, with `input` a value of its input schema, as its bytes in a blob (ADR-0042): the
/// tools root asks it of the worker that offered the tool, and answers what the worker answered.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.tools.call_tool")]
pub struct CallTool {
    pub tool: String,
    pub input: Bytes,
}
