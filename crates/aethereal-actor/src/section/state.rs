//! The schema of a type's state as a section holds it: the plain bytes of its [`SchemaNodes`], written in a constant
//! from the type's shape (ADR-0003, ADR-0054).
//!
//! The nodes go as [`SchemaNodes::of`] makes them: each shape's children before it, in the order they appear, equal
//! shapes sharing the node of the first, and the state's own shape last. A constant cannot allocate, so the shapes
//! given nodes so far are kept in an array of the room the section's writer sizes for its largest state.
//!
//! [`SchemaNodes`]: aethereal_data::SchemaNodes
//! [`SchemaNodes::of`]: aethereal_data::SchemaNodes::of

use super::{count, put, text};
use aethereal_data::{Field, Fields, Float, Integer, Shape, Variant};

// The selector of each case of `SchemaNode`, in the order it declares them (ADR-0003).
const BOOLEAN: usize = 0;
const INTEGER: usize = 1;
const FLOAT: usize = 2;
const STRING: usize = 3;
const BYTES: usize = 4;
const OPTION: usize = 5;
const SEQUENCE: usize = 6;
const ARRAY: usize = 7;
const MAP: usize = 8;
const STRUCT: usize = 9;
const TUPLE: usize = 10;
const ENUM: usize = 11;
const JSON: usize = 12;

// The selector of each case of `FieldsNode`.
const NAMED: usize = 0;
const UNNAMED: usize = 1;

/// How many shapes the tree of `shape` holds, itself included: the most nodes its flat form can have, and so the room
/// [`write`] needs for it.
pub(super) const fn shapes(shape: &Shape) -> usize {
    let within = match *shape {
        Shape::Boolean | Shape::Integer(_) | Shape::Float(_) | Shape::String | Shape::Bytes | Shape::Json => 0,
        Shape::Option(inner) | Shape::Sequence(inner) | Shape::Array { element: inner, .. } => shapes(inner),
        Shape::Map { key, value } => shapes(key) + shapes(value),
        Shape::Struct { fields, .. } => field_shapes(fields),
        Shape::Tuple { elements } => element_shapes(elements),
        Shape::Enum { variants, .. } => {
            let mut within = 0;
            let mut index = 0;
            while index < variants.len() {
                within += match variants[index].fields {
                    Fields::Named(fields) => field_shapes(fields),
                    Fields::Unnamed(elements) => element_shapes(elements),
                };
                index += 1;
            }

            within
        }
    };

    1 + within
}

const fn field_shapes(fields: &[Field]) -> usize {
    let mut within = 0;
    let mut index = 0;
    while index < fields.len() {
        within += shapes(&fields[index].shape);
        index += 1;
    }

    within
}

const fn element_shapes(elements: &[Shape]) -> usize {
    let mut within = 0;
    let mut index = 0;
    while index < elements.len() {
        within += shapes(&elements[index]);
        index += 1;
    }

    within
}

/// Writes the flat schema of `shape` at `at`, as far as `out` has room, and answers with where what follows it goes:
/// its count of nodes, then each node.
///
/// # Panics
///
/// Panics, stopping the build, when `ROOM` is less than [`shapes`] of `shape`.
pub(super) const fn write<const ROOM: usize>(out: &mut [u8], at: usize, shape: &'static Shape) -> usize {
    let mut nodes = Nodes::<ROOM> { shapes: [&Shape::Boolean; ROOM], len: 0 };
    let first = count(out, at, 0);
    let after = children(out, first, shape, &mut nodes);
    let after = node(out, after, shape, &nodes);
    count(out, at, nodes.len + 1);

    after
}

/// The shapes given a node so far, each at its node's index.
struct Nodes<const ROOM: usize> {
    shapes: [&'static Shape; ROOM],
    len: usize,
}

impl<const ROOM: usize> Nodes<ROOM> {
    /// The index of the node of `shape`, when one was given it.
    const fn find(&self, shape: &Shape) -> Option<usize> {
        let mut index = 0;
        while index < self.len {
            if same(self.shapes[index], shape) {
                return Some(index);
            }
            index += 1;
        }

        None
    }

    /// Whether `shape`, or one equal to it, was given a node.
    const fn holds(&self, shape: &Shape) -> bool {
        self.find(shape).is_some()
    }

    /// The index of the node of `shape`, which its parent's children were given before it.
    ///
    /// # Panics
    ///
    /// Panics when it has none, which [`children`] gives every child of a shape before the shape is written.
    const fn index(&self, shape: &Shape) -> usize {
        match self.find(shape) {
            Some(index) => index,
            None => panic!("a shape's children are given nodes before it"),
        }
    }
}

/// Gives `shape` a node, writing it at `at` after its children's, unless one equal to it has one, and answers with
/// where what follows goes.
const fn intern<const ROOM: usize>(out: &mut [u8], at: usize, shape: &'static Shape, nodes: &mut Nodes<ROOM>) -> usize {
    if nodes.holds(shape) {
        return at;
    }
    let at = children(out, at, shape, nodes);
    let at = node(out, at, shape, nodes);
    nodes.shapes[nodes.len] = shape;
    nodes.len += 1;

    at
}

/// Gives each child of `shape` a node, in the order they appear, writing them at `at`, and answers with where what
/// follows goes.
const fn children<const ROOM: usize>(
    out: &mut [u8],
    at: usize,
    shape: &'static Shape,
    nodes: &mut Nodes<ROOM>,
) -> usize {
    match *shape {
        Shape::Boolean | Shape::Integer(_) | Shape::Float(_) | Shape::String | Shape::Bytes | Shape::Json => at,
        Shape::Option(inner) | Shape::Sequence(inner) | Shape::Array { element: inner, .. } => {
            intern(out, at, inner, nodes)
        }
        Shape::Map { key, value } => {
            let at = intern(out, at, key, nodes);

            intern(out, at, value, nodes)
        }
        Shape::Struct { fields, .. } => {
            let mut at = at;
            let mut index = 0;
            while index < fields.len() {
                at = intern(out, at, &fields[index].shape, nodes);
                index += 1;
            }

            at
        }
        Shape::Tuple { elements } => {
            let mut at = at;
            let mut index = 0;
            while index < elements.len() {
                at = intern(out, at, &elements[index], nodes);
                index += 1;
            }

            at
        }
        Shape::Enum { variants, .. } => {
            let mut at = at;
            let mut index = 0;
            while index < variants.len() {
                match variants[index].fields {
                    Fields::Named(fields) => {
                        let mut field = 0;
                        while field < fields.len() {
                            at = intern(out, at, &fields[field].shape, nodes);
                            field += 1;
                        }
                    }
                    Fields::Unnamed(elements) => {
                        let mut element = 0;
                        while element < elements.len() {
                            at = intern(out, at, &elements[element], nodes);
                            element += 1;
                        }
                    }
                }
                index += 1;
            }

            at
        }
    }
}

/// Writes the node of `shape` at `at`, naming each child by the index of its node, and answers with where what follows
/// goes.
const fn node<const ROOM: usize>(out: &mut [u8], at: usize, shape: &Shape, nodes: &Nodes<ROOM>) -> usize {
    match *shape {
        Shape::Boolean => count(out, at, BOOLEAN),
        Shape::Integer(integer) => {
            let at = count(out, at, INTEGER);

            count(out, at, integer_selector(integer))
        }
        Shape::Float(float) => {
            let at = count(out, at, FLOAT);

            count(out, at, float_selector(float))
        }
        Shape::String => count(out, at, STRING),
        Shape::Bytes => count(out, at, BYTES),
        Shape::Json => count(out, at, JSON),
        Shape::Option(inner) => {
            let at = count(out, at, OPTION);

            count(out, at, nodes.index(inner))
        }
        Shape::Sequence(inner) => {
            let at = count(out, at, SEQUENCE);

            count(out, at, nodes.index(inner))
        }
        Shape::Array { element, length } => {
            let at = count(out, at, ARRAY);
            let at = count(out, at, nodes.index(element));

            number(out, at, length)
        }
        Shape::Map { key, value } => {
            let at = count(out, at, MAP);
            let at = count(out, at, nodes.index(key));

            count(out, at, nodes.index(value))
        }
        Shape::Struct { name, fields } => {
            let at = count(out, at, STRUCT);
            let at = text(out, at, name);

            named(out, at, fields, nodes)
        }
        Shape::Tuple { elements } => {
            let at = count(out, at, TUPLE);

            unnamed(out, at, elements, nodes)
        }
        Shape::Enum { name, variants } => {
            let at = count(out, at, ENUM);
            let mut at = text(out, at, name);
            at = count(out, at, variants.len());
            let mut index = 0;
            while index < variants.len() {
                let variant = &variants[index];
                at = text(out, at, variant.name);
                at = number(out, at, variant.selector);
                at = match variant.fields {
                    Fields::Named(fields) => {
                        let at = count(out, at, NAMED);

                        named(out, at, fields, nodes)
                    }
                    Fields::Unnamed(elements) => {
                        let at = count(out, at, UNNAMED);

                        unnamed(out, at, elements, nodes)
                    }
                };
                index += 1;
            }

            at
        }
    }
}

/// Writes named fields: their count, then each one's name and the index of its shape's node.
const fn named<const ROOM: usize>(out: &mut [u8], at: usize, fields: &[Field], nodes: &Nodes<ROOM>) -> usize {
    let mut at = count(out, at, fields.len());
    let mut index = 0;
    while index < fields.len() {
        at = text(out, at, fields[index].name);
        at = count(out, at, nodes.index(&fields[index].shape));
        index += 1;
    }

    at
}

/// Writes positional fields: their count, then the index of each one's node.
const fn unnamed<const ROOM: usize>(out: &mut [u8], at: usize, elements: &[Shape], nodes: &Nodes<ROOM>) -> usize {
    let mut at = count(out, at, elements.len());
    let mut index = 0;
    while index < elements.len() {
        at = count(out, at, nodes.index(&elements[index]));
        index += 1;
    }

    at
}

/// A number the schema holds: an unsigned 32-bit number, little-endian (ADR-0002).
const fn number(out: &mut [u8], at: usize, number: u32) -> usize {
    put(out, at, &number.to_le_bytes())
}

/// The selector of `integer`, in the order [`Integer`] declares its widths.
const fn integer_selector(integer: Integer) -> usize {
    match integer {
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

/// The selector of `float`, in the order [`Float`] declares its widths.
const fn float_selector(float: Float) -> usize {
    match float {
        Float::F32 => 0,
        Float::F64 => 1,
    }
}

/// Whether `one` and `other` are one shape, names included: what gives them one node, as [`SchemaNodes::of`] gives a
/// node to each shape unequal to every shape before it.
///
/// [`SchemaNodes::of`]: aethereal_data::SchemaNodes::of
const fn same(one: &Shape, other: &Shape) -> bool {
    match (*one, *other) {
        (Shape::Boolean, Shape::Boolean)
        | (Shape::String, Shape::String)
        | (Shape::Bytes, Shape::Bytes)
        | (Shape::Json, Shape::Json) => true,
        (Shape::Integer(one), Shape::Integer(other)) => integer_selector(one) == integer_selector(other),
        (Shape::Float(one), Shape::Float(other)) => float_selector(one) == float_selector(other),
        (Shape::Option(one), Shape::Option(other)) | (Shape::Sequence(one), Shape::Sequence(other)) => same(one, other),
        (Shape::Array { element, length }, Shape::Array { element: other_element, length: other_length }) => {
            length == other_length && same(element, other_element)
        }
        (Shape::Map { key, value }, Shape::Map { key: other_key, value: other_value }) => {
            same(key, other_key) && same(value, other_value)
        }
        (Shape::Struct { name, fields }, Shape::Struct { name: other_name, fields: other_fields }) => {
            same_text(name, other_name) && same_fields(fields, other_fields)
        }
        (Shape::Tuple { elements }, Shape::Tuple { elements: other_elements }) => same_shapes(elements, other_elements),
        (Shape::Enum { name, variants }, Shape::Enum { name: other_name, variants: other_variants }) => {
            same_text(name, other_name) && same_variants(variants, other_variants)
        }
        _ => false,
    }
}

const fn same_fields(one: &[Field], other: &[Field]) -> bool {
    if one.len() != other.len() {
        return false;
    }
    let mut index = 0;
    while index < one.len() {
        let named_alike = same_text(one[index].name, other[index].name);
        let shaped_alike = same(&one[index].shape, &other[index].shape);
        let alike = named_alike && shaped_alike;
        if !alike {
            return false;
        }
        index += 1;
    }

    true
}

const fn same_shapes(one: &[Shape], other: &[Shape]) -> bool {
    if one.len() != other.len() {
        return false;
    }
    let mut index = 0;
    while index < one.len() {
        if !same(&one[index], &other[index]) {
            return false;
        }
        index += 1;
    }

    true
}

const fn same_variants(one: &[Variant], other: &[Variant]) -> bool {
    if one.len() != other.len() {
        return false;
    }
    let mut index = 0;
    while index < one.len() {
        let (variant, other_variant) = (&one[index], &other[index]);
        let named_alike = same_text(variant.name, other_variant.name);
        let selected_alike = variant.selector == other_variant.selector;
        let fields_alike = match (variant.fields, other_variant.fields) {
            (Fields::Named(fields), Fields::Named(other_fields)) => same_fields(fields, other_fields),
            (Fields::Unnamed(elements), Fields::Unnamed(other_elements)) => same_shapes(elements, other_elements),
            _ => false,
        };
        let alike = named_alike && selected_alike && fields_alike;
        if !alike {
            return false;
        }
        index += 1;
    }

    true
}

const fn same_text(one: &str, other: &str) -> bool {
    let (one, other) = (one.as_bytes(), other.as_bytes());
    if one.len() != other.len() {
        return false;
    }
    let mut index = 0;
    while index < one.len() {
        if one[index] != other[index] {
            return false;
        }
        index += 1;
    }

    true
}
