//! The MCP capability (ADR-0037): one root, [`Mcp`], a server of MCP's revision `2026-07-28` on the HTTP capability.
//!
//! The root names no tool. Any actor may [`Offer`] it tools at its typed path, `path::<Mcp>(())`, naming a worker that
//! speaks [`Tooling`], and the root forwards each call to the worker, as bytes of the schemas in the tool's
//! [`ToolSheet`]. It watches each worker, and when one ends, drops its tools.
//!
//! The root needs nothing of its host but `Http`, so it is bootstrapped for every host. Its settings, the section
//! `mcp`, are its config.
//!
//! A tool is written in the crate it belongs to, as a [`Tool`], and a program lists what it serves in one root,
//! [`Toolbox`], which stands each up and offers it.

mod front;
mod mcp;
mod settings;
mod tool;
mod toolbox;
mod tooling;

pub use mcp::{Calling, Mcp, Serving, Unserved};
pub use settings::McpSettings;
pub use tool::{Tool, ToolList};
pub use toolbox::{Offering, Toolbox, ToolboxConfig, Unlisted};
pub use tooling::{Call, Called, Offer, Offered, ToolSheet, Tooling, Withdraw};
