//! The layout hash over both forms of a schema (ADR-0006).

use crate::{Field, FieldNode, Fields, FieldsNode, Float, Fnv, Integer, SchemaNode, SchemaNodes, Shape, Variant};
use alloc::vec::Vec;

// The tag bytes of ADR-0006's table: the first byte each shape hashes.
const BOOLEAN: u8 = 0;
const INTEGER: u8 = 1;
const FLOAT: u8 = 2;
const STRING: u8 = 3;
const BYTES: u8 = 4;
const OPTION: u8 = 5;
const SEQUENCE: u8 = 6;
const ARRAY: u8 = 7;
const MAP: u8 = 8;
const STRUCT: u8 = 9;
const TUPLE: u8 = 10;
const ENUM: u8 = 11;

/// The tag byte of an integer width: `0` to `7` for `U8`, `U16`, `U32`, `U64`, `I8`, `I16`, `I32`, `I64`.
pub const fn integer_code(kind: Integer) -> u8 {
    match kind {
        Integer::U8 => 0,
        Integer::U16 => 1,
        Integer::U32 => 2,
        Integer::U64 => 3,
        Integer::I8 => 4,
        Integer::I16 => 5,
        Integer::I32 => 6,
        Integer::I64 => 7,
    }
}

/// The tag byte of a float width: `0` for `F32`, `1` for `F64`.
pub const fn float_code(kind: Float) -> u8 {
    match kind {
        Float::F32 => 0,
        Float::F64 => 1,
    }
}

/// The layout hash of a constant shape. A `Shape` is a finite tree fixed by a type, so the recursion is bounded by the type.
pub(super) const fn shape_layout(shape: &Shape) -> u64 {
    match *shape {
        Shape::Boolean => Fnv::new().byte(BOOLEAN).finish(),
        Shape::Integer(kind) => Fnv::new().byte(INTEGER).byte(integer_code(kind)).finish(),
        Shape::Float(kind) => Fnv::new().byte(FLOAT).byte(float_code(kind)).finish(),
        Shape::String => Fnv::new().byte(STRING).finish(),
        Shape::Bytes => Fnv::new().byte(BYTES).finish(),
        Shape::Option(inner) => Fnv::new().byte(OPTION).hash(shape_layout(inner)).finish(),
        Shape::Sequence(inner) => Fnv::new().byte(SEQUENCE).hash(shape_layout(inner)).finish(),
        Shape::Array { element, length } => Fnv::new().byte(ARRAY).hash(shape_layout(element)).number(length).finish(),
        Shape::Map { key, value } => Fnv::new().byte(MAP).hash(shape_layout(key)).hash(shape_layout(value)).finish(),
        Shape::Struct { fields, .. } => fold_fields(Fnv::new().byte(STRUCT), fields).finish(),
        Shape::Tuple { elements } => fold_shapes(Fnv::new().byte(TUPLE), elements).finish(),
        Shape::Enum { variants, .. } => fold_variants(Fnv::new().byte(ENUM), variants).finish(),
    }
}

/// The count, then each field's layout hash in declared order, folded onto `start`.
const fn fold_fields(start: Fnv, fields: &[Field]) -> Fnv {
    let mut hash = start.count(fields.len());
    let mut index = 0;
    while index < fields.len() {
        hash = hash.hash(shape_layout(&fields[index].shape));
        index += 1;
    }

    hash
}

/// The count, then each element's layout hash in order, folded onto `start`.
const fn fold_shapes(start: Fnv, shapes: &[Shape]) -> Fnv {
    let mut hash = start.count(shapes.len());
    let mut index = 0;
    while index < shapes.len() {
        hash = hash.hash(shape_layout(&shapes[index]));
        index += 1;
    }

    hash
}

/// The count, then per variant its selector and its fields, folded onto `start`.
const fn fold_variants(start: Fnv, variants: &[Variant]) -> Fnv {
    let mut hash = start.count(variants.len());
    let mut index = 0;
    while index < variants.len() {
        let selected = hash.number(variants[index].selector);
        hash = match variants[index].fields {
            Fields::Named(fields) => fold_fields(selected, fields),
            Fields::Unnamed(shapes) => fold_shapes(selected, shapes),
        };
        index += 1;
    }

    hash
}

/// The layout hash of a flat schema: one step per node, in stored order.
pub(super) fn nodes_layout(schema: &SchemaNodes) -> u64 {
    let mut hashes = Vec::new();
    for node in schema.earlier() {
        let hash = node_layout(node, &hashes);
        hashes.push(hash);
    }

    node_layout(schema.root(), &hashes)
}

/// The layout hash of one flat node, given the hash of every node before it.
fn node_layout(node: &SchemaNode, earlier: &[u64]) -> u64 {
    match node {
        SchemaNode::Boolean => Fnv::new().byte(BOOLEAN).finish(),
        SchemaNode::Integer(kind) => Fnv::new().byte(INTEGER).byte(integer_code(*kind)).finish(),
        SchemaNode::Float(kind) => Fnv::new().byte(FLOAT).byte(float_code(*kind)).finish(),
        SchemaNode::String => Fnv::new().byte(STRING).finish(),
        SchemaNode::Bytes => Fnv::new().byte(BYTES).finish(),
        SchemaNode::Option(child) => Fnv::new().byte(OPTION).hash(hash_at(earlier, *child)).finish(),
        SchemaNode::Sequence(child) => Fnv::new().byte(SEQUENCE).hash(hash_at(earlier, *child)).finish(),
        SchemaNode::Array { element, length } => {
            Fnv::new().byte(ARRAY).hash(hash_at(earlier, *element)).number(*length).finish()
        }
        SchemaNode::Map { key, value } => {
            Fnv::new().byte(MAP).hash(hash_at(earlier, *key)).hash(hash_at(earlier, *value)).finish()
        }
        SchemaNode::Struct { fields, .. } => fold_field_nodes(Fnv::new().byte(STRUCT), fields, earlier).finish(),
        SchemaNode::Tuple { elements } => fold_indices(Fnv::new().byte(TUPLE), elements, earlier).finish(),
        SchemaNode::Enum { variants, .. } => {
            let start = Fnv::new().byte(ENUM).count(variants.len());
            let folded = variants.iter().fold(start, |hash, variant| {
                let selected = hash.number(variant.selector);
                fold_variant_fields(selected, &variant.fields, earlier)
            });

            folded.finish()
        }
    }
}

/// The count, then each flat field's layout hash in order, folded onto `start`.
fn fold_field_nodes(start: Fnv, fields: &[FieldNode], earlier: &[u64]) -> Fnv {
    fields.iter().fold(start.count(fields.len()), |hash, field| hash.hash(hash_at(earlier, field.shape)))
}

/// The count, then each named node's layout hash in order, folded onto `start`.
fn fold_indices(start: Fnv, indices: &[u32], earlier: &[u64]) -> Fnv {
    indices.iter().fold(start.count(indices.len()), |hash, index| hash.hash(hash_at(earlier, *index)))
}

/// One flat variant's fields folded onto `start`, named or not.
fn fold_variant_fields(start: Fnv, fields: &FieldsNode, earlier: &[u64]) -> Fnv {
    match fields {
        FieldsNode::Named(named) => fold_field_nodes(start, named, earlier),
        FieldsNode::Unnamed(indices) => fold_indices(start, indices, earlier),
    }
}

/// The hash of the node at `index`.
///
/// A `SchemaNodes` names only earlier nodes, so the fallback below cannot be
/// reached but keeps the lookup total without panicking, as `node_index` does.
fn hash_at(earlier: &[u64], index: u32) -> u64 {
    earlier.get(index as usize).copied().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::super::samples::{Command, Ledger, NoteOn, Path, doubling};
    use super::*;
    use crate::Schema;
    use alloc::collections::BTreeMap;
    use alloc::vec::Vec;

    #[test]
    fn pinned_layout_hashes_match() {
        // Catches a changed tag, code, or fold order.
        // Tripwire: layout hashes computed from ADR-0006's text by a separate program.
        assert_eq!(shape_layout(&NoteOn::SHAPE), 0xcfad_3ea2_0cdb_75b6);
        assert_eq!(shape_layout(&Path::SHAPE), 0x952c_c7a2_c575_79ed);
        assert_eq!(shape_layout(&Command::SHAPE), 0x6244_b875_ee15_b97e);
        assert_eq!(shape_layout(&Ledger::SHAPE), 0x79e1_fedc_d5e5_d3e5);
    }

    #[test]
    fn flat_hashes_equal_tree_hashes() {
        // Catches a flat arm that folds what its tree arm does not.
        assert_eq!(nodes_layout(&SchemaNodes::of(&NoteOn::SHAPE)), shape_layout(&NoteOn::SHAPE));
        assert_eq!(nodes_layout(&SchemaNodes::of(&Path::SHAPE)), shape_layout(&Path::SHAPE));
        assert_eq!(nodes_layout(&SchemaNodes::of(&Command::SHAPE)), shape_layout(&Command::SHAPE));
        assert_eq!(nodes_layout(&SchemaNodes::of(&Ledger::SHAPE)), shape_layout(&Ledger::SHAPE));
    }

    #[test]
    fn struct_and_tuple_share_no_layout() {
        // Catches a hash that merges the struct and tuple tags.
        assert_ne!(shape_layout(&<(u8, u8)>::SHAPE), shape_layout(&NoteOn::SHAPE));
    }

    #[test]
    fn lengths_orders_and_widths_change_the_layout() {
        // Catches a hash that drops a length, an order, a signedness, or a constructor.
        assert_ne!(shape_layout(&<[u8; 3]>::SHAPE), shape_layout(&<[u8; 4]>::SHAPE));
        assert_ne!(shape_layout(&<BTreeMap<u8, u16>>::SHAPE), shape_layout(&<BTreeMap<u16, u8>>::SHAPE));
        assert_ne!(shape_layout(&<(u8, u16)>::SHAPE), shape_layout(&<(u16, u8)>::SHAPE));
        assert_ne!(shape_layout(&<u32>::SHAPE), shape_layout(&<i32>::SHAPE));
        assert_ne!(shape_layout(&<Option<u8>>::SHAPE), shape_layout(&<Vec<u16>>::SHAPE));
    }

    #[test]
    fn shared_flat_schema_hashes_in_proportion() {
        // Catches a walk that unfolds sharing: 64 levels unfold to 2^64 leaves and never return.
        // Tripwire: the flat hash of 64 shared levels over a boolean, computed separately.
        assert_eq!(nodes_layout(&doubling(64, "level", SchemaNode::Boolean)), 0xfb4d_4758_de94_1bc6);
    }
}
