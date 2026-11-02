//! The MCP capability (ADR-0037): one root, [`Mcp`], a server of MCP's revision `2026-07-28` on the HTTP capability.
//!
//! The root names no tool. It is a tools [`Holder`]: any actor may [`Offer`] it tools at its typed path,
//! `path::<Mcp>(())`, naming a worker that speaks [`Tooling`], and the root forwards each call to the worker, as bytes
//! of the schemas in the tool's [`ToolSheet`]. It watches each worker, and when one ends, drops its tools.
//!
//! The root needs nothing of its host but `Http`, so it is bootstrapped for every host. Its settings, the section
//! `mcp`, are its config.
//!
//! A tool is written in the crate it belongs to, as a [`Tool`], and a program lists what it serves in one root,
//! [`Toolbox`], which stands each up and offers it. A toolbox is listed after `Mcp`, and what `Mcp` needs after that:
//!
//! ```
//! # use aethereal_actor::Ordered;
//! # use aethereal_http::Http;
//! # use aethereal_mcp::Mcp;
//! # use aethereal_tcp::Tcp;
//! # use aethereal_tools::Toolbox;
//! # fn ordered<R: Ordered<I>, I>() {}
//! ordered::<(Toolbox<(), Mcp>, (Mcp, (Http, (Tcp, ())))), _>();
//! ```
//!
//! Before it, or with no `Mcp`, the program does not build:
//!
//! ```compile_fail
//! # use aethereal_actor::Ordered;
//! # use aethereal_http::Http;
//! # use aethereal_mcp::Mcp;
//! # use aethereal_tcp::Tcp;
//! # use aethereal_tools::Toolbox;
//! # fn ordered<R: Ordered<I>, I>() {}
//! ordered::<(Mcp, (Toolbox<(), Mcp>, (Http, (Tcp, ())))), _>();
//! ```
//!
//! [`Holder`]: aethereal_tools::Holder
//! [`Offer`]: aethereal_tools::Offer
//! [`Tooling`]: aethereal_tools::Tooling
//! [`ToolSheet`]: aethereal_tools::ToolSheet
//! [`Tool`]: aethereal_tools::Tool
//! [`Toolbox`]: aethereal_tools::Toolbox

mod front;
mod mcp;
mod settings;

pub use mcp::{Calling, Mcp, Serving, Unserved};
pub use settings::McpSettings;
