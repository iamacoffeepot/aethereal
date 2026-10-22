//! A place in the schema, and how fields are written (ADR-0007).

use super::{JsonError, JsonFault};
use crate::{DecodeError, FieldNode, FieldsNode, SchemaFault, SchemaNode, SchemaNodes};
use alloc::string::ToString;
use alloc::vec::Vec;

/// The deepest schema nesting a walk follows.
pub const DEPTH_LIMIT: u32 = 128;

/// A place in a schema: a node, and how many nodes below the root it is.
#[derive(Debug, Clone, Copy)]
pub struct At<'schema> {
    schema: &'schema SchemaNodes,
    node: &'schema SchemaNode,
    level: u32,
}

impl<'schema> At<'schema> {
    /// The root of `schema`, at level zero.
    pub fn root(schema: &'schema SchemaNodes) -> Self {
        let node = schema.root();
        Self { schema, node, level: 0 }
    }

    /// The schema node at this place.
    pub fn node(self) -> &'schema SchemaNode {
        self.node
    }

    /// The node at `index`, one level down.
    pub fn child(self, index: u32) -> Result<Self, JsonError> {
        let level = self.level + 1;
        let deep = level > DEPTH_LIMIT;
        if deep {
            return Err(JsonFault::SchemaTooDeep.into());
        }

        let found = self.schema.node(index);
        let Some(node) = found else {
            let fault = SchemaFault::ForwardReference;
            return Err(DecodeError::InvalidSchema { node: index, fault }.into());
        };

        Ok(Self { schema: self.schema, node, level })
    }

    /// Whether a value here could itself be written `null`.
    pub fn could_be_null(self) -> Result<bool, JsonError> {
        let mut current = self;
        loop {
            match current.node {
                SchemaNode::Option(_) => {
                    return Ok(true);
                }
                SchemaNode::Struct { fields, .. } => {
                    let layout = Layout::of_struct(fields);
                    match layout {
                        Layout::Single(index) => {
                            current = current.child(index)?;
                        }
                        Layout::Empty | Layout::Several(_) | Layout::Named(_) => {
                            return Ok(false);
                        }
                    }
                }
                SchemaNode::Boolean
                | SchemaNode::Integer(_)
                | SchemaNode::Float(_)
                | SchemaNode::String
                | SchemaNode::Bytes
                | SchemaNode::Sequence(_)
                | SchemaNode::Array { .. }
                | SchemaNode::Map { .. }
                | SchemaNode::Tuple { .. }
                | SchemaNode::Enum { .. } => {
                    return Ok(false);
                }
            }
        }
    }
}

/// How a struct's or a variant's fields are written (ADR-0007).
#[derive(Debug, PartialEq, Eq)]
pub enum Layout<'schema> {
    /// No fields.
    Empty,
    /// One positional field: its value alone.
    Single(u32),
    /// Several positional fields: an array of them.
    Several(Vec<u32>),
    /// Named fields: an object.
    Named(&'schema [FieldNode]),
}

impl<'schema> Layout<'schema> {
    /// How a struct's fields are written: empty, positional, or named.
    pub fn of_struct(fields: &'schema [FieldNode]) -> Self {
        let empty = fields.is_empty();
        if empty {
            return Self::Empty;
        }

        let mut positional = true;
        for (position, field) in fields.iter().enumerate() {
            let expected = position.to_string();
            let matches = field.name == expected;
            if !matches {
                positional = false;
                break;
            }
        }

        if !positional {
            return Self::Named(fields);
        }

        let single = fields.len() == 1;
        if single {
            let Some(first) = fields.first() else {
                return Self::Empty;
            };
            return Self::Single(first.shape);
        }

        let indices = fields.iter().map(|field| field.shape).collect();
        Self::Several(indices)
    }

    /// How a variant's fields are written: empty, positional, or named.
    pub fn of_variant(fields: &'schema FieldsNode) -> Self {
        match fields {
            FieldsNode::Unnamed(indices) => {
                let empty = indices.is_empty();
                if empty {
                    return Self::Empty;
                }
                let single = indices.len() == 1;
                if single {
                    let Some(first) = indices.first() else {
                        return Self::Empty;
                    };
                    let index = *first;
                    return Self::Single(index);
                }
                Self::Several(indices.clone())
            }
            FieldsNode::Named(named) => {
                let empty = named.is_empty();
                if empty {
                    return Self::Empty;
                }
                Self::Named(named)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Budget, Plain, Schema, from_bytes, to_bytes};
    use alloc::string::String;
    use alloc::vec;

    #[derive(Plain)]
    struct One(u8);

    #[derive(Plain)]
    struct Two(u8, u8);

    #[derive(Plain)]
    struct Unit;

    #[derive(Plain)]
    struct Wrap(Option<u8>);

    #[derive(Plain)]
    struct DoubleWrap(Wrap);

    #[derive(Plain)]
    struct Named {
        inner: Option<u8>,
    }

    #[test]
    fn struct_layout_follows_names_and_positions() {
        // Catches a layout that parses names instead of comparing with positions.
        let two = SchemaNodes::of(&<Two as Schema>::SHAPE);
        let Some(SchemaNode::Struct { fields: two_fields, .. }) = two.node(1) else {
            panic!("two root is a struct");
        };
        assert_eq!(Layout::of_struct(two_fields), Layout::Several(vec![0, 0]));

        let one = SchemaNodes::of(&<One as Schema>::SHAPE);
        let Some(SchemaNode::Struct { fields: one_fields, .. }) = one.node(1) else {
            panic!("one root is a struct");
        };
        assert_eq!(Layout::of_struct(one_fields), Layout::Single(0));

        let mixed =
            vec![FieldNode { name: String::from("0"), shape: 0 }, FieldNode { name: String::from("x"), shape: 0 }];
        assert_eq!(Layout::of_struct(&mixed), Layout::Named(mixed.as_slice()));

        let swapped =
            vec![FieldNode { name: String::from("1"), shape: 0 }, FieldNode { name: String::from("0"), shape: 0 }];
        assert_eq!(Layout::of_struct(&swapped), Layout::Named(swapped.as_slice()));

        let unit = SchemaNodes::of(&<Unit as Schema>::SHAPE);
        let Some(SchemaNode::Struct { fields: unit_fields, .. }) = unit.node(0) else {
            panic!("unit root is a struct");
        };
        assert_eq!(Layout::of_struct(unit_fields), Layout::Empty);
    }

    #[test]
    fn could_be_null_follows_single_wrappers() {
        // Catches a null test that stops at a struct or looks inside a tuple.
        let option = SchemaNodes::of(&<Option<u8> as Schema>::SHAPE);
        assert_eq!(At::root(&option).could_be_null(), Ok(true));

        let byte = SchemaNodes::of(&<u8 as Schema>::SHAPE);
        assert_eq!(At::root(&byte).could_be_null(), Ok(false));

        let wrap = SchemaNodes::of(&<Wrap as Schema>::SHAPE);
        assert_eq!(At::root(&wrap).could_be_null(), Ok(true));

        let double = SchemaNodes::of(&<DoubleWrap as Schema>::SHAPE);
        assert_eq!(At::root(&double).could_be_null(), Ok(true));

        let tuple = SchemaNodes::of(&<(Option<u8>,) as Schema>::SHAPE);
        assert_eq!(At::root(&tuple).could_be_null(), Ok(false));

        let named = SchemaNodes::of(&<Named as Schema>::SHAPE);
        assert_eq!(At::root(&named).could_be_null(), Ok(false));
    }

    #[test]
    fn deep_schema_is_refused_at_the_limit() {
        // Catches a walk that follows a schema received as bytes without a cap.
        let mut nodes = vec![SchemaNode::Boolean];
        for position in 1..=200 {
            let child = u32::try_from(position - 1).unwrap_or(u32::MAX);
            nodes.push(SchemaNode::Option(child));
        }
        let bytes = to_bytes(&nodes);
        let is_ok = bytes.is_ok();
        assert!(is_ok);
        if let Ok(bytes) = bytes {
            let mut budget = Budget::new(1 << 20);
            let back = from_bytes::<SchemaNodes>(&bytes, &mut budget);
            let is_flat = back.is_ok();
            assert!(is_flat);
            if let Ok(flat) = back {
                let mut at = At::root(&flat);
                let mut depth = 0;
                loop {
                    let node = at.node();
                    let SchemaNode::Option(child) = node else {
                        break;
                    };
                    let index = *child;
                    let next = at.child(index);
                    let is_last = depth + 1 > DEPTH_LIMIT;
                    if is_last {
                        assert_eq!(next.map(|_| ()), Err(JsonFault::SchemaTooDeep.into()));
                        break;
                    }
                    let is_ok = next.is_ok();
                    assert!(is_ok);
                    if let Ok(next) = next {
                        at = next;
                        depth += 1;
                    } else {
                        break;
                    }
                }
                assert_eq!(depth, DEPTH_LIMIT);
            }
        }
    }
}
