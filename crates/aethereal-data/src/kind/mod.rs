//! Kinds: plain data with a name (ADR-0006).

mod fnv;
mod id;
pub mod layout;
mod name;
#[cfg(test)]
pub mod samples;

pub use id::KindId;
pub use name::KindName;

use crate::Plain;

/// Plain data with a name (ADR-0006).
pub trait Kind: Plain {
    /// The kind's name.
    const NAME: KindName;
}
