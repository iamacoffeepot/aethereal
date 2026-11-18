//! The hub's two stacks (ADR-0035, ADR-0057), each the actor that owns one port: the owner's, on the loopback, and
//! the sessions', where their containers reach it. Each is its own section, which suggests no flags.

#[macro_use]
mod forward;
mod container;
mod local;

pub use container::{ContainerStack, defaults as container};
pub use local::{LocalStack, defaults as local};
