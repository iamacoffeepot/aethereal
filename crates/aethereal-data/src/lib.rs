//! Plain data: values that are only their bytes.
//!
//! The byte layout is ADR-0002, the schema vocabulary ADR-0003, and the
//! `Plain` trait with the Rust types that implement it ADR-0004.

#![no_std]

extern crate alloc;
extern crate self as aethereal_data;
#[cfg(feature = "std")]
extern crate std;

mod budget;
mod error;
#[cfg(feature = "json")]
mod json;
mod key;
mod kind;
mod plain;
mod schema;

pub use budget::Budget;
pub use error::{DecodeError, EncodeError};
#[cfg(feature = "json")]
pub use json::{JsonError, JsonFault, JsonForm, JsonPath, JsonStep, TextFault, from_json, to_json};
pub use key::Key;
pub use kind::{Kind, KindId, KindName};
pub use plain::{Plain, from_bytes, to_bytes};
pub use schema::{
    Field, FieldNode, Fields, FieldsNode, Float, Integer, Schema, SchemaFault, SchemaNode, SchemaNodes, Shape, Variant,
    VariantNode,
};

/// Derives `Schema` and `Plain` from one field list (ADR-0004).
///
/// Every field must be `Plain`; a field that is not is a compile error
/// naming it.
///
/// ```compile_fail
/// use aethereal_data::Plain;
///
/// #[derive(Plain)]
/// struct HoldsCell {
///     cell: core::cell::Cell<u32>,
/// }
/// ```
///
/// Two variants sharing one selector is a compile error naming the second.
///
/// ```compile_fail
/// use aethereal_data::Plain;
///
/// #[derive(Plain)]
/// enum Dup {
///     #[plain(selector = 1)]
///     First,
///     #[plain(selector = 1)]
///     Second,
/// }
/// ```
///
/// A lifetime parameter is a compile error; decode owns its data.
///
/// ```compile_fail
/// use aethereal_data::Plain;
///
/// #[derive(Plain)]
/// struct Holder<'a> {
///     value: u32,
/// }
/// ```
pub use aethereal_data_derive::Plain;

/// Derives `Key` for a struct with exactly one field (ADR-0004).
///
/// Anything else is a compile error saying so.
///
/// ```compile_fail
/// use aethereal_data::{Key, Plain};
///
/// #[derive(Plain, Key)]
/// struct Pair {
///     first: u64,
///     second: u64,
/// }
/// ```
pub use aethereal_data_derive::Key;

/// Items the derives expand to; not part of the public interface.
#[doc(hidden)]
pub mod __derive {
    /// Reads a selector the emitted decode code calls.
    pub use crate::plain::read_selector;
    /// Writes a selector the emitted encode code calls.
    pub use crate::plain::write_selector;
    /// The vector the emitted encode code appends to.
    pub use alloc::vec::Vec;
}
