//! The `Plain`, `Bound`, and `Key` derives from outside the crate (ADR-0004), and a kind named on its derive
//! (ADR-0006).

use aethereal_data::{
    Bound, Boundary, Budget, Decode, DecodeError, Encode, EncodeError, Field, Fields, Integer, Key, Kind, KindId,
    KindName, Plain, Reader, Schema, Shape, Variant, Writer, from_bytes, to_bytes,
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

#[derive(Plain)]
#[kind(name = "test.derive.note_on")]
struct NoteOn {
    pitch: u8,
    velocity: u8,
}

#[derive(Plain)]
struct NoteOnByHand {
    pitch: u8,
    velocity: u8,
}

impl Kind for NoteOnByHand {
    const NAME: KindName = KindName::new("test.derive.note_on");
}

#[derive(Plain)]
#[kind(name = "test.derive.signal")]
enum Signal {
    Stop,
    #[plain(selector = 4)]
    Go(u8),
}

#[derive(Plain)]
enum SignalByHand {
    Stop,
    #[plain(selector = 4)]
    Go(u8),
}

impl Kind for SignalByHand {
    const NAME: KindName = KindName::new("test.derive.signal");
}

#[derive(Plain)]
#[kind(name = "test.derive.carried")]
struct Carried<T> {
    inner: T,
}

#[derive(Plain)]
struct CarriedByHand<T> {
    inner: T,
}

impl<T: Schema> Kind for CarriedByHand<T> {
    const NAME: KindName = KindName::new("test.derive.carried");
}

/// A value with a schema and a codec that is not `Plain`, as a value bound to its actor is.
struct Held(u32);

impl Schema for Held {
    const SHAPE: Shape = Shape::Integer(Integer::U32);
}

impl<B: Boundary> Encode<B> for Held {
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        self.0.encode(out)
    }
}

impl<B: Boundary> Decode<B> for Held {
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        u32::decode(input).map(Self)
    }
}

#[derive(Bound)]
#[kind(name = "test.derive.holding")]
struct Holding<T> {
    held: T,
    count: u8,
}

#[derive(Bound)]
struct HoldingByHand<T> {
    held: T,
    count: u8,
}

impl<T: Schema> Kind for HoldingByHand<T> {
    const NAME: KindName = KindName::new("test.derive.holding");
}

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

#[test]
fn named_struct_is_the_kind_written_by_hand() {
    // Catches an attribute that mints a name or an id other than the one written out by hand.
    assert_eq!(NoteOn::NAME, NoteOnByHand::NAME);
    assert_eq!(KindId::of::<NoteOn>(), KindId::of::<NoteOnByHand>());
}

#[test]
fn named_enum_is_the_kind_written_by_hand() {
    // Catches the enum expansion naming its kind apart from the one written by hand.
    assert_eq!(Signal::NAME, SignalByHand::NAME);
    assert_eq!(KindId::of::<Signal>(), KindId::of::<SignalByHand>());
}

#[test]
fn named_generic_is_a_kind_at_each_type() {
    // Catches a generic kind bounded more tightly than its schema, or an impl that drops the type's parameters.
    assert_eq!(KindId::of::<Carried<u32>>(), KindId::of::<CarriedByHand<u32>>());
    assert_eq!(KindId::of::<Carried<String>>(), KindId::of::<CarriedByHand<String>>());
}

#[test]
fn named_bound_type_is_a_kind_over_a_value_that_is_not_plain() {
    // Catches the bound derive's kind requiring `Plain` of its fields, which a bound value is not.
    assert_eq!(Holding::<Held>::NAME, HoldingByHand::<Held>::NAME);
    assert_eq!(KindId::of::<Holding<Held>>(), KindId::of::<HoldingByHand<Held>>());
}
