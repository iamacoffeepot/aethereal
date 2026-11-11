//! A struct value's top-level fields, found by name in its bytes with the schema they were written with (ADR-0054).

mod extents;
#[cfg(test)]
mod tests;

use super::same_layout::NodeLayouts;
use crate::{DecodeError, SchemaNode, SchemaNodes, Shape};
use alloc::vec::Vec;
use extents::Extents;

/// A struct value's top-level fields as its bytes hold them, each found by its name with the schema the bytes were
/// written with (ADR-0054).
///
/// The bytes are walked once, as it is made, by the schema alone (ADR-0007): each field's bytes begin where the
/// fields the schema lists before it end, whatever type later reads them. A field is then found by its name and
/// matched by its layout, the names inside it ignored (ADR-0006), and whoever asked for it reads its bytes whole as
/// the type it asked with. Only the top-level fields are found by name; a value nested inside one is read by layout.
pub struct NamedFields<'a> {
    fields: Vec<Located<'a>>,
    layouts: NodeLayouts,
}

/// One top-level field: its name, the node of its shape, and its bytes.
struct Located<'a> {
    name: &'a str,
    node: u32,
    bytes: &'a [u8],
}

/// What a struct holds under one field name, against the layout it is to be read with.
#[derive(Debug, PartialEq, Eq)]
pub enum FieldBytes<'a> {
    /// The struct has no field of that name.
    Absent,
    /// The struct has a field of that name, of another layout: reading its bytes as the layout asked for would read
    /// something else.
    OtherLayout,
    /// The struct has a field of that name and of that layout: its bytes.
    Same(&'a [u8]),
}

impl<'a> NamedFields<'a> {
    /// The top-level fields of the struct value `bytes` holds for `schema`.
    ///
    /// # Errors
    ///
    /// [`DecodeError::NotAStruct`] when `schema`'s root is not a struct. Otherwise why `bytes` are not a value of
    /// `schema` as far as where each value ends shows it: an option byte other than `0` or `1`, a selector that names
    /// no variant, input that ends early, or bytes left over. What lies inside a field is checked by the type that
    /// reads it.
    pub fn of(schema: &'a SchemaNodes, bytes: &'a [u8]) -> Result<Self, DecodeError> {
        let SchemaNode::Struct { fields, .. } = schema.root() else {
            return Err(DecodeError::NotAStruct);
        };

        let mut extents = Extents::of(schema, bytes);
        let mut located = Vec::with_capacity(fields.len());
        for field in fields {
            let bytes = extents.step_over(field.shape)?;
            located.push(Located { name: &field.name, node: field.shape, bytes });
        }
        extents.finish()?;

        Ok(Self { fields: located, layouts: NodeLayouts::of(schema) })
    }

    /// What the struct holds under `name`, to be read as a value of `shape`.
    pub fn field(&mut self, name: &str, shape: &Shape) -> FieldBytes<'a> {
        let Some(found) = self.fields.iter().find(|field| field.name == name) else {
            return FieldBytes::Absent;
        };

        let same = self.layouts.matches(found.node, &SchemaNodes::of(shape));
        if same {
            FieldBytes::Same(found.bytes)
        } else {
            FieldBytes::OtherLayout
        }
    }
}
