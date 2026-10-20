//! Plain data: values that are only their bytes.
//!
//! The byte layout is ADR-0002, the schema vocabulary ADR-0003, and the
//! codec over a boundary, with `Plain` and the Rust types that implement it,
//! ADR-0004.

#![no_std]

extern crate alloc;
extern crate self as aethereal_data;
#[cfg(feature = "std")]
extern crate std;

mod boundary;
mod budget;
mod error;
mod fnv;
#[cfg(feature = "json")]
mod json;
mod key;
mod kind;
mod path;
mod plain;
mod schema;

pub use boundary::{Bare, Boundary, Decode, Encode, Reader, Writer};
pub use budget::Budget;
pub use error::{DecodeError, EncodeError};
pub use fnv::Fnv;
#[cfg(feature = "json")]
pub use json::{JsonError, JsonFault, JsonForm, JsonPath, JsonStep, TextFault, from_json, to_json};
pub use key::Key;
pub use kind::{Kind, KindId, KindName};
pub use path::{
    ActorId, ActorKey, ActorPath, Instanced, KeyName, Keyed, PathSegment, PathTooDeep, Singleton, Then, TypedKey,
};
pub use plain::{Plain, from_bytes, to_bytes};
pub use schema::{
    Field, FieldNode, Fields, FieldsNode, Float, Integer, Schema, SchemaFault, SchemaNode, SchemaNodes, Shape, Variant,
    VariantNode,
};

/// Derives `Schema`, the codec over every boundary, and `Plain` from one field list (ADR-0004).
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
/// A `Box` is its contents, so a boxed field builds as the field would.
///
/// ```
/// use aethereal_data::Plain;
///
/// #[derive(Plain)]
/// struct Node {
///     next: Option<Box<u32>>,
/// }
/// ```
///
/// A type that contains itself is a compile error, through a `Box` as
/// without one.
///
/// ```compile_fail
/// use aethereal_data::Plain;
///
/// #[derive(Plain)]
/// struct Node {
///     next: Option<Box<Node>>,
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

/// Derives `Schema` and the codec over every boundary, for a type that holds a bound value (ADR-0004).
///
/// It writes the codec `Plain` writes and does not state `Plain`: the type
/// crosses exactly the boundaries all its fields cross.
pub use aethereal_data_derive::Bound;

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
}
