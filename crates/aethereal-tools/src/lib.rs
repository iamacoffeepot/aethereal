//! Tools (ADR-0037, ADR-0038, ADR-0053): workers that take bytes of a schema and answer bytes of another, and what a
//! root that holds them checks of each; and the engine's own, which read its log and its memory.
//!
//! A tool is written in the crate it belongs to, as a [`Tool`], and speaks [`Tooling`]. A [`Holder`] takes each
//! [`Offer`] of tools, and asks each call of the worker that offered it: `Mcp` in a hub, [`Tools`] in an engine. A
//! program lists its tools in one root, [`Toolbox`], which stands each up and offers it to the holder its type names.
//!
//! An engine holds its tools whether or not it links to a hub: any actor may [`ListTools`] or [`CallTool`] of
//! [`Tools`], or [`Subscribe`] to hear each change to what it holds, as `Link` does to hand the set to its hub.
//!
//! A tool's sheet carries the flags its input's fields mark, so a line typed at the dev overlay's console is read into
//! its input, and its answer printed, by [`CommandLine`] and [`print_value`] (ADR-0053).
//!
//! The crate has no HTTP and no TCP, so a capability's `tools` feature brings in neither.

mod command;
mod holder;
mod logs;
mod stats;
mod table;
mod tool;
mod toolbox;
mod tooling;
mod tools;
mod tuning;

pub use command::{CommandError, CommandLine, FlagsFault, print_image, print_value, split_words};
pub use holder::Holder;
pub use logs::{
    ActorLog, LogEntry, LogLevel, LoggedEntry, LogsTool, LogsToolConfig, ReadLog, ReadRecent, RecentEntry, RecentLog,
};
pub use stats::{
    BacklogFigure, EngineFigures, EngineStatsTool, EngineStatsToolConfig, GuestFigure, Meter, ProcessMemory,
    ReadEngineStats,
};
pub use table::{HeldOutput, HeldTool, Table};
pub use tool::{Tool, ToolList};
pub use toolbox::{Offering, Toolbox, ToolboxConfig, Unlisted};
pub use tooling::{Call, Called, Done, Offer, Offered, Output, ToolFlag, ToolSheet, Tooling, Typed, Withdraw};
pub use tools::{CallTool, Forwarding, ListTools, Sheets, Subscribe, Tools, ToolsSettings};
pub use tuning::{
    About, ActorTuning, ListTuning, Listed, Reading, Setting, TunableList, TuneFields, TuningList, TuningTool,
    TuningToolConfig,
};
