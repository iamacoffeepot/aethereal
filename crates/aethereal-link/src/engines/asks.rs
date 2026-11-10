//! What the hub's tools ask of its engines, and what an engine tells its root (ADR-0038, ADR-0040).

use aethereal_actor::Bytes;
use aethereal_data::{Bound, Plain};
use aethereal_process::ExitStatus;
use aethereal_store::{Binary, StoreHash};
use aethereal_tools::ToolSheet;

/// Asks `Engines`, at `path::<Engines>(())`, for every engine linked that has said `Hello`, answered with an
/// [`EngineList`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.list_engines")]
pub struct ListEngines;

/// Every engine linked that has said `Hello`, in the order of their ids, and the last engines the hub spawned that
/// have ended, the latest last (ADR-0040).
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.engine_list")]
pub struct EngineList {
    pub engines: Vec<EngineListing>,
    pub exited: Vec<ExitedEngine>,
}

/// One engine linked: its id, which is its `Engine`'s key, the name it gave, which two engines may share, how many
/// tools it holds, and where it came from.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.engine_listing")]
pub struct EngineListing {
    pub engine: u32,
    pub name: String,
    pub tools: u32,
    pub origin: Origin,
}

/// Where an engine linked came from (ADR-0040).
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.origin")]
pub enum Origin {
    /// The hub spawned it from its store: the binary as the spawn named it, and the hash of the build it runs.
    Spawned { binary: Binary, hash: StoreHash },
    /// It was started outside the hub, with a token from `MintToken`.
    External,
}

/// An engine the hub spawned that has ended: the binary as its spawn named it, how its program exited, and the head and
/// tail of its stderr, each bounded (ADR-0040).
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.exited_engine")]
pub struct ExitedEngine {
    pub binary: Binary,
    pub status: ExitStatus,
    pub stderr: String,
}

/// Asks an engine, at `path::<Engine>(engine)`, for its tools, answered with [`Described`]. An engine that has gone is
/// answered `No` by the host, as any place that holds no actor is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.describe_engine")]
pub struct DescribeEngine;

/// An engine's tools, as it last sent them.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.described")]
pub enum Described {
    /// Its set, and the set's revision, which a call of one of them names.
    Tools { revision: u64, tools: Vec<ToolSheet> },
    /// It has not sent its tools yet.
    NotYet,
}

/// Asks an engine, at `path::<Engine>(engine)`, to call its tool `tool` with `input`, bytes of the tool's input schema
/// in the set of `revision`, answered with the [`Called`](aethereal_tools::Called) the engine sends back. The engine
/// refuses a call of a set it has changed since.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.link.call_engine")]
pub struct CallEngine {
    pub revision: u64,
    pub tool: String,
    /// In a blob (ADR-0042), read into the frame the call is written to.
    pub input: Bytes,
}

/// What an engine tells its root it shows the agent, each time it changes: the name it gave, and how many tools it
/// holds. The root takes it only from its own engines.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.shown")]
pub struct Shown {
    pub name: String,
    pub tools: u32,
}
