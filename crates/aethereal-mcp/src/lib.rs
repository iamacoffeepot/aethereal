//! The MCP capability (ADR-0037): one root, [`Mcp`], a server of MCP's revision `2026-07-28` on the HTTP capability.
//!
//! The root names no tool. Any actor may [`Offer`] it tools at its typed path, `path::<Mcp>(())`, naming a worker that
//! speaks [`Tooling`], and the root forwards each call to the worker, as bytes of the schemas in the tool's
//! [`ToolSheet`]. It watches each worker, and when one ends, drops its tools.
//!
//! The root needs nothing of its host but `Http`, so it is bootstrapped for every host. Its settings, the section
//! `mcp`, are its config.

mod front;
mod mcp;
mod settings;
mod tooling;

pub use mcp::{Calling, Mcp, Serving, Unserved};
pub use settings::McpSettings;
pub use tooling::{Call, Called, Offer, Offered, ToolSheet, Tooling, Withdraw};
