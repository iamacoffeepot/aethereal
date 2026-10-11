//! The schema vocabulary: the shapes a value's bytes can take (ADR-0003).

/// An integer width and signedness in the schema vocabulary (ADR-0003).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Integer {
    /// An unsigned 8-bit integer.
    U8,
    /// An unsigned 16-bit integer.
    U16,
    /// An unsigned 32-bit integer.
    U32,
    /// An unsigned 64-bit integer.
    U64,
    /// A signed 8-bit integer.
    I8,
    /// A signed 16-bit integer.
    I16,
    /// A signed 32-bit integer.
    I32,
    /// A signed 64-bit integer.
    I64,
}

/// A float width in the schema vocabulary (ADR-0003).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Float {
    /// A 32-bit float.
    F32,
    /// A 64-bit float.
    F64,
}

/// A shape in the schema vocabulary (ADR-0003).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// One byte, `0` or `1` (ADR-0002).
    Boolean,
    /// Its declared width, little-endian, two's complement (ADR-0002).
    Integer(Integer),
    /// Its IEEE 754 bits, little-endian (ADR-0002).
    Float(Float),
    /// A count, then that many bytes of UTF-8 (ADR-0002).
    String,
    /// A count, then that many bytes (ADR-0002).
    Bytes,
    /// One byte, `0` or `1`; then the value if `1` (ADR-0002).
    Option(&'static Self),
    /// A count, then that many elements (ADR-0002).
    Sequence(&'static Self),
    /// The elements, with no count (ADR-0002).
    Array {
        /// The shape of each element.
        element: &'static Self,
        /// The number of elements.
        length: u32,
    },
    /// A count, then that many key and value pairs (ADR-0002).
    Map {
        /// The shape of each key; one of the key shapes ADR-0002 lists.
        key: &'static Self,
        /// The shape of each value.
        value: &'static Self,
    },
    /// Each field in declared order (ADR-0002).
    Struct {
        /// The struct's name; for readers that need it, never in the bytes (ADR-0003).
        name: &'static str,
        /// Each field in declared order.
        fields: &'static [Field],
    },
    /// Each element in order (ADR-0002).
    Tuple {
        /// Each element in order.
        elements: &'static [Self],
    },
    /// A selector, then the selected variant's fields in order (ADR-0002).
    Enum {
        /// The enum's name; for readers that need it, never in the bytes (ADR-0003).
        name: &'static str,
        /// Each variant with its declared selector.
        variants: &'static [Variant],
    },
}

/// One field of a struct shape (ADR-0003).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    /// The field's name; for readers that need it, never in the bytes (ADR-0003).
    pub name: &'static str,
    /// The field's shape.
    pub shape: Shape,
}

/// One variant of an enum shape (ADR-0003).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Variant {
    /// The variant's name; for readers that need it, never in the bytes (ADR-0003).
    pub name: &'static str,
    /// The variant's declared number on the wire, not its position (ADR-0002).
    pub selector: u32,
    /// The variant's fields in order.
    pub fields: Fields,
}

/// The fields of one enum variant (ADR-0003).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fields {
    /// Named fields, in declared order.
    Named(&'static [Field]),
    /// Positional fields, in order; a variant with no fields is `Unnamed(&[])`.
    Unnamed(&'static [Shape]),
}

/// A type that states its schema as a constant (ADR-0003).
pub trait Schema {
    /// The type's shape in the schema vocabulary (ADR-0003).
    const SHAPE: Shape;
    /// The shape of a sequence of this type (ADR-0004: `Vec<u8>` is `Bytes`).
    #[doc(hidden)]
    const SEQUENCE: Shape = Shape::Sequence(&Self::SHAPE);
}
