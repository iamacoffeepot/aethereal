//! Tools (ADR-0037, ADR-0038): workers that take bytes of a schema and answer bytes of another, and what a root that
//! holds them checks of each.
//!
//! A tool is written in the crate it belongs to, as a [`Tool`], and speaks [`Tooling`]. A [`Holder`] takes each
//! [`Offer`] of tools, and asks each call of the worker that offered it: `Mcp` in a hub, `Link` in an engine. A program
//! lists its tools in one root, [`Toolbox`], which stands each up and offers it to the holder its type names.
//!
//! The crate has no HTTP and no TCP, so a capability's `tools` feature brings in neither.

mod holder;
mod logs;
mod table;
mod tool;
mod toolbox;
mod tooling;

pub use holder::Holder;
pub use logs::{ActorLog, LogEntry, LogLevel, LoggedEntry, LogsTool, LogsToolConfig, ReadLog};
pub use table::{HeldOutput, HeldTool, Table};
pub use tool::{Tool, ToolList};
pub use toolbox::{Offering, Toolbox, ToolboxConfig, Unlisted};
pub use tooling::{Call, Called, Done, Offer, Offered, Output, ToolSheet, Tooling, Withdraw};
