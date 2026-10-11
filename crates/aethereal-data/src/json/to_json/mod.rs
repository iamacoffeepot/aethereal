//! The walk from bytes to JSON, all shapes but struct and enum.

mod fields;
#[cfg(test)]
mod tests;

use super::layout::At;
use super::{JsonError, JsonStep, escape, float, hex, integer, key};
use crate::plain::{read_count, take_bytes};
use crate::{Budget, DecodeError, Float, Integer, SchemaNode, SchemaNodes};
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::str::from_utf8;

/// The walk from bytes to JSON.
struct Walk<'bytes, 'budget> {
    /// The bytes still to read.
    input: &'bytes [u8],
    /// The text written so far.
    out: String,
    /// The budget the text is charged to.
    budget: &'budget mut Budget,
    /// How many bytes of `out` have been charged.
    paid: usize,
}

/// The JSON text of the value `bytes` holds for `schema`; the text written is charged to `budget` (ADR-0007).
pub fn to_json(schema: &SchemaNodes, bytes: &[u8], budget: &mut Budget) -> Result<String, JsonError> {
    let mut walk = Walk { input: bytes, out: String::new(), budget, paid: 0 };
    walk.value(At::root(schema))?;
    walk.finish()
}

impl Walk<'_, '_> {
    /// Charges the text written since the last call.
    fn settle(&mut self) -> Result<(), JsonError> {
        let len = self.out.len();
        let fresh = len.saturating_sub(self.paid);
        self.budget.charge(fresh)?;
        self.paid = len;
        Ok(())
    }

    /// Refuses trailing bytes, settles, and returns the text.
    fn finish(mut self) -> Result<String, JsonError> {
        let trailing = self.input.len();
        let clean = trailing == 0;
        if !clean {
            return Err(DecodeError::TrailingBytes { count: trailing }.into());
        }
        self.settle()?;
        Ok(self.out)
    }

    /// Writes the value at `at` as JSON.
    ///
    /// Both walks follow the schema and may recurse: a call for a node only
    /// ever calls for a node that node names, and a flat schema's nodes name
    /// only earlier nodes, so the depth is bounded by the schema and never by
    /// the bytes or the JSON. A schema can itself arrive as bytes, so the
    /// bound is enforced: `At::child` is the only way down and refuses past
    /// the depth limit.
    fn value(&mut self, at: At<'_>) -> Result<(), JsonError> {
        self.settle()?;
        match at.node() {
            SchemaNode::Boolean => self.boolean(),
            SchemaNode::Integer(kind) => {
                let kind_value = *kind;
                self.integer(kind_value)
            }
            SchemaNode::Float(kind) => {
                let kind_value = *kind;
                self.float(kind_value)
            }
            SchemaNode::String => self.string(),
            SchemaNode::Bytes => self.bytes(),
            SchemaNode::Option(child) => {
                let child_index = *child;
                self.option(at, child_index)
            }
            SchemaNode::Sequence(child) => {
                let child_index = *child;
                self.sequence(at, child_index)
            }
            SchemaNode::Array { element, length } => {
                let element_index = *element;
                let length_value = *length;
                self.array(at, element_index, length_value)
            }
            SchemaNode::Map { key, value } => {
                let key_index = *key;
                let value_index = *value;
                self.map(at, key_index, value_index)
            }
            SchemaNode::Tuple { elements } => self.tuple(at, elements),
            SchemaNode::Struct { fields, .. } => self.struct_value(at, fields),
            SchemaNode::Enum { variants, .. } => self.enum_value(at, variants),
        }
    }

    /// Writes a boolean byte as `true` or `false`.
    fn boolean(&mut self) -> Result<(), JsonError> {
        let data = take_bytes(&mut self.input, 1)?;
        let Some(first) = data.first() else {
            return Err(DecodeError::UnexpectedEnd.into());
        };
        let byte = *first;
        let is_false = byte == 0;
        if is_false {
            self.out.push_str("false");
            return Ok(());
        }
        let is_true = byte == 1;
        if is_true {
            self.out.push_str("true");
            return Ok(());
        }
        Err(DecodeError::InvalidBoolean(byte).into())
    }

    /// Writes an integer's digits, quoted when wide.
    fn integer(&mut self, kind: Integer) -> Result<(), JsonError> {
        let value = integer::take(kind, &mut self.input)?;
        let wide = integer::is_wide(kind);
        let digits = value.to_string();
        if wide {
            self.out.push('"');
            self.out.push_str(&digits);
            self.out.push('"');
            return Ok(());
        }
        self.out.push_str(&digits);
        Ok(())
    }

    /// Writes a float in its JSON form.
    fn float(&mut self, kind: Float) -> Result<(), JsonError> {
        float::write(kind, &mut self.input, &mut self.out)?;
        Ok(())
    }

    /// Writes a string as a JSON string.
    fn string(&mut self) -> Result<(), JsonError> {
        let count = read_count(&mut self.input)?;
        let length = count as usize;
        let data = take_bytes(&mut self.input, length)?;
        let text = from_utf8(data).map_err(|_| DecodeError::InvalidUtf8)?;
        escape::write(text, &mut self.out);
        Ok(())
    }

    /// Writes bytes as lowercase hexadecimal in quotes.
    fn bytes(&mut self) -> Result<(), JsonError> {
        let count = read_count(&mut self.input)?;
        let length = count as usize;
        let data = take_bytes(&mut self.input, length)?;
        self.out.push('"');
        hex::write(data, &mut self.out);
        self.out.push('"');
        Ok(())
    }

    /// Writes `null` or the value, wrapped when it could be `null`.
    fn option(&mut self, at: At<'_>, child: u32) -> Result<(), JsonError> {
        let data = take_bytes(&mut self.input, 1)?;
        let Some(first) = data.first() else {
            return Err(DecodeError::UnexpectedEnd.into());
        };
        let byte = *first;
        let is_none = byte == 0;
        if is_none {
            self.out.push_str("null");
            return Ok(());
        }
        let is_some = byte == 1;
        if !is_some {
            return Err(DecodeError::InvalidOption(byte).into());
        }
        let child_at = at.child(child)?;
        let wrapped = child_at.could_be_null()?;
        if wrapped {
            self.out.push('[');
            self.value(child_at)?;
            self.out.push(']');
            return Ok(());
        }
        self.value(child_at)
    }

    /// Writes a count and its elements as an array.
    fn sequence(&mut self, at: At<'_>, child: u32) -> Result<(), JsonError> {
        let count = read_count(&mut self.input)?;
        let length = count as usize;
        let child_at = at.child(child)?;
        self.out.push('[');
        for position in 0..length {
            let first = position == 0;
            if !first {
                self.out.push(',');
            }
            self.value(child_at).map_err(|error| error.within(JsonStep::Index(position)))?;
        }
        self.out.push(']');
        Ok(())
    }

    /// Writes an array: hexadecimal for bytes, otherwise an array of exactly its length.
    fn array(&mut self, at: At<'_>, element: u32, length: u32) -> Result<(), JsonError> {
        let child_at = at.child(element)?;
        let expected = SchemaNode::Integer(Integer::U8);
        let is_byte = child_at.node() == &expected;
        if is_byte {
            let count = length as usize;
            let data = take_bytes(&mut self.input, count)?;
            self.out.push('"');
            hex::write(data, &mut self.out);
            self.out.push('"');
            return Ok(());
        }
        let count = length as usize;
        self.out.push('[');
        for position in 0..count {
            let first = position == 0;
            if !first {
                self.out.push(',');
            }
            self.value(child_at).map_err(|error| error.within(JsonStep::Index(position)))?;
        }
        self.out.push(']');
        Ok(())
    }

    /// Writes a count and its pairs as an object in the order read.
    fn map(&mut self, at: At<'_>, key: u32, value: u32) -> Result<(), JsonError> {
        let count = read_count(&mut self.input)?;
        let length = count as usize;
        let key_at = at.child(key)?;
        let shape = key::shape(key_at)?;
        let value_at = at.child(value)?;
        self.out.push('{');
        let mut orders = Vec::new();
        for position in 0..length {
            let first = position == 0;
            if !first {
                self.out.push(',');
            }
            let taken = key::take(shape, &mut self.input)?;
            let name = taken.name;
            let follows = orders.last().is_none_or(|prior| taken.order > *prior);
            if !follows {
                let refused: JsonError = DecodeError::KeyOrder.into();
                return Err(refused.within(JsonStep::Key(name)));
            }
            orders.clear();
            orders.push(taken.order);
            escape::write(&name, &mut self.out);
            self.out.push(':');
            self.value(value_at).map_err(|error| error.within(JsonStep::Key(name.clone())))?;
        }
        self.out.push('}');
        Ok(())
    }

    /// Writes each element in order.
    fn tuple(&mut self, at: At<'_>, elements: &[u32]) -> Result<(), JsonError> {
        self.out.push('[');
        for (position, index) in elements.iter().enumerate() {
            let first = position == 0;
            if !first {
                self.out.push(',');
            }
            let element_index = *index;
            let child_at = at.child(element_index)?;
            self.value(child_at).map_err(|error| error.within(JsonStep::Index(position)))?;
        }
        self.out.push(']');
        Ok(())
    }
}
