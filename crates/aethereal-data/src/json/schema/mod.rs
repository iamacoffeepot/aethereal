//! The JSON Schema of what `from_json` reads (ADR-0037).

#[cfg(test)]
mod tests;

use super::key::{self, KeyShape};
use super::layout::{At, Layout};
use super::{JsonError, escape, integer};
use crate::{Budget, FieldNode, Integer, SchemaNode, SchemaNodes, VariantNode};
use alloc::string::{String, ToString};

/// The digits of an unsigned integer, as ADR-0007 writes them.
const UNSIGNED_DIGITS: &str = "^(0|[1-9][0-9]*)$";
/// The digits of a signed integer, as ADR-0007 writes them.
const SIGNED_DIGITS: &str = "^(0|-?[1-9][0-9]*)$";

/// The walk from a schema to its JSON Schema.
struct Walk<'budget> {
    /// The text written so far.
    out: String,
    /// The budget the text is charged to.
    budget: &'budget mut Budget,
    /// How many bytes of `out` have been charged.
    paid: usize,
}

/// The JSON Schema, draft 2020-12, of the forms ADR-0007 gives `schema`; the text written is charged to `budget`.
///
/// A value `from_json` reads always meets it. The reverse holds but for what JSON Schema cannot say here: that a
/// 64-bit integer or a map's integer key is in range, that a float fits its width, and that an integer is written
/// without a fraction or an exponent, as `1.0` meets `integer`.
///
/// Each node is written where it is used, so a schema that shares nodes can write far more than it holds. The budget
/// is what bounds that.
///
/// # Errors
///
/// [`JsonFault::SchemaTooDeep`](super::JsonFault::SchemaTooDeep) past the depth limit, and a decode fault when the
/// budget is spent or a map's key is not a key shape.
pub fn json_schema(schema: &SchemaNodes, budget: &mut Budget) -> Result<String, JsonError> {
    let mut walk = Walk { out: String::new(), budget, paid: 0 };
    walk.value(At::root(schema))?;
    walk.settle()?;

    Ok(walk.out)
}

impl Walk<'_> {
    /// Charges the text written since the last call.
    fn settle(&mut self) -> Result<(), JsonError> {
        let len = self.out.len();
        let fresh = len.saturating_sub(self.paid);
        self.budget.charge(fresh)?;
        self.paid = len;

        Ok(())
    }

    /// Writes the JSON Schema of the node at `at`. Bounded as the other walks are: `At::child` is the only way down.
    fn value(&mut self, at: At<'_>) -> Result<(), JsonError> {
        self.settle()?;
        match at.node() {
            SchemaNode::Boolean => self.out.push_str("{\"type\":\"boolean\"}"),
            SchemaNode::Integer(kind) => self.integer(*kind),
            SchemaNode::Float(_) => {
                self.out
                    .push_str("{\"anyOf\":[{\"type\":\"number\"},{\"enum\":[\"NaN\",\"Infinity\",\"-Infinity\"]}]}");
            }
            SchemaNode::String => self.out.push_str("{\"type\":\"string\"}"),
            SchemaNode::Bytes => self.hex(),
            SchemaNode::Option(child) => self.option(at, *child)?,
            SchemaNode::Sequence(child) => {
                self.out.push_str("{\"type\":\"array\",\"items\":");
                self.value(at.child(*child)?)?;
                self.out.push('}');
            }
            SchemaNode::Array { element, length } => self.array(at, *element, *length)?,
            SchemaNode::Map { key, value } => self.map(at, *key, *value)?,
            SchemaNode::Tuple { elements } => self.positional(at, elements)?,
            SchemaNode::Struct { fields, .. } => self.fields(at, Layout::of_struct(fields))?,
            SchemaNode::Enum { variants, .. } => self.variants(at, variants)?,
        }

        Ok(())
    }

    /// Writes an integer: a number within its width, or a string of digits when wide.
    fn integer(&mut self, kind: Integer) {
        let wide = integer::is_wide(kind);
        if wide {
            self.digits(kind);
            return;
        }
        let (least, most) = bounds(kind);
        self.out.push_str("{\"type\":\"integer\",\"minimum\":");
        self.out.push_str(&least.to_string());
        self.out.push_str(",\"maximum\":");
        self.out.push_str(&most.to_string());
        self.out.push('}');
    }

    /// Writes a string of an integer's digits.
    fn digits(&mut self, kind: Integer) {
        let pattern = if is_signed(kind) {
            SIGNED_DIGITS
        } else {
            UNSIGNED_DIGITS
        };
        self.out.push_str("{\"type\":\"string\",\"pattern\":");
        escape::write(pattern, &mut self.out);
        self.out.push('}');
    }

    /// Writes a string of lowercase hexadecimal of any whole number of bytes.
    fn hex(&mut self) {
        self.out.push_str("{\"type\":\"string\",\"pattern\":\"^([0-9a-f]{2})*$\"}");
    }

    /// Writes a string of lowercase hexadecimal of exactly `length` bytes.
    fn hex_of(&mut self, length: u32) {
        let digits = (u64::from(length) * 2).to_string();
        self.out.push_str("{\"type\":\"string\",\"pattern\":\"^[0-9a-f]*$\",\"minLength\":");
        self.out.push_str(&digits);
        self.out.push_str(",\"maxLength\":");
        self.out.push_str(&digits);
        self.out.push('}');
    }

    /// Writes `null` or the value, or a one-element array of it where the value could itself be `null`.
    fn option(&mut self, at: At<'_>, child: u32) -> Result<(), JsonError> {
        let child_at = at.child(child)?;
        let wrapped = child_at.could_be_null()?;
        self.out.push_str("{\"anyOf\":[{\"type\":\"null\"},");
        if wrapped {
            self.out.push_str("{\"type\":\"array\",\"prefixItems\":[");
            self.value(child_at)?;
            self.out.push_str("],\"minItems\":1,\"maxItems\":1}");
        } else {
            self.value(child_at)?;
        }
        self.out.push_str("]}");

        Ok(())
    }

    /// Writes an array: hexadecimal for bytes, otherwise an array of exactly its length.
    fn array(&mut self, at: At<'_>, element: u32, length: u32) -> Result<(), JsonError> {
        let element_at = at.child(element)?;
        let expected = SchemaNode::Integer(Integer::U8);
        let is_byte = element_at.node() == &expected;
        if is_byte {
            self.hex_of(length);
            return Ok(());
        }
        let count = length.to_string();
        self.out.push_str("{\"type\":\"array\",\"items\":");
        self.value(element_at)?;
        self.out.push_str(",\"minItems\":");
        self.out.push_str(&count);
        self.out.push_str(",\"maxItems\":");
        self.out.push_str(&count);
        self.out.push('}');

        Ok(())
    }

    /// Writes an object whose names are keys and whose members are values.
    fn map(&mut self, at: At<'_>, key: u32, value: u32) -> Result<(), JsonError> {
        let shape = key::shape(at.child(key)?)?;
        self.out.push_str("{\"type\":\"object\",\"propertyNames\":");
        match shape {
            KeyShape::Integer(kind) => self.digits(kind),
            KeyShape::String => self.out.push_str("{\"type\":\"string\"}"),
            KeyShape::Bytes => self.hex(),
            KeyShape::ByteArray(length) => self.hex_of(length),
        }
        self.out.push_str(",\"additionalProperties\":");
        self.value(at.child(value)?)?;
        self.out.push('}');

        Ok(())
    }

    /// Writes fields by their layout. A variant with none is not written here: it is its name.
    fn fields(&mut self, at: At<'_>, layout: Layout<'_>) -> Result<(), JsonError> {
        match layout {
            Layout::Empty => self.out.push_str("{\"type\":\"object\",\"additionalProperties\":false}"),
            Layout::Single(index) => self.value(at.child(index)?)?,
            Layout::Several(indices) => self.positional(at, &indices)?,
            Layout::Named(named) => self.named(at, named)?,
        }

        Ok(())
    }

    /// Writes an array of exactly `indices`, each element its own shape.
    fn positional(&mut self, at: At<'_>, indices: &[u32]) -> Result<(), JsonError> {
        // `prefixItems` must hold a schema, so an empty tuple is only its length.
        if indices.is_empty() {
            self.out.push_str("{\"type\":\"array\",\"maxItems\":0}");
            return Ok(());
        }
        self.out.push_str("{\"type\":\"array\",\"prefixItems\":[");
        for (position, index) in indices.iter().enumerate() {
            if position != 0 {
                self.out.push(',');
            }
            self.value(at.child(*index)?)?;
        }
        let count = indices.len().to_string();
        self.out.push_str("],\"minItems\":");
        self.out.push_str(&count);
        self.out.push_str(",\"maxItems\":");
        self.out.push_str(&count);
        self.out.push('}');

        Ok(())
    }

    /// Writes an object of exactly `named`, each required but an option, which reads as none when it is left out.
    fn named(&mut self, at: At<'_>, named: &[FieldNode]) -> Result<(), JsonError> {
        self.out.push_str("{\"type\":\"object\",\"properties\":{");
        let mut required = String::new();
        for (position, field) in named.iter().enumerate() {
            if position != 0 {
                self.out.push(',');
            }
            escape::write(&field.name, &mut self.out);
            self.out.push(':');
            let child = at.child(field.shape)?;
            self.value(child)?;
            let is_option = matches!(child.node(), SchemaNode::Option(_));
            if is_option {
                continue;
            }
            if !required.is_empty() {
                required.push(',');
            }
            escape::write(&field.name, &mut required);
        }
        self.out.push_str("},\"required\":[");
        self.out.push_str(&required);
        self.out.push_str("],\"additionalProperties\":false}");

        Ok(())
    }

    /// Writes one form for each variant.
    fn variants(&mut self, at: At<'_>, variants: &[VariantNode]) -> Result<(), JsonError> {
        // `oneOf` must hold a schema, and an enum with no variants has no value.
        if variants.is_empty() {
            self.out.push_str("{\"not\":{}}");
            return Ok(());
        }
        self.out.push_str("{\"oneOf\":[");
        for (position, variant) in variants.iter().enumerate() {
            if position != 0 {
                self.out.push(',');
            }
            self.variant(at, variant)?;
        }
        self.out.push_str("]}");

        Ok(())
    }

    /// Writes a variant: its name with no fields, otherwise an object of one member, its name, holding its fields.
    fn variant(&mut self, at: At<'_>, variant: &VariantNode) -> Result<(), JsonError> {
        let layout = Layout::of_variant(&variant.fields);
        let is_empty = matches!(layout, Layout::Empty);
        if is_empty {
            self.out.push_str("{\"const\":");
            escape::write(&variant.name, &mut self.out);
            self.out.push('}');
            return Ok(());
        }
        self.out.push_str("{\"type\":\"object\",\"properties\":{");
        escape::write(&variant.name, &mut self.out);
        self.out.push(':');
        self.fields(at, layout)?;
        self.out.push_str("},\"required\":[");
        escape::write(&variant.name, &mut self.out);
        self.out.push_str("],\"additionalProperties\":false}");

        Ok(())
    }
}

/// The least and most an integer written as a number holds. The two written as strings have no bounds here.
fn bounds(kind: Integer) -> (i64, i64) {
    match kind {
        Integer::U8 => (0, i64::from(u8::MAX)),
        Integer::U16 => (0, i64::from(u16::MAX)),
        Integer::U32 => (0, i64::from(u32::MAX)),
        Integer::I8 => (i64::from(i8::MIN), i64::from(i8::MAX)),
        Integer::I16 => (i64::from(i16::MIN), i64::from(i16::MAX)),
        Integer::I32 => (i64::from(i32::MIN), i64::from(i32::MAX)),
        Integer::U64 => (0, i64::MAX),
        Integer::I64 => (i64::MIN, i64::MAX),
    }
}

/// Whether an integer may be negative.
fn is_signed(kind: Integer) -> bool {
    match kind {
        Integer::I8 | Integer::I16 | Integer::I32 | Integer::I64 => true,
        Integer::U8 | Integer::U16 | Integer::U32 | Integer::U64 => false,
    }
}
