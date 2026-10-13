//! Kinds: plain data with a name (ADR-0006).

mod id;
pub mod layout;
mod name;
#[cfg(test)]
pub mod samples;

pub use id::KindId;
pub use name::KindName;

use crate::Schema;

/// A described type with a name (ADR-0006).
pub trait Kind: Schema {
    /// The kind's name.
    const NAME: KindName;
}
