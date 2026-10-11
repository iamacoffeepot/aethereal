//! A schema as plain data: nodes naming children by index (ADR-0003).

use crate::plain::write_count;
use crate::{Boundary, Decode, DecodeError, Encode, EncodeError, Field, Float, Integer, Plain, Schema, Shape, Writer};
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

/// A schema as plain data: nodes that name their children by index, children before parents, the root last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaNodes {
    earlier: Vec<SchemaNode>,
    root: SchemaNode,
}

/// One node of a flat schema (ADR-0003).
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
pub enum SchemaNode {
    Boolean,
    Integer(Integer),
    Float(Float),
    String,
    Bytes,
    Option(u32),
    Sequence(u32),
    Array { element: u32, length: u32 },
    Map { key: u32, value: u32 },
    Struct { name: String, fields: Vec<FieldNode> },
    Tuple { elements: Vec<u32> },
    Enum { name: String, variants: Vec<VariantNode> },
}

impl SchemaNode {
    /// Every index this node names.
    fn children(&self) -> Vec<u32> {
        match self {
            Self::Boolean | Self::Integer(_) | Self::Float(_) | Self::String | Self::Bytes => Vec::new(),
            Self::Option(child) | Self::Sequence(child) => Vec::from([*child]),
            Self::Array { element, .. } => Vec::from([*element]),
            Self::Map { key, value } => Vec::from([*key, *value]),
            Self::Struct { fields, .. } => fields.iter().map(|field| field.shape).collect(),
            Self::Tuple { elements } => elements.clone(),
            Self::Enum { variants, .. } => {
                let mut out = Vec::new();
                for variant in variants {
                    match &variant.fields {
                        FieldsNode::Named(named) => {
                            out.extend(named.iter().map(|field| field.shape));
                        }
                        FieldsNode::Unnamed(indices) => {
                            out.extend(indices.iter().copied());
                        }
                    }
                }
                out
            }
        }
    }
}

/// One struct field naming its shape by index (ADR-0003).
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
pub struct FieldNode {
    /// The field's name; for readers that need it, never in the bytes.
    pub name: String,
    /// The field's shape as an index into the node list.
    pub shape: u32,
}

/// One enum variant naming its fields by index (ADR-0003).
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
pub struct VariantNode {
    /// The variant's name; for readers that need it, never in the bytes.
    pub name: String,
    /// The variant's declared number on the wire, not its position.
    pub selector: u32,
    /// The variant's fields in order.
    pub fields: FieldsNode,
}

/// The fields of one flat variant (ADR-0003).
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
pub enum FieldsNode {
    Named(Vec<FieldNode>),
    Unnamed(Vec<u32>),
}

/// Why a received flat schema is refused (ADR-0003).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaFault {
    /// There are no nodes, so there is no root.
    Empty,
    /// A node names an index at or above its own.
    ForwardReference,
    /// A map's key is not one of the key shapes ADR-0002 lists.
    KeyShape,
    /// A node equals an earlier node.
    Repeated,
    /// A node other than the last is named by no later node.
    Unreferenced,
    /// A struct or a variant has two fields of one name.
    RepeatedField,
    /// An enum has two variants of one name.
    RepeatedVariant,
    /// An enum has two variants of one selector.
    RepeatedSelector,
}

impl fmt::Display for SchemaFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("no nodes, so no root"),
            Self::ForwardReference => f.write_str("a node names an index at or above its own"),
            Self::KeyShape => f.write_str("a map key is not a key shape"),
            Self::Repeated => f.write_str("a node equals an earlier node"),
            Self::Unreferenced => f.write_str("a node is named by no later node"),
            Self::RepeatedField => f.write_str("a struct or a variant has two fields of one name"),
            Self::RepeatedVariant => f.write_str("an enum has two variants of one name"),
            Self::RepeatedSelector => f.write_str("an enum has two variants of one selector"),
        }
    }
}

impl SchemaNodes {
    /// The flat form of `shape`: children before parents, equal shapes sharing one node, the root last.
    #[must_use]
    pub fn of(shape: &Shape) -> Self {
        let mut earlier = Vec::new();
        let root = node_of(shape, &mut earlier);

        Self { earlier, root }
    }

    /// The root node: the last one.
    #[must_use]
    pub fn root(&self) -> &SchemaNode {
        &self.root
    }

    /// Every node but the root, in stored order.
    pub(crate) fn earlier(&self) -> &[SchemaNode] {
        &self.earlier
    }

    /// The node at `index`, or `None` past the end. The root is at `len() - 1`.
    #[must_use]
    pub fn node(&self, index: u32) -> Option<&SchemaNode> {
        let position = index as usize;
        let root_position = self.earlier.len();
        let is_root = position == root_position;
        if is_root {
            Some(&self.root)
        } else {
            self.earlier.get(position)
        }
    }

    /// The number of nodes, root included.
    #[must_use]
    pub fn count(&self) -> u32 {
        node_index(self.earlier.len() + 1)
    }
}

impl Schema for SchemaNodes {
    const SHAPE: Shape =
        Shape::Struct { name: "SchemaNodes", fields: &[Field { name: "nodes", shape: Vec::<SchemaNode>::SHAPE }] };
}

impl<B: Boundary> Encode<B> for SchemaNodes {
    fn encode(&self, out: &mut B::Writer<'_>) -> Result<(), EncodeError> {
        let total = self.earlier.len() + 1;
        write_count(out.bytes(), total)?;
        for node in &self.earlier {
            Encode::<B>::encode(node, out)?;
        }
        Encode::<B>::encode(&self.root, out)
    }
}

impl<B: Boundary> Decode<B> for SchemaNodes {
    fn decode(input: &mut B::Reader<'_>) -> Result<Self, DecodeError> {
        let mut nodes = <Vec<SchemaNode> as Decode<B>>::decode(input)?;
        reject_forward(&nodes)?;
        reject_bad_key(&nodes)?;
        reject_repeated(&nodes)?;
        reject_unreferenced(&nodes)?;
        reject_repeated_field(&nodes)?;
        reject_repeated_variant(&nodes)?;
        reject_repeated_selector(&nodes)?;

        let Some(root) = nodes.pop() else {
            return Err(DecodeError::InvalidSchema { node: 0, fault: SchemaFault::Empty });
        };

        Ok(Self { earlier: nodes, root })
    }
}

impl Plain for SchemaNodes {}

/// Converts a position in the node list to its wire index.
///
/// Positions come from a vector built either by walking a finite `Shape` or by
/// decoding a `u32` count, so they always fit in a `u32`; the fallback below
/// cannot be reached but keeps the conversion total without panicking.
pub(super) fn node_index(position: usize) -> u32 {
    u32::try_from(position).unwrap_or(u32::MAX)
}

/// Returns the shape's own node, interning its children first without pushing it.
///
/// A `Shape` is a finite tree, so the recursion below is bounded by the type.
fn node_of(shape: &Shape, earlier: &mut Vec<SchemaNode>) -> SchemaNode {
    match *shape {
        Shape::Boolean => SchemaNode::Boolean,
        Shape::Integer(kind) => SchemaNode::Integer(kind),
        Shape::Float(kind) => SchemaNode::Float(kind),
        Shape::String => SchemaNode::String,
        Shape::Bytes => SchemaNode::Bytes,
        Shape::Option(inner) => {
            let child = intern(inner, earlier);
            SchemaNode::Option(child)
        }
        Shape::Sequence(inner) => {
            let child = intern(inner, earlier);
            SchemaNode::Sequence(child)
        }
        Shape::Array { element, length } => {
            let child = intern(element, earlier);
            SchemaNode::Array { element: child, length }
        }
        Shape::Map { key, value } => {
            let key_child = intern(key, earlier);
            let value_child = intern(value, earlier);
            SchemaNode::Map { key: key_child, value: value_child }
        }
        Shape::Struct { name, fields } => {
            let mut flat = Vec::new();
            for field in fields {
                let child = intern(&field.shape, earlier);
                let entry = FieldNode { name: String::from(field.name), shape: child };
                flat.push(entry);
            }
            SchemaNode::Struct { name: String::from(name), fields: flat }
        }
        Shape::Tuple { elements } => {
            let mut flat = Vec::new();
            for element in elements {
                let child = intern(element, earlier);
                flat.push(child);
            }
            SchemaNode::Tuple { elements: flat }
        }
        Shape::Enum { name, variants } => {
            let mut flat = Vec::new();
            for variant in variants {
                let fields = intern_fields(&variant.fields, earlier);
                let entry = VariantNode { name: String::from(variant.name), selector: variant.selector, fields };
                flat.push(entry);
            }
            SchemaNode::Enum { name: String::from(name), variants: flat }
        }
    }
}

/// Returns the index for `shape`, pushing its node when no equal node is present.
fn intern(shape: &Shape, earlier: &mut Vec<SchemaNode>) -> u32 {
    let node = node_of(shape, earlier);
    find_or_push(node, earlier)
}

/// Interns one variant's fields.
fn intern_fields(fields: &crate::Fields, earlier: &mut Vec<SchemaNode>) -> FieldsNode {
    match fields {
        crate::Fields::Named(named) => {
            let mut flat = Vec::new();
            for field in *named {
                let child = intern(&field.shape, earlier);
                let entry = FieldNode { name: String::from(field.name), shape: child };
                flat.push(entry);
            }
            FieldsNode::Named(flat)
        }
        crate::Fields::Unnamed(shapes) => {
            let mut flat = Vec::new();
            for shape in *shapes {
                let child = intern(shape, earlier);
                flat.push(child);
            }
            FieldsNode::Unnamed(flat)
        }
    }
}

/// Returns the existing index for `node` or pushes it.
fn find_or_push(node: SchemaNode, earlier: &mut Vec<SchemaNode>) -> u32 {
    for (position, existing) in earlier.iter().enumerate() {
        let is_equal = *existing == node;
        if is_equal {
            return node_index(position);
        }
    }

    let index = node_index(earlier.len());
    earlier.push(node);

    index
}

/// Refuses the first node naming an index at or above its own.
fn reject_forward(nodes: &[SchemaNode]) -> Result<(), DecodeError> {
    for (position, node) in nodes.iter().enumerate() {
        let own = node_index(position);
        let children = node.children();
        let is_forward = children.into_iter().any(|child| child >= own);
        if is_forward {
            let fault = SchemaFault::ForwardReference;

            return Err(DecodeError::InvalidSchema { node: own, fault });
        }
    }

    Ok(())
}

/// Refuses the first map whose key is not a key shape.
fn reject_bad_key(nodes: &[SchemaNode]) -> Result<(), DecodeError> {
    for (position, node) in nodes.iter().enumerate() {
        if let SchemaNode::Map { key, .. } = node {
            let key_ok = is_key_shape(nodes, *key);
            if !key_ok {
                let index = node_index(position);
                let fault = SchemaFault::KeyShape;

                return Err(DecodeError::InvalidSchema { node: index, fault });
            }
        }
    }

    Ok(())
}

/// Whether the node at `index` is a key shape: an integer, string, bytes,
/// an array of `U8`, or a struct of exactly one key shape, following single-field structs by loop.
fn is_key_shape(nodes: &[SchemaNode], mut index: u32) -> bool {
    loop {
        let position = index as usize;
        let found = nodes.get(position);
        let Some(node) = found else {
            return false;
        };

        match node {
            SchemaNode::Integer(_) | SchemaNode::String | SchemaNode::Bytes => {
                return true;
            }
            SchemaNode::Array { element, .. } => {
                let element_position = *element as usize;
                let element_node = nodes.get(element_position);
                let Some(element_node) = element_node else {
                    return false;
                };
                let is_bytes = *element_node == SchemaNode::Integer(Integer::U8);

                return is_bytes;
            }
            SchemaNode::Struct { fields, .. } => {
                let count = fields.len();
                let is_single = count == 1;
                if is_single {
                    let first = fields.first();
                    let Some(field) = first else {
                        return false;
                    };
                    index = field.shape;
                } else {
                    return false;
                }
            }
            SchemaNode::Boolean
            | SchemaNode::Float(_)
            | SchemaNode::Option(_)
            | SchemaNode::Sequence(_)
            | SchemaNode::Map { .. }
            | SchemaNode::Tuple { .. }
            | SchemaNode::Enum { .. } => {
                return false;
            }
        }
    }
}

/// Refuses the first node equal to an earlier one.
fn reject_repeated(nodes: &[SchemaNode]) -> Result<(), DecodeError> {
    for (position, node) in nodes.iter().enumerate() {
        let mut duplicate = false;
        for earlier in nodes.iter().take(position) {
            let is_equal = *earlier == *node;
            duplicate = duplicate || is_equal;
        }

        if duplicate {
            let index = node_index(position);
            let fault = SchemaFault::Repeated;

            return Err(DecodeError::InvalidSchema { node: index, fault });
        }
    }

    Ok(())
}

/// Refuses the first non-root node named by no later node.
fn reject_unreferenced(nodes: &[SchemaNode]) -> Result<(), DecodeError> {
    let length = nodes.len();
    let last_position = length.saturating_sub(1);

    for (position, _) in nodes.iter().enumerate() {
        let is_root = position == last_position;
        if is_root {
            continue;
        }

        let target = node_index(position);
        let mut named = false;
        for later in nodes.iter().skip(position + 1) {
            let direct = later.children().contains(&target);
            named = named || direct;
        }

        if !named {
            let fault = SchemaFault::Unreferenced;

            return Err(DecodeError::InvalidSchema { node: target, fault });
        }
    }

    Ok(())
}

/// Whether two fields share one name, comparing sorted names side by side.
fn has_repeated_field_name(fields: &[FieldNode]) -> bool {
    let mut names: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
    names.sort_unstable();
    let mut repeated = false;
    for window in names.windows(2) {
        let first = window.first();
        let second = window.get(1);
        let same = first == second;
        if same {
            repeated = true;
            break;
        }
    }

    repeated
}

/// Whether two variants share one name, comparing sorted names side by side.
fn has_repeated_variant_name(variants: &[VariantNode]) -> bool {
    let mut names: Vec<&str> = variants.iter().map(|variant| variant.name.as_str()).collect();
    names.sort_unstable();
    let mut repeated = false;
    for window in names.windows(2) {
        let first = window.first();
        let second = window.get(1);
        let same = first == second;
        if same {
            repeated = true;
            break;
        }
    }

    repeated
}

/// Whether two variants share one selector, comparing sorted selectors side by side.
fn has_repeated_variant_selector(variants: &[VariantNode]) -> bool {
    let mut selectors: Vec<u32> = variants.iter().map(|variant| variant.selector).collect();
    selectors.sort_unstable();
    let mut repeated = false;
    for window in selectors.windows(2) {
        let first = window.first();
        let second = window.get(1);
        let same = first == second;
        if same {
            repeated = true;
            break;
        }
    }

    repeated
}

/// Refuses the first struct or variant with two fields of one name.
fn reject_repeated_field(nodes: &[SchemaNode]) -> Result<(), DecodeError> {
    for (position, node) in nodes.iter().enumerate() {
        let mut repeated = false;
        if let SchemaNode::Struct { fields, .. } = node {
            repeated = has_repeated_field_name(fields);
        } else if let SchemaNode::Enum { variants, .. } = node {
            for variant in variants {
                let variant_repeated = match &variant.fields {
                    FieldsNode::Named(named) => has_repeated_field_name(named),
                    FieldsNode::Unnamed(_) => false,
                };
                if variant_repeated {
                    repeated = true;
                    break;
                }
            }
        }

        if repeated {
            let index = node_index(position);
            let fault = SchemaFault::RepeatedField;

            return Err(DecodeError::InvalidSchema { node: index, fault });
        }
    }

    Ok(())
}

/// Refuses the first enum with two variants of one name.
fn reject_repeated_variant(nodes: &[SchemaNode]) -> Result<(), DecodeError> {
    for (position, node) in nodes.iter().enumerate() {
        let repeated = if let SchemaNode::Enum { variants, .. } = node {
            has_repeated_variant_name(variants)
        } else {
            false
        };

        if repeated {
            let index = node_index(position);
            let fault = SchemaFault::RepeatedVariant;

            return Err(DecodeError::InvalidSchema { node: index, fault });
        }
    }

    Ok(())
}

/// Refuses the first enum with two variants of one selector.
fn reject_repeated_selector(nodes: &[SchemaNode]) -> Result<(), DecodeError> {
    for (position, node) in nodes.iter().enumerate() {
        let repeated = if let SchemaNode::Enum { variants, .. } = node {
            has_repeated_variant_selector(variants)
        } else {
            false
        };

        if repeated {
            let index = node_index(position);
            let fault = SchemaFault::RepeatedSelector;

            return Err(DecodeError::InvalidSchema { node: index, fault });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Budget, from_bytes, to_bytes};
    use alloc::collections::BTreeMap;
    use alloc::vec;

    #[test]
    fn shared_shape_appears_once_with_children_first() {
        // Catches an `of` walk that appends a node per occurrence instead of sharing equals.
        let shape = Shape::Struct {
            name: "Small",
            fields: &[
                Field { name: "a", shape: Shape::Integer(Integer::U32) },
                Field { name: "b", shape: Shape::Integer(Integer::U32) },
            ],
        };
        let flat = SchemaNodes::of(&shape);
        let first = SchemaNode::Integer(Integer::U32);
        let root = SchemaNode::Struct {
            name: String::from("Small"),
            fields: vec![
                FieldNode { name: String::from("a"), shape: 0 },
                FieldNode { name: String::from("b"), shape: 0 },
            ],
        };
        assert_eq!(flat.count(), 2);
        assert_eq!(flat.node(0), Some(&first));
        assert_eq!(flat.node(1), Some(&root));
        assert_eq!(flat.root(), &root);
    }

    #[test]
    fn map_and_enum_flat_form_survives_a_round_trip() {
        // Catches an `of` walk that emits what the checked decode refuses.
        let shape = <BTreeMap<String, Result<u32, String>> as Schema>::SHAPE;
        let flat = SchemaNodes::of(&shape);
        let bytes = to_bytes(&flat);
        let is_ok = bytes.is_ok();
        assert!(is_ok);
        if let Ok(bytes) = bytes {
            let mut budget = Budget::new(4096);
            let back = from_bytes::<SchemaNodes>(&bytes, &mut budget);
            assert_eq!(back, Ok(flat));
        }
    }

    #[test]
    fn empty_list_is_refused() {
        // Catches a decoder that invents a root for no nodes.
        let empty: Vec<SchemaNode> = vec![];
        let bytes = to_bytes(&empty);
        let is_ok = bytes.is_ok();
        assert!(is_ok);
        if let Ok(bytes) = bytes {
            let mut budget = Budget::new(64);
            let refused = from_bytes::<SchemaNodes>(&bytes, &mut budget);
            let expected = Err(DecodeError::InvalidSchema { node: 0, fault: SchemaFault::Empty });
            assert_eq!(refused, expected);
        }
    }

    #[test]
    fn forward_reference_is_refused() {
        // Catches a decoder that follows an index at or above its own node.
        let nodes = vec![SchemaNode::Sequence(1), SchemaNode::Boolean];
        let bytes = to_bytes(&nodes);
        let is_ok = bytes.is_ok();
        assert!(is_ok);
        if let Ok(bytes) = bytes {
            let mut budget = Budget::new(256);
            let refused = from_bytes::<SchemaNodes>(&bytes, &mut budget);
            let expected = Err(DecodeError::InvalidSchema { node: 0, fault: SchemaFault::ForwardReference });
            assert_eq!(refused, expected);
        }
    }

    #[test]
    fn non_key_map_is_refused() {
        // Catches a decoder that accepts a map key outside the key shapes.
        let nodes = vec![SchemaNode::Boolean, SchemaNode::Integer(Integer::U32), SchemaNode::Map { key: 0, value: 1 }];
        let bytes = to_bytes(&nodes);
        let is_ok = bytes.is_ok();
        assert!(is_ok);
        if let Ok(bytes) = bytes {
            let mut budget = Budget::new(256);
            let refused = from_bytes::<SchemaNodes>(&bytes, &mut budget);
            let expected = Err(DecodeError::InvalidSchema { node: 2, fault: SchemaFault::KeyShape });
            assert_eq!(refused, expected);
        }
    }

    #[test]
    fn repeated_node_is_refused() {
        // Catches a decoder that keeps two equal nodes instead of sharing one.
        let nodes = vec![SchemaNode::Boolean, SchemaNode::Boolean];
        let bytes = to_bytes(&nodes);
        let is_ok = bytes.is_ok();
        assert!(is_ok);
        if let Ok(bytes) = bytes {
            let mut budget = Budget::new(256);
            let refused = from_bytes::<SchemaNodes>(&bytes, &mut budget);
            let expected = Err(DecodeError::InvalidSchema { node: 1, fault: SchemaFault::Repeated });
            assert_eq!(refused, expected);
        }
    }

    #[test]
    fn unreferenced_node_is_refused() {
        // Catches a decoder that keeps a node no later node names.
        let nodes = vec![SchemaNode::Boolean, SchemaNode::Integer(Integer::U8), SchemaNode::Sequence(1)];
        let bytes = to_bytes(&nodes);
        let is_ok = bytes.is_ok();
        assert!(is_ok);
        if let Ok(bytes) = bytes {
            let mut budget = Budget::new(256);
            let refused = from_bytes::<SchemaNodes>(&bytes, &mut budget);
            let expected = Err(DecodeError::InvalidSchema { node: 0, fault: SchemaFault::Unreferenced });
            assert_eq!(refused, expected);
        }
    }

    #[test]
    fn repeated_struct_field_is_refused() {
        // Catches a decoder that lets two struct fields share one name.
        let nodes = vec![
            SchemaNode::Integer(Integer::U8),
            SchemaNode::Struct {
                name: String::from("Dup"),
                fields: vec![
                    FieldNode { name: String::from("a"), shape: 0 },
                    FieldNode { name: String::from("a"), shape: 0 },
                ],
            },
        ];
        let bytes = to_bytes(&nodes);
        let is_ok = bytes.is_ok();
        assert!(is_ok);
        if let Ok(bytes) = bytes {
            let mut budget = Budget::new(256);
            let refused = from_bytes::<SchemaNodes>(&bytes, &mut budget);
            let expected = Err(DecodeError::InvalidSchema { node: 1, fault: SchemaFault::RepeatedField });
            assert_eq!(refused, expected);
        }
    }

    #[test]
    fn repeated_variant_field_is_refused() {
        // Catches a decoder that lets two fields of one variant share one name.
        let nodes = vec![
            SchemaNode::Integer(Integer::U8),
            SchemaNode::Enum {
                name: String::from("E"),
                variants: vec![VariantNode {
                    name: String::from("V"),
                    selector: 0,
                    fields: FieldsNode::Named(vec![
                        FieldNode { name: String::from("a"), shape: 0 },
                        FieldNode { name: String::from("a"), shape: 0 },
                    ]),
                }],
            },
        ];
        let bytes = to_bytes(&nodes);
        let is_ok = bytes.is_ok();
        assert!(is_ok);
        if let Ok(bytes) = bytes {
            let mut budget = Budget::new(512);
            let refused = from_bytes::<SchemaNodes>(&bytes, &mut budget);
            let expected = Err(DecodeError::InvalidSchema { node: 1, fault: SchemaFault::RepeatedField });
            assert_eq!(refused, expected);
        }
    }

    #[test]
    fn repeated_variant_name_is_refused() {
        // Catches a decoder that lets two variants share one name.
        let nodes = vec![
            SchemaNode::Boolean,
            SchemaNode::Enum {
                name: String::from("E"),
                variants: vec![
                    VariantNode { name: String::from("A"), selector: 0, fields: FieldsNode::Unnamed(vec![0]) },
                    VariantNode { name: String::from("A"), selector: 1, fields: FieldsNode::Unnamed(vec![0]) },
                ],
            },
        ];
        let bytes = to_bytes(&nodes);
        let is_ok = bytes.is_ok();
        assert!(is_ok);
        if let Ok(bytes) = bytes {
            let mut budget = Budget::new(512);
            let refused = from_bytes::<SchemaNodes>(&bytes, &mut budget);
            let expected = Err(DecodeError::InvalidSchema { node: 1, fault: SchemaFault::RepeatedVariant });
            assert_eq!(refused, expected);
        }
    }

    #[test]
    fn repeated_variant_selector_is_refused() {
        // Catches a decoder that lets two variants share one selector.
        let nodes = vec![
            SchemaNode::Boolean,
            SchemaNode::Enum {
                name: String::from("E"),
                variants: vec![
                    VariantNode { name: String::from("A"), selector: 3, fields: FieldsNode::Unnamed(vec![0]) },
                    VariantNode { name: String::from("B"), selector: 3, fields: FieldsNode::Unnamed(vec![0]) },
                ],
            },
        ];
        let bytes = to_bytes(&nodes);
        let is_ok = bytes.is_ok();
        assert!(is_ok);
        if let Ok(bytes) = bytes {
            let mut budget = Budget::new(512);
            let refused = from_bytes::<SchemaNodes>(&bytes, &mut budget);
            let expected = Err(DecodeError::InvalidSchema { node: 1, fault: SchemaFault::RepeatedSelector });
            assert_eq!(refused, expected);
        }
    }

    #[test]
    fn same_field_name_in_two_variants_is_kept() {
        // Catches a check that compares field names across variants.
        let root = SchemaNode::Enum {
            name: String::from("E"),
            variants: vec![
                VariantNode {
                    name: String::from("A"),
                    selector: 0,
                    fields: FieldsNode::Named(vec![FieldNode { name: String::from("a"), shape: 0 }]),
                },
                VariantNode {
                    name: String::from("B"),
                    selector: 1,
                    fields: FieldsNode::Named(vec![FieldNode { name: String::from("a"), shape: 0 }]),
                },
            ],
        };
        let nodes = vec![SchemaNode::Boolean, root.clone()];
        let bytes = to_bytes(&nodes);
        let is_ok = bytes.is_ok();
        assert!(is_ok);
        if let Ok(bytes) = bytes {
            let mut budget = Budget::new(512);
            let back = from_bytes::<SchemaNodes>(&bytes, &mut budget);
            let expected = SchemaNodes { earlier: vec![SchemaNode::Boolean], root };
            assert_eq!(back, Ok(expected));
        }
    }
}
