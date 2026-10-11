//! Actor paths: where an actor stands (ADR-0008).

mod actor;
mod child;
mod cut;
mod key;
#[cfg(test)]
mod samples;
mod segment;

pub use actor::{ActorPath, PathTooDeep};
pub use child::ChildKey;
pub use key::KeyName;
pub use segment::PathSegment;
