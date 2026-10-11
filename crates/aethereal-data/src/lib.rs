//! Plain data: values that are only their bytes.
//!
//! The byte layout is ADR-0002, the schema vocabulary ADR-0003, and the
//! `Plain` trait with the Rust types that implement it ADR-0004.

#![no_std]

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

mod budget;
mod error;
mod key;
mod plain;
mod schema;

pub use budget::Budget;
pub use error::{DecodeError, EncodeError};
pub use key::Key;
pub use plain::{Plain, from_bytes, to_bytes};
pub use schema::{Field, Fields, Float, Integer, Schema, Shape, Variant};
