//! Where a path a layer sets lands in the composite's schema (ADR-0028).

use super::document::Step;
use crate::json::layout::{At, Layout};
use crate::{SchemaNode, SchemaNodes};

/// A place in the composite's schema that a path reaches, followed through what JSON writes without a step of its
/// own: a struct of one positional field, and an option whose value is never written `null`.
#[derive(Debug, Clone, Copy)]
pub(super) enum Place<'schema> {
    /// A node of the schema.
    Schema(At<'schema>),
    /// Inside a value the schema has, where a path's steps are not followed: an enum's variant, an option written
    /// as an array. What is set here is JSON, and the one decode reads it.
    Opaque,
    /// A field the schema does not have. The one decode refuses whatever is set here, naming it.
    Unknown,
}

impl<'schema> Place<'schema> {
    /// The root of `schema`.
    pub(super) fn root(schema: &'schema SchemaNodes) -> Self {
        Self::Schema(At::root(schema)).settled()
    }

    /// The place one step further: a member or an element.
    pub(super) fn step(self, step: &Step) -> Self {
        match step {
            Step::Field(name) => self.field(name),
            Step::Index(index) => self.index(*index),
        }
    }

    /// The place of the member `name`: a struct's named field, or a map's value.
    pub(super) fn field(self, name: &str) -> Self {
        let Self::Schema(at) = self else {
            return self;
        };
        let child = match at.node() {
            SchemaNode::Struct { fields, .. } => {
                let Layout::Named(named) = Layout::of_struct(fields) else {
                    return Self::Opaque;
                };
                let found = named.iter().find(|field| field.name == name);
                let Some(field) = found else {
                    return Self::Unknown;
                };
                field.shape
            }
            SchemaNode::Map { value, .. } => *value,
            SchemaNode::Boolean
            | SchemaNode::Integer(_)
            | SchemaNode::Float(_)
            | SchemaNode::String
            | SchemaNode::Bytes
            | SchemaNode::Option(_)
            | SchemaNode::Sequence(_)
            | SchemaNode::Array { .. }
            | SchemaNode::Tuple { .. }
            | SchemaNode::Enum { .. } => return Self::Opaque,
        };

        Self::child(at, child)
    }

    /// The place of the element at `index`: of a sequence, an array, a tuple, or several positional fields.
    pub(super) fn index(self, index: usize) -> Self {
        let Self::Schema(at) = self else {
            return self;
        };
        let child = match at.node() {
            SchemaNode::Sequence(element) | SchemaNode::Array { element, .. } => Some(*element),
            SchemaNode::Tuple { elements } => elements.get(index).copied(),
            SchemaNode::Struct { fields, .. } => {
                let Layout::Several(indices) = Layout::of_struct(fields) else {
                    return Self::Opaque;
                };
                indices.get(index).copied()
            }
            SchemaNode::Boolean
            | SchemaNode::Integer(_)
            | SchemaNode::Float(_)
            | SchemaNode::String
            | SchemaNode::Bytes
            | SchemaNode::Option(_)
            | SchemaNode::Map { .. }
            | SchemaNode::Enum { .. } => return Self::Opaque,
        };
        let Some(child) = child else {
            return Self::Opaque;
        };

        Self::child(at, child)
    }

    /// Whether an object set here is merged by its members: the place is a struct of named fields.
    pub(super) fn by_fields(self) -> bool {
        let Self::Schema(at) = self else {
            return false;
        };
        let SchemaNode::Struct { fields, .. } = at.node() else {
            return false;
        };

        matches!(Layout::of_struct(fields), Layout::Named(_))
    }

    /// The node at `index` below `at`, settled; past the schema's depth the walk stops following it.
    fn child(at: At<'schema>, index: u32) -> Self {
        at.child(index).map_or(Self::Opaque, |child| Self::Schema(child).settled())
    }

    /// The place followed through a struct of one positional field and an option never written `null`.
    fn settled(self) -> Self {
        let mut place = self;
        loop {
            let Self::Schema(at) = place else {
                return place;
            };
            let inner = match at.node() {
                SchemaNode::Struct { fields, .. } => {
                    let Layout::Single(index) = Layout::of_struct(fields) else {
                        return place;
                    };
                    at.child(index)
                }
                SchemaNode::Option(index) => {
                    let Ok(child) = at.child(*index) else {
                        return Self::Opaque;
                    };
                    match child.could_be_null() {
                        Ok(false) => Ok(child),
                        Ok(true) | Err(_) => return Self::Opaque,
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
                | SchemaNode::Enum { .. } => return place,
            };
            place = inner.map_or(Self::Opaque, Self::Schema);
        }
    }
}
