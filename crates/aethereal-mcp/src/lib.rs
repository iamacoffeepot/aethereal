//! The MCP capability (ADR-0037): one root, [`Mcp`], a server of MCP's revision `2026-07-28` on the HTTP capability.
//!
//! `Mcp` is a handler (ADR-0035): a program serves it on a port with a stack of its own, whose last layer is
//! [`McpRoutes`], `POST /mcp` to `Mcp`, and whose layers before it attach who sent each request, a [`Caller`]. On a
//! port on the loopback, every request is the owner's, which [`MarkOwner`] attaches; on the sessions' port an actor
//! layer that holds their bearers attaches the session each names (ADR-0057).
//!
//! The root names no tool. It is a tools [`Holder`]: any actor may [`Offer`] it tools at its typed path,
//! `path::<Mcp>(())`, naming a worker that speaks [`Tooling`], and the root forwards each call to the worker, as bytes
//! of the schemas in the tool's [`ToolSheet`], with its caller. It watches each worker, and when one ends, drops its
//! tools.
//!
//! The root needs nothing of its host, so it is bootstrapped for every host. Its settings, the section `mcp`, are its
//! config.
//!
//! A tool is written in the crate it belongs to, as a [`Tool`], and a program lists what it serves in one root,
//! [`Toolbox`], which stands each up and offers it. A toolbox is listed after `Mcp`:
//!
//! ```
//! # use aethereal_actor::Ordered;
//! # use aethereal_mcp::Mcp;
//! # use aethereal_tools::Toolbox;
//! # fn ordered<R: Ordered<I>, I>() {}
//! ordered::<(Toolbox<(), Mcp>, (Mcp, ())), _>();
//! ```
//!
//! Before it, or with no `Mcp`, the program does not build:
//!
//! ```compile_fail
//! # use aethereal_actor::Ordered;
//! # use aethereal_mcp::Mcp;
//! # use aethereal_tools::Toolbox;
//! # fn ordered<R: Ordered<I>, I>() {}
//! ordered::<(Mcp, (Toolbox<(), Mcp>, ())), _>();
//! ```
//!
//! [`Caller`]: aethereal_tools::Caller
//! [`Holder`]: aethereal_tools::Holder
//! [`Offer`]: aethereal_tools::Offer
//! [`Tooling`]: aethereal_tools::Tooling
//! [`ToolSheet`]: aethereal_tools::ToolSheet
//! [`Tool`]: aethereal_tools::Tool
//! [`Toolbox`]: aethereal_tools::Toolbox

mod front;
mod mcp;
mod owner;
mod routes;
mod settings;

pub use mcp::{Calling, Mcp, Reading};
pub use owner::MarkOwner;
pub use routes::McpRoutes;
pub use settings::McpSettings;
