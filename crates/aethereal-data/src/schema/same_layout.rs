//! Whether two flat schemas share a layout (ADR-0006).

use crate::kind::layout::{float_code, integer_code};
use crate::schema::nodes::node_index;
use crate::{FieldsNode, ProtocolNode, SchemaNode, SchemaNodes};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

impl SchemaNodes {
    /// Whether `other` is this schema once every name in both is removed (ADR-0006).
    ///
    /// Each node of each schema is looked up once, in stored order, so the work is in
    /// proportion to the two schemas' sizes, times the logarithm of the map's size.
    /// Nothing recurses and shared nodes are never unfolded.
    #[must_use]
    pub fn same_layout(&self, other: &Self) -> bool {
        let mut classes = Classes::new(Protocols::Compared);
        let ours = classes.root_class(self);
        let theirs = classes.root_class(other);

        ours == theirs
    }
}

/// The layout of each node of one schema but for the protocols its references name, against which other schemas are
/// matched (ADR-0054).
///
/// A field whose layout differs only in the protocols of the references it holds, at any depth, is read as the field
/// it is: its bytes are the same, and the protocol each reference names changes with the code that declares it.
///
/// Its nodes are classed once, as it is made, so matching a schema against one of them costs that schema's size alone.
pub(super) struct NodeLayouts {
    classes: Classes,
    nodes: Vec<u32>,
}

impl NodeLayouts {
    /// The layout of each node of `schema`, but for its references' protocols.
    pub(super) fn of(schema: &SchemaNodes) -> Self {
        let mut classes = Classes::new(Protocols::Ignored);
        let mut nodes = classes.earlier_classes(schema);
        let root = classes.class_of(nameless(schema.root(), &nodes, classes.protocols));
        nodes.push(root);

        Self { classes, nodes }
    }

    /// Whether the node at `index` has the layout of `other`'s root but for its references' protocols: false for an
    /// index past the schema's nodes.
    pub(super) fn matches(&mut self, index: u32, other: &SchemaNodes) -> bool {
        let theirs = self.classes.root_class(other);
        let ours = self.nodes.get(index as usize).copied();

        ours == Some(theirs)
    }
}

/// A node with every name removed and each child replaced by its layout class.
///
/// Two nodes have the same layout exactly when their nameless forms are equal.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Nameless {
    Boolean,
    Integer(u8),
    Float(u8),
    String,
    Bytes,
    Json,
    Option(u32),
    Sequence(u32),
    Array {
        element: u32,
        length: u32,
    },
    Map {
        key: u32,
        value: u32,
    },
    Struct(Vec<u32>),
    Tuple(Vec<u32>),
    Enum(Vec<NamelessVariant>),
    /// A reference: the class of what it is written as, and the protocol it names, which holds no names (ADR-0016).
    Reference {
        written: u32,
        protocol: NamelessProtocol,
    },
}

/// A reference's protocol as a layout holds it: what its node names, or nothing where protocols are ignored.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum NamelessProtocol {
    /// Classed where protocols are ignored: every reference written alike is one layout.
    Ignored,
    /// The protocol the node names.
    Named(ProtocolNode),
}

/// Whether a class's references are told apart by the protocols they name.
#[derive(Clone, Copy)]
enum Protocols {
    /// They are: two references naming different protocols are different layouts (ADR-0006, ADR-0016).
    Compared,
    /// They are not: a field read across a move is read whatever protocols its references name (ADR-0054).
    Ignored,
}

/// A variant with its name removed: its selector, then each field's layout class.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct NamelessVariant {
    selector: u32,
    fields: Vec<u32>,
}

/// The layout classes met so far: one number for each distinct nameless form, shared by every schema classed.
struct Classes {
    numbers: BTreeMap<Nameless, u32>,
    protocols: Protocols,
}

impl Classes {
    /// No classes yet, telling references apart by their protocols or not as `protocols` says.
    const fn new(protocols: Protocols) -> Self {
        Self { numbers: BTreeMap::new(), protocols }
    }

    /// The class of `schema`'s root, classing every node once in stored order.
    fn root_class(&mut self, schema: &SchemaNodes) -> u32 {
        let earlier = self.earlier_classes(schema);

        self.class_of(nameless(schema.root(), &earlier, self.protocols))
    }

    /// The class of every node of `schema` but the root, classing each once in stored order.
    fn earlier_classes(&mut self, schema: &SchemaNodes) -> Vec<u32> {
        let mut earlier = Vec::new();
        for node in schema.earlier() {
            let class = self.class_of(nameless(node, &earlier, self.protocols));
            earlier.push(class);
        }

        earlier
    }

    /// The number of `form`'s class: the one it already has, or the next unused one.
    fn class_of(&mut self, form: Nameless) -> u32 {
        let next = node_index(self.numbers.len());

        *self.numbers.entry(form).or_insert(next)
    }
}

/// The nameless form of `node`, given the class of every node before it, a reference's protocol kept or ignored as
/// `protocols` says.
fn nameless(node: &SchemaNode, earlier: &[u32], protocols: Protocols) -> Nameless {
    match node {
        SchemaNode::Boolean => Nameless::Boolean,
        SchemaNode::Integer(kind) => Nameless::Integer(integer_code(*kind)),
        SchemaNode::Float(kind) => Nameless::Float(float_code(*kind)),
        SchemaNode::String => Nameless::String,
        SchemaNode::Bytes => Nameless::Bytes,
        SchemaNode::Json => Nameless::Json,
        SchemaNode::Option(child) => Nameless::Option(class_at(earlier, *child)),
        SchemaNode::Sequence(child) => Nameless::Sequence(class_at(earlier, *child)),
        SchemaNode::Array { element, length } => {
            Nameless::Array { element: class_at(earlier, *element), length: *length }
        }
        SchemaNode::Map { key, value } => {
            Nameless::Map { key: class_at(earlier, *key), value: class_at(earlier, *value) }
        }
        SchemaNode::Struct { fields, .. } => {
            Nameless::Struct(fields.iter().map(|field| class_at(earlier, field.shape)).collect())
        }
        SchemaNode::Tuple { elements } => Nameless::Tuple(classes_at(earlier, elements)),
        SchemaNode::Enum { variants, .. } => Nameless::Enum(
            variants
                .iter()
                .map(|variant| NamelessVariant {
                    selector: variant.selector,
                    fields: field_classes(&variant.fields, earlier),
                })
                .collect(),
        ),
        SchemaNode::Reference { written, protocol } => {
            let protocol = match protocols {
                Protocols::Compared => NamelessProtocol::Named(protocol.clone()),
                Protocols::Ignored => NamelessProtocol::Ignored,
            };

            Nameless::Reference { written: class_at(earlier, *written), protocol }
        }
    }
}

/// The class of each of a variant's fields, named or not.
fn field_classes(fields: &FieldsNode, earlier: &[u32]) -> Vec<u32> {
    match fields {
        FieldsNode::Named(named) => named.iter().map(|field| class_at(earlier, field.shape)).collect(),
        FieldsNode::Unnamed(indices) => classes_at(earlier, indices),
    }
}

/// The class of each node `indices` names.
fn classes_at(earlier: &[u32], indices: &[u32]) -> Vec<u32> {
    indices.iter().map(|index| class_at(earlier, *index)).collect()
}

/// The class of the node at `index`.
///
/// A `SchemaNodes` names only earlier nodes, so the fallback below cannot be
/// reached but keeps the lookup total without panicking, as `hash_at` does.
fn class_at(earlier: &[u32], index: u32) -> u32 {
    earlier.get(index as usize).copied().unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kind::samples::{Command, NoteOn, Path, Point, Renamed, Reselected, Route, Spot, Wide, doubling};
    use crate::{Plain, Schema};

    #[derive(Plain)]
    struct Segment {
        from: Point,
        to: Point,
    }

    #[derive(Plain)]
    struct Dot {
        left: f32,
        top: f32,
    }

    #[derive(Plain)]
    struct Edge {
        head: Spot,
        tail: Dot,
    }

    #[test]
    fn renames_share_a_layout() {
        // Catches a comparison that looks at names inside the schema.
        assert!(SchemaNodes::of(&Path::SHAPE).same_layout(&SchemaNodes::of(&Route::SHAPE)));
        assert!(SchemaNodes::of(&Command::SHAPE).same_layout(&SchemaNodes::of(&Renamed::SHAPE)));
    }

    #[test]
    fn changed_shapes_share_no_layout() {
        // Catches a comparison blind to a field type, a selector, or a constructor.
        assert!(!SchemaNodes::of(&NoteOn::SHAPE).same_layout(&SchemaNodes::of(&Wide::SHAPE)));
        assert!(!SchemaNodes::of(&Command::SHAPE).same_layout(&SchemaNodes::of(&Reselected::SHAPE)));
        assert!(!SchemaNodes::of(&NoteOn::SHAPE).same_layout(&SchemaNodes::of(&<(u8, u8)>::SHAPE)));
        assert!(!SchemaNodes::of(&<(u8, u8)>::SHAPE).same_layout(&SchemaNodes::of(&<(u8, u8, u8)>::SHAPE)));
    }

    #[test]
    fn sharing_differences_do_not_matter() {
        // Catches a comparison of indices instead of layouts.
        let shared = SchemaNodes::of(&Segment::SHAPE);
        let split = SchemaNodes::of(&Edge::SHAPE);
        assert_ne!(shared.count(), split.count());
        assert!(shared.same_layout(&split));
        assert!(split.same_layout(&shared));
    }

    #[test]
    fn deep_shared_schemas_class_once() {
        // Catches any walk that unfolds sharing: 64 levels unfold to 2^64 leaves and never return.
        let ours = doubling(64, "ours", SchemaNode::Boolean);
        let theirs = doubling(64, "theirs", SchemaNode::Boolean);
        let different = doubling(64, "theirs", SchemaNode::String);
        assert!(ours.same_layout(&ours));
        assert!(ours.same_layout(&theirs));
        assert!(!ours.same_layout(&different));
    }
}
