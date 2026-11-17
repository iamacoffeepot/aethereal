//! Why a route is refused as the router is built (ADR-0035).

use core::error::Error;
use core::fmt;

/// Why a route is refused as the router is built, so the stack fails its `begin` and the program does not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unroutable {
    /// `pattern` matches a path `other` does, for one method.
    Overlaps { pattern: String, other: String },
    /// `pattern` is not `/` and then segments, or a capture in it is not a whole segment, or its name is empty or
    /// named twice.
    Malformed { pattern: String },
    /// The `{names}` in `pattern` are not the names its captures type reads, in its order.
    Uncaptured { pattern: String },
}

impl fmt::Display for Unroutable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Overlaps { pattern, other } => {
                write!(f, "the route `{pattern}` matches a path the route `{other}` does, for the same method")
            }
            Self::Malformed { pattern } => {
                write!(f, "the route `{pattern}` is not `/` and then segments, each a literal or a whole `{{name}}`")
            }
            Self::Uncaptured { pattern } => {
                write!(f, "the captures of the route `{pattern}` are not the names its captures type reads")
            }
        }
    }
}

impl Error for Unroutable {}
