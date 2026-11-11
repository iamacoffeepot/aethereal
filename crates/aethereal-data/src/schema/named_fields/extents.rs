//! Where each value in bytes ends, found by walking the schema they were written with (ADR-0002).

use crate::plain::{read_array, read_count, read_selector, take_bytes};
use crate::{DecodeError, FieldsNode, Float, Integer, SchemaFault, SchemaNode, SchemaNodes};
use alloc::vec::Vec;

/// How many bytes a value of a node's layout takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Size {
    /// Every value takes this many bytes; a size past what any input could hold saturates.
    Fixed(u64),
    /// Values take different numbers of bytes, and each takes at least one.
    Varies,
}

/// One step of the walk still to take.
#[derive(Debug, Clone, Copy)]
enum Step {
    /// One value of the node.
    One(u32),
    /// `left` more values of the node, one after another.
    Many { node: u32, left: u32 },
    /// `left` more key and value pairs of a map.
    Pairs { key: u32, value: u32, left: u32 },
}

/// A walk over bytes by a schema that finds where each value ends, without reading what it is.
///
/// It reads only what decides where a value ends: a count, an option's byte, a selector. It steps over a run of
/// values whose layout takes the same bytes every time in one step, and every value whose layout varies takes at
/// least one byte, so its work is bounded by the bytes and the schema, never by a count alone: four billion values
/// that take no bytes are one step. Its steps are kept on a stack of its own, which grows with the schema's fields
/// and never with a count.
pub(super) struct Extents<'a> {
    schema: &'a SchemaNodes,
    sizes: Vec<Size>,
    input: &'a [u8],
}

impl<'a> Extents<'a> {
    /// A walk over `input` by `schema`, at its front.
    pub(super) fn of(schema: &'a SchemaNodes, input: &'a [u8]) -> Self {
        Self { schema, sizes: sizes(schema), input }
    }

    /// The bytes of the next value, one of the node at `node`, which the walk steps over.
    pub(super) fn step_over(&mut self, node: u32) -> Result<&'a [u8], DecodeError> {
        let before = self.input;
        let mut steps = Vec::from([Step::One(node)]);
        while let Some(step) = steps.pop() {
            match step {
                Step::One(index) => self.one(index, &mut steps)?,
                Step::Many { node, left } => self.many(node, left, &mut steps)?,
                Step::Pairs { key, value, left } => self.pairs(key, value, left, &mut steps)?,
            }
        }

        let taken = before.len().saturating_sub(self.input.len());
        before.get(..taken).ok_or(DecodeError::UnexpectedEnd)
    }

    /// Refuses bytes left over after the last value stepped over.
    pub(super) fn finish(self) -> Result<(), DecodeError> {
        let count = self.input.len();
        let is_whole = count == 0;
        if is_whole {
            Ok(())
        } else {
            Err(DecodeError::TrailingBytes { count })
        }
    }

    /// Steps over one value of the node at `index`, or what of it decides where it ends, leaving the rest on `steps`.
    fn one(&mut self, index: u32, steps: &mut Vec<Step>) -> Result<(), DecodeError> {
        if let Size::Fixed(bytes) = self.size(index)? {
            return self.take(bytes);
        }

        match self.node(index)? {
            // Each of these takes the same bytes every time, so it was stepped over above.
            SchemaNode::Boolean | SchemaNode::Integer(_) | SchemaNode::Float(_) => {}
            SchemaNode::String | SchemaNode::Bytes | SchemaNode::Json => {
                let count = read_count(&mut self.input)?;
                self.take(u64::from(count))?;
            }
            SchemaNode::Option(child) => {
                let [marker] = read_array::<1>(&mut self.input)?;
                match marker {
                    0 => {}
                    1 => steps.push(Step::One(*child)),
                    other => return Err(DecodeError::InvalidOption(other)),
                }
            }
            SchemaNode::Sequence(element) => {
                let left = read_count(&mut self.input)?;
                steps.push(Step::Many { node: *element, left });
            }
            SchemaNode::Array { element, length } => steps.push(Step::Many { node: *element, left: *length }),
            SchemaNode::Map { key, value } => {
                let left = read_count(&mut self.input)?;
                steps.push(Step::Pairs { key: *key, value: *value, left });
            }
            SchemaNode::Struct { fields, .. } => steps.extend(fields.iter().rev().map(|field| Step::One(field.shape))),
            SchemaNode::Tuple { elements } => steps.extend(elements.iter().rev().map(|element| Step::One(*element))),
            SchemaNode::Enum { variants, .. } => {
                let selector = read_selector(&mut self.input)?;
                let Some(variant) = variants.iter().find(|variant| variant.selector == selector) else {
                    return Err(DecodeError::InvalidSelector { name: "named by a schema", selector });
                };
                match &variant.fields {
                    FieldsNode::Named(named) => steps.extend(named.iter().rev().map(|field| Step::One(field.shape))),
                    FieldsNode::Unnamed(shapes) => steps.extend(shapes.iter().rev().map(|shape| Step::One(*shape))),
                }
            }
        }

        Ok(())
    }

    /// Steps over `left` values of the node at `node`: all at once when each takes the same bytes, otherwise the next
    /// one, leaving the rest on `steps`.
    fn many(&mut self, node: u32, left: u32, steps: &mut Vec<Step>) -> Result<(), DecodeError> {
        if let Size::Fixed(each) = self.size(node)? {
            return self.take(each.saturating_mul(u64::from(left)));
        }

        if left > 0 {
            steps.push(Step::Many { node, left: left - 1 });
            steps.push(Step::One(node));
        }

        Ok(())
    }

    /// Steps over `left` pairs of a map: all at once when each takes the same bytes, otherwise the next one, leaving
    /// the rest on `steps`.
    fn pairs(&mut self, key: u32, value: u32, left: u32, steps: &mut Vec<Step>) -> Result<(), DecodeError> {
        let pair = add(self.size(key)?, self.size(value)?);
        if let Size::Fixed(each) = pair {
            return self.take(each.saturating_mul(u64::from(left)));
        }

        if left > 0 {
            steps.push(Step::Pairs { key, value, left: left - 1 });
            steps.push(Step::One(value));
            steps.push(Step::One(key));
        }

        Ok(())
    }

    /// The size of a value of the node at `index`.
    fn size(&self, index: u32) -> Result<Size, DecodeError> {
        self.sizes.get(index as usize).copied().ok_or_else(|| unknown(index))
    }

    /// The node at `index`.
    fn node(&self, index: u32) -> Result<&'a SchemaNode, DecodeError> {
        self.schema.node(index).ok_or_else(|| unknown(index))
    }

    /// Takes `bytes` bytes off the front of the input.
    fn take(&mut self, bytes: u64) -> Result<(), DecodeError> {
        let count = usize::try_from(bytes).unwrap_or(usize::MAX);
        take_bytes(&mut self.input, count)?;

        Ok(())
    }
}

/// The refusal of an index past the schema's nodes, which a flat schema never names (ADR-0003).
const fn unknown(index: u32) -> DecodeError {
    DecodeError::InvalidSchema { node: index, fault: SchemaFault::ForwardReference }
}

/// The size of a value of each node of `schema`, in stored order, the root last.
fn sizes(schema: &SchemaNodes) -> Vec<Size> {
    let mut sizes = Vec::new();
    for node in schema.earlier() {
        let size = size_of(node, &sizes);
        sizes.push(size);
    }
    let root = size_of(schema.root(), &sizes);
    sizes.push(root);

    sizes
}

/// The size of a value of `node`, given the size of every node before it.
fn size_of(node: &SchemaNode, earlier: &[Size]) -> Size {
    match node {
        SchemaNode::Boolean => Size::Fixed(1),
        SchemaNode::Integer(kind) => Size::Fixed(integer_bytes(*kind)),
        SchemaNode::Float(Float::F32) => Size::Fixed(4),
        SchemaNode::Float(Float::F64) => Size::Fixed(8),
        SchemaNode::String
        | SchemaNode::Bytes
        | SchemaNode::Json
        | SchemaNode::Option(_)
        | SchemaNode::Sequence(_)
        | SchemaNode::Map { .. }
        | SchemaNode::Enum { .. } => Size::Varies,
        SchemaNode::Array { element, length } => match size_at(earlier, *element) {
            Size::Fixed(each) => Size::Fixed(each.saturating_mul(u64::from(*length))),
            Size::Varies if *length == 0 => Size::Fixed(0),
            Size::Varies => Size::Varies,
        },
        SchemaNode::Struct { fields, .. } => {
            fields.iter().map(|field| size_at(earlier, field.shape)).fold(Size::Fixed(0), add)
        }
        SchemaNode::Tuple { elements } => {
            elements.iter().map(|element| size_at(earlier, *element)).fold(Size::Fixed(0), add)
        }
    }
}

/// The size of the node at `index` among `earlier`: one past them varies, and the walk refuses it when it gets there.
fn size_at(earlier: &[Size], index: u32) -> Size {
    earlier.get(index as usize).copied().unwrap_or(Size::Varies)
}

/// The size of one value of each size, one after the other.
const fn add(first: Size, second: Size) -> Size {
    match (first, second) {
        (Size::Fixed(first), Size::Fixed(second)) => Size::Fixed(first.saturating_add(second)),
        _ => Size::Varies,
    }
}

/// The bytes an integer of `kind` takes (ADR-0002).
const fn integer_bytes(kind: Integer) -> u64 {
    match kind {
        Integer::U8 | Integer::I8 => 1,
        Integer::U16 | Integer::I16 => 2,
        Integer::U32 | Integer::I32 => 4,
        Integer::U64 | Integer::I64 => 8,
    }
}
