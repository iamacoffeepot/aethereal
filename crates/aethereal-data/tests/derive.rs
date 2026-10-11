//! The `Plain` and `Key` derives from outside the crate (ADR-0004).

use aethereal_data::{
    Budget, DecodeError, Field, Fields, Integer, Key, Plain, Schema, Shape, Variant, from_bytes, to_bytes,
};
use std::collections::BTreeMap;
use std::vec::Vec;

#[derive(Plain, Debug, Clone, PartialEq, Eq)]
struct Tick {
    frame: u64,
    delta: u64,
}

#[derive(Plain, Debug, Clone, PartialEq, Eq)]
struct Pair(u32, u32);

#[derive(Plain, Debug, Clone, PartialEq, Eq)]
struct Unit;

#[derive(Plain, Debug, Clone, PartialEq, Eq)]
enum Message {
    Ping,
    Data(u32, u8),
    Spot { x: u16 },
}

#[derive(Plain, Debug, Clone, PartialEq, Eq)]
enum WithCustom {
    First,
    #[plain(selector = 7)]
    Second,
    Third,
}

#[derive(Plain, Debug, Clone, PartialEq, Eq)]
struct Wrapper<T> {
    inner: T,
}

#[derive(Plain, Key, Debug, Clone)]
struct Id(u64);

#[test]
fn named_struct_bytes_are_fields_in_order() {
    // Catches a struct codec that reorders fields or adds a marker.
    let tick = Tick { frame: 1, delta: 2 };
    let expected: Vec<u8> =
        std::vec![0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,];
    assert_eq!(to_bytes(&tick), Ok(expected));
}

#[test]
fn named_struct_shape_lists_named_fields() {
    // Catches a struct shape that forgets a field name.
    const FIELDS: &[Field] = &[
        Field { name: "frame", shape: Shape::Integer(Integer::U64) },
        Field { name: "delta", shape: Shape::Integer(Integer::U64) },
    ];
    let expected = Shape::Struct { name: "Tick", fields: FIELDS };
    assert_eq!(Tick::SHAPE, expected);
}

#[test]
fn tuple_struct_fields_are_named_by_position() {
    // Catches tuple fields named by Rust ident instead of `"0"`, `"1"`.
    const FIELDS: &[Field] = &[
        Field { name: "0", shape: Shape::Integer(Integer::U32) },
        Field { name: "1", shape: Shape::Integer(Integer::U32) },
    ];
    let expected = Shape::Struct { name: "Pair", fields: FIELDS };
    assert_eq!(Pair::SHAPE, expected);
}

#[test]
fn unit_struct_has_no_bytes() {
    // Catches a unit codec that writes a marker byte.
    assert_eq!(to_bytes(&Unit), Ok(Vec::new()));
    assert_eq!(from_bytes::<Unit>(&[], &mut Budget::new(16)), Ok(Unit));
}

#[test]
fn enum_variants_use_positional_selectors() {
    // Catches swapped selectors or a missing selector prefix.
    assert_eq!(to_bytes(&Message::Ping), Ok(std::vec![0x00, 0x00, 0x00, 0x00]));
    assert_eq!(to_bytes(&Message::Data(1, 2)), Ok(std::vec![0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02]));
    assert_eq!(to_bytes(&Message::Spot { x: 0x0201 }), Ok(std::vec![0x02, 0x00, 0x00, 0x00, 0x01, 0x02]));
}

#[test]
fn enum_shape_names_variants_and_fields() {
    // Catches a variant shape that drops a name or flattens struct fields.
    const SPOT_FIELDS: &[Field] = &[Field { name: "x", shape: Shape::Integer(Integer::U16) }];
    const DATA_FIELDS: &[Shape] = &[Shape::Integer(Integer::U32), Shape::Integer(Integer::U8)];
    const VARIANTS: &[Variant] = &[
        Variant { name: "Ping", selector: 0, fields: Fields::Unnamed(&[]) },
        Variant { name: "Data", selector: 1, fields: Fields::Unnamed(DATA_FIELDS) },
        Variant { name: "Spot", selector: 2, fields: Fields::Named(SPOT_FIELDS) },
    ];
    let expected = Shape::Enum { name: "Message", variants: VARIANTS };
    assert_eq!(Message::SHAPE, expected);
}

#[test]
fn custom_selector_writes_and_reads_seven() {
    // Catches a custom selector ignored on encode or on decode.
    assert_eq!(to_bytes(&WithCustom::Second), Ok(std::vec![0x07, 0x00, 0x00, 0x00]));
    assert_eq!(from_bytes::<WithCustom>(&[0x07, 0x00, 0x00, 0x00], &mut Budget::new(16)), Ok(WithCustom::Second));
}

#[test]
fn unknown_selector_names_the_enum() {
    // Catches a decoder that reads past the declared variants.
    let refused = from_bytes::<Message>(&[0x63, 0x00, 0x00, 0x00], &mut Budget::new(16));
    assert_eq!(refused, Err(DecodeError::InvalidSelector { name: "Message", selector: 99 }));
}

#[test]
fn generic_wrapper_works_at_two_types() {
    // Catches a missing `Plain` bound on the type parameter.
    assert_eq!(to_bytes(&Wrapper { inner: 1u32 }), Ok(std::vec![0x01, 0x00, 0x00, 0x00]));
    assert_eq!(to_bytes(&Wrapper { inner: String::from("hi") }), Ok(std::vec![0x02, 0x00, 0x00, 0x00, b'h', b'i']));
}

#[test]
fn key_newtype_orders_like_its_inner() {
    // Catches a `Key` Ord that does not delegate to the field.
    let mut map = BTreeMap::new();
    map.insert(Id(2), 20u8);
    map.insert(Id(1), 10u8);
    let expected: Vec<u8> = std::vec![
        0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0A, 0x02, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x14,
    ];
    assert_eq!(to_bytes(&map), Ok(expected));
}
