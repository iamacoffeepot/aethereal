//! Which actor of a type stands in a place (ADR-0008).

use crate::Plain;
use crate::path::KeyName;

/// Which actor of its type this is among those in one place (ADR-0008).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Plain)]
pub enum ChildKey {
    /// The one actor of its type in its place.
    Only,
    /// One of several, told apart by name.
    Named(KeyName),
}
