//! The walk from the tree to bytes, all shapes but struct and enum.

mod fields;
#[cfg(test)]
mod tests;

use super::layout::At;
use super::reader;
use super::tree::{Tree, Value};
use super::{JsonError, JsonFault, JsonForm, JsonStep, float, hex, integer, key};
use crate::plain::write_count;
use crate::{Budget, Float, Integer, SchemaNode, SchemaNodes};
use alloc::vec::Vec;

/// The walk from JSON to bytes.
struct Walk<'tree, 'json> {
    /// The tree the values are read from.
    tree: &'tree Tree<'json>,
}

/// The bytes of the value `json` states for `schema`; what the reader holds is charged to `budget` (ADR-0007).
pub fn from_json(schema: &SchemaNodes, json: &str, budget: &mut Budget) -> Result<Vec<u8>, JsonError> {
    let tree = reader::read(json, budget)?;
    let walk = Walk { tree: &tree };
    let mut out = Vec::new();
    walk.value(At::root(schema), tree.root(), &mut out)?;
    Ok(out)
}

/// The bytes of `value`, a part of `tree`, for `schema`: `from_json` for a value read without a schema.
pub fn from_value(schema: &SchemaNodes, tree: &Tree<'_>, value: &Value<'_>) -> Result<Vec<u8>, JsonError> {
    let walk = Walk { tree };
    let mut out = Vec::new();
    walk.value(At::root(schema), value, &mut out)?;
    Ok(out)
}

impl Walk<'_, '_> {
    /// Reads the value at `at` from `value` into `out`.
    ///
    /// Both walks follow the schema and may recurse: a call for a node only
    /// ever calls for a node that node names, and a flat schema's nodes name
    /// only earlier nodes, so the depth is bounded by the schema and never by
    /// the bytes or the JSON. A schema can itself arrive as bytes, so the
    /// bound is enforced: `At::child` is the only way down and refuses past
    /// the depth limit.
    fn value(&self, at: At<'_>, value: &Value<'_>, out: &mut Vec<u8>) -> Result<(), JsonError> {
        match at.node() {
            SchemaNode::Boolean => Self::boolean(value, out),
            SchemaNode::Integer(kind) => {
                let kind_value = *kind;
                Self::integer(kind_value, value, out)
            }
            SchemaNode::Float(kind) => {
                let kind_value = *kind;
                Self::float(kind_value, value, out)
            }
            SchemaNode::String => Self::string(value, out),
            SchemaNode::Bytes => Self::bytes(value, out),
            SchemaNode::Option(child) => {
                let child_index = *child;
                self.option(at, child_index, value, out)
            }
            SchemaNode::Sequence(child) => {
                let child_index = *child;
                self.sequence(at, child_index, value, out)
            }
            SchemaNode::Array { element, length } => {
                let element_index = *element;
                let length_value = *length;
                self.array(at, element_index, length_value, value, out)
            }
            SchemaNode::Map { key, value: inner } => {
                let key_index = *key;
                let value_index = *inner;
                self.map(at, key_index, value_index, value, out)
            }
            SchemaNode::Tuple { elements } => self.tuple(at, elements, value, out),
            SchemaNode::Struct { fields, .. } => self.struct_value(at, fields, value, out),
            SchemaNode::Enum { variants, .. } => self.enum_value(at, variants, value, out),
        }
    }

    /// Reads a boolean from `true` or `false`.
    fn boolean(value: &Value<'_>, out: &mut Vec<u8>) -> Result<(), JsonError> {
        let Value::Boolean(inner) = value else {
            return Err(JsonFault::Expected(JsonForm::Boolean).into());
        };
        let bit = *inner;
        let byte = u8::from(bit);
        out.push(byte);
        Ok(())
    }

    /// Reads an integer from a number or a string of digits.
    fn integer(kind: Integer, value: &Value<'_>, out: &mut Vec<u8>) -> Result<(), JsonError> {
        let wide = integer::is_wide(kind);
        let parsed = if wide {
            let Value::String(text) = value else {
                return Err(JsonFault::Expected(JsonForm::Digits).into());
            };
            integer::from_digits(text)?
        } else {
            let Value::Number(text) = value else {
                return Err(JsonFault::Expected(JsonForm::Number).into());
            };
            integer::from_number(text)?
        };
        integer::append(kind, parsed, out)?;
        Ok(())
    }

    /// Reads a float from a number or a named string.
    fn float(kind: Float, value: &Value<'_>, out: &mut Vec<u8>) -> Result<(), JsonError> {
        match value {
            Value::Number(text) => {
                float::from_number(kind, text, out)?;
                Ok(())
            }
            Value::String(name) => {
                float::from_name(kind, name, out)?;
                Ok(())
            }
            Value::Null | Value::Boolean(_) | Value::Array(_) | Value::Object(_) => {
                Err(JsonFault::Expected(JsonForm::Float).into())
            }
        }
    }

    /// Reads a string as a count and its bytes.
    fn string(value: &Value<'_>, out: &mut Vec<u8>) -> Result<(), JsonError> {
        let Value::String(text) = value else {
            return Err(JsonFault::Expected(JsonForm::String).into());
        };
        write_count(out, text.len())?;
        out.extend_from_slice(text.as_bytes());
        Ok(())
    }

    /// Reads bytes from a hexadecimal string.
    fn bytes(value: &Value<'_>, out: &mut Vec<u8>) -> Result<(), JsonError> {
        let Value::String(text) = value else {
            return Err(JsonFault::Expected(JsonForm::Hex).into());
        };
        let mut data = Vec::new();
        hex::read(text, &mut data)?;
        write_count(out, data.len())?;
        out.extend_from_slice(&data);
        Ok(())
    }

    /// Reads `null` or the value, unwrapping a one-element array when it could be `null`.
    fn option(&self, at: At<'_>, child: u32, value: &Value<'_>, out: &mut Vec<u8>) -> Result<(), JsonError> {
        let is_null = matches!(value, Value::Null);
        if is_null {
            out.push(0);
            return Ok(());
        }
        let child_at = at.child(child)?;
        let wrapped = child_at.could_be_null()?;
        if wrapped {
            let Value::Array(span) = value else {
                return Err(JsonFault::Expected(JsonForm::Array).into());
            };
            let span_value = *span;
            let elements = self.tree.elements(span_value);
            let found = elements.len();
            let single = found == 1;
            if !single {
                return Err(JsonFault::WrongLength { expected: 1, found }.into());
            }
            let Some(first) = elements.first() else {
                return Err(JsonFault::WrongLength { expected: 1, found }.into());
            };
            out.push(1);
            self.value(child_at, first, out)
        } else {
            out.push(1);
            self.value(child_at, value, out)
        }
    }

    /// Reads a count and its elements from an array.
    fn sequence(&self, at: At<'_>, child: u32, value: &Value<'_>, out: &mut Vec<u8>) -> Result<(), JsonError> {
        let Value::Array(span) = value else {
            return Err(JsonFault::Expected(JsonForm::Array).into());
        };
        let span_value = *span;
        let elements = self.tree.elements(span_value);
        let child_at = at.child(child)?;
        write_count(out, elements.len())?;
        for (position, element) in elements.iter().enumerate() {
            self.value(child_at, element, out).map_err(|error| error.within(JsonStep::Index(position)))?;
        }
        Ok(())
    }

    /// Reads an array: hexadecimal for bytes, otherwise exactly its length.
    fn array(
        &self,
        at: At<'_>,
        element: u32,
        length: u32,
        value: &Value<'_>,
        out: &mut Vec<u8>,
    ) -> Result<(), JsonError> {
        let child_at = at.child(element)?;
        let expected = SchemaNode::Integer(Integer::U8);
        let is_byte = child_at.node() == &expected;
        if is_byte {
            let Value::String(text) = value else {
                return Err(JsonFault::Expected(JsonForm::Hex).into());
            };
            let mut data = Vec::new();
            hex::read(text, &mut data)?;
            let expected_length = length as usize;
            let found = data.len();
            let right_length = found == expected_length;
            if !right_length {
                return Err(JsonFault::WrongLength { expected: expected_length, found }.into());
            }
            out.extend_from_slice(&data);
        } else {
            let Value::Array(span) = value else {
                return Err(JsonFault::Expected(JsonForm::Array).into());
            };
            let span_value = *span;
            let elements = self.tree.elements(span_value);
            let expected_length = length as usize;
            let found = elements.len();
            let right_length = found == expected_length;
            if !right_length {
                return Err(JsonFault::WrongLength { expected: expected_length, found }.into());
            }
            for (position, element_value) in elements.iter().enumerate() {
                self.value(child_at, element_value, out).map_err(|error| error.within(JsonStep::Index(position)))?;
            }
        }
        Ok(())
    }

    /// Reads a count and its pairs from an object, sorted into key order.
    fn map(&self, at: At<'_>, key: u32, value: u32, input: &Value<'_>, out: &mut Vec<u8>) -> Result<(), JsonError> {
        let Value::Object(span) = input else {
            return Err(JsonFault::Expected(JsonForm::Object).into());
        };
        let span_value = *span;
        let members = self.tree.members(span_value);
        let key_at = at.child(key)?;
        let shape = key::shape(key_at)?;
        let value_at = at.child(value)?;
        let mut entries: Vec<(key::KeyOrder, Vec<u8>)> = Vec::new();
        for member in members {
            let taken = key::from_name(shape, &member.name)
                .map_err(JsonError::from)
                .map_err(|error| error.within(JsonStep::Key(member.name.clone())))?;
            let mut buffer = Vec::new();
            buffer.extend_from_slice(&taken.bytes);
            self.value(value_at, &member.value, &mut buffer)
                .map_err(|error| error.within(JsonStep::Key(member.name.clone())))?;
            let order = taken.order;
            entries.push((order, buffer));
        }
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        write_count(out, entries.len())?;
        for (_, buffer) in entries {
            out.extend_from_slice(&buffer);
        }
        Ok(())
    }

    /// Reads each element in order.
    fn tuple(&self, at: At<'_>, elements: &[u32], value: &Value<'_>, out: &mut Vec<u8>) -> Result<(), JsonError> {
        let Value::Array(span) = value else {
            return Err(JsonFault::Expected(JsonForm::Array).into());
        };
        let span_value = *span;
        let items = self.tree.elements(span_value);
        let expected = elements.len();
        let found = items.len();
        let right_length = expected == found;
        if !right_length {
            return Err(JsonFault::WrongLength { expected, found }.into());
        }
        for (position, index) in elements.iter().enumerate() {
            let element_index = *index;
            let child_at = at.child(element_index)?;
            let Some(item) = items.get(position) else {
                return Err(JsonFault::WrongLength { expected, found }.into());
            };
            self.value(child_at, item, out).map_err(|error| error.within(JsonStep::Index(position)))?;
        }
        Ok(())
    }
}
