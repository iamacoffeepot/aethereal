//! Actor paths: where an actor stands (ADR-0008).

mod actor;
mod cut;
mod id;
mod key;
#[cfg(test)]
mod samples;
mod segment;
mod typed;

pub use actor::{ActorPath, PathTooDeep};
pub use id::ActorId;
pub use key::KeyName;
pub use segment::PathSegment;
pub use typed::{ActorKey, Instanced, Keyed, Singleton, Then, TypedKey};
