//! Tests for the tree-to-bytes walk.

use super::from_json;
use crate::json::rows::rows;
use crate::{Budget, DecodeError, JsonFault, JsonForm, Plain, Schema, SchemaNodes, TextFault};
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Plain)]
struct Tick {
    frame: u64,
    delta_millis: u32,
}

#[derive(Plain)]
struct Player {
    id: u64,
    name: Option<String>,
}

#[derive(Plain)]
struct World {
    players: Vec<Player>,
}

#[derive(Plain)]
struct Wrap(Option<u8>);

#[derive(Plain)]
struct Holder {
    inner: Wrap,
}

#[derive(Plain)]
enum Shape {
    Dot,
    Circle(f32),
    Box {
        w: f32,
        h: f32,
    },
    Pair(u8, u8),
    #[plain(selector = 9)]
    Far,
}

#[test]
fn every_row_reads_its_bytes() {
    // Catches a shape whose reader drifts from the table.
    for row in rows() {
        let mut budget = Budget::new(1 << 20);
        let bytes = from_json(&row.schema, &row.json, &mut budget);
        assert_eq!(bytes, Ok(row.bytes.clone()), "row {}", row.json);
    }
}

#[test]
fn narrow_integers_refuse_fractions_and_overflow() {
    // Catches an integer read that goes through a float or wraps.
    let schema = SchemaNodes::of(&<u32 as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    let fraction = from_json(&schema, "1.5", &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert_eq!(fraction, Err(JsonFault::NotAnInteger));
    let mut budget = Budget::new(1024);
    let exponent = from_json(&schema, "1e2", &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert_eq!(exponent, Err(JsonFault::NotAnInteger));
    let mut budget = Budget::new(1024);
    let large = from_json(&schema, "4294967296", &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert_eq!(large, Err(JsonFault::OutOfRange));
    let mut budget = Budget::new(1024);
    let quoted = from_json(&schema, "\"1\"", &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert_eq!(quoted, Err(JsonFault::Expected(JsonForm::Number)));
}

#[test]
fn wide_integers_refuse_numbers_and_bad_digits() {
    // Catches a 64-bit read that accepts a number or a leading plus.
    let schema = SchemaNodes::of(&<u64 as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    let number = from_json(&schema, "1", &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert_eq!(number, Err(JsonFault::Expected(JsonForm::Digits)));
    let mut budget = Budget::new(1024);
    let plus = from_json(&schema, "\"+1\"", &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert_eq!(plus, Err(JsonFault::NotDecimal));
    let mut budget = Budget::new(1024);
    let leading = from_json(&schema, "\"01\"", &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert_eq!(leading, Err(JsonFault::NotDecimal));
    let mut budget = Budget::new(1024);
    let large = from_json(&schema, "\"18446744073709551616\"", &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert_eq!(large, Err(JsonFault::OutOfRange));
    let mut budget = Budget::new(1024);
    let negative = from_json(&schema, "\"-1\"", &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert_eq!(negative, Err(JsonFault::OutOfRange));
}

#[test]
fn floats_refuse_other_names_and_values() {
    // Catches a float read that leans on `parse`, which accepts `nan`.
    let schema = SchemaNodes::of(&<f64 as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    let lower = from_json(&schema, "\"nan\"", &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert_eq!(lower, Err(JsonFault::Expected(JsonForm::Float)));
    let mut budget = Budget::new(1024);
    let boolean = from_json(&schema, "true", &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert_eq!(boolean, Err(JsonFault::Expected(JsonForm::Float)));
}

#[test]
fn bytes_refuse_odd_and_uppercase() {
    // Catches a hexadecimal read that drops a nibble or accepts uppercase.
    let schema = SchemaNodes::of(&<Vec<u8> as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    let odd = from_json(&schema, "\"abc\"", &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert_eq!(odd, Err(JsonFault::OddHexLength));
    let mut budget = Budget::new(1024);
    let upper = from_json(&schema, "\"AB\"", &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert_eq!(upper, Err(JsonFault::HexDigit));
}

#[test]
fn fixed_arrays_refuse_wrong_lengths() {
    // Catches an array read that trusts the JSON length.
    let schema = SchemaNodes::of(&<[u16; 2] as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    let one = from_json(&schema, "[1]", &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert_eq!(one, Err(JsonFault::WrongLength { expected: 2, found: 1 }));
    let mut budget = Budget::new(1024);
    let three = from_json(&schema, "[1,2,3]", &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert_eq!(three, Err(JsonFault::WrongLength { expected: 2, found: 3 }));
}

#[test]
fn byte_arrays_have_one_form() {
    // Catches a byte array read that accepts an array of numbers.
    let schema = SchemaNodes::of(&<[u8; 3] as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    let short = from_json(&schema, "\"0102\"", &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert_eq!(short, Err(JsonFault::WrongLength { expected: 3, found: 2 }));
    let mut budget = Budget::new(1024);
    let array = from_json(&schema, "[1,2,3]", &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert_eq!(array, Err(JsonFault::Expected(JsonForm::Hex)));
}

#[test]
fn tuples_refuse_wrong_lengths() {
    // Catches a tuple read that trusts the JSON length.
    let schema = SchemaNodes::of(&<(u8, bool) as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    let short = from_json(&schema, "[1]", &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert_eq!(short, Err(JsonFault::WrongLength { expected: 2, found: 1 }));
}

#[test]
fn tick_unknown_and_missing_fields_name_paths() {
    // Catches a walk that loses which member was unknown or missing.
    let schema = SchemaNodes::of(&<Tick as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    let unknown = from_json(&schema, "{\"frame\":\"1\",\"delta_millis\":16,\"extra\":1}", &mut budget);
    let Err(error) = unknown else {
        panic!("extra member is unknown");
    };
    assert_eq!(error.fault(), JsonFault::UnknownField);
    assert_eq!(error.path().to_string(), "extra");
    let mut budget = Budget::new(1024);
    let missing = from_json(&schema, "{\"delta_millis\":16}", &mut budget);
    let Err(error) = missing else {
        panic!("frame is missing");
    };
    assert_eq!(error.fault(), JsonFault::MissingField);
    assert_eq!(error.path().to_string(), "frame");
    let mut budget = Budget::new(1024);
    let repeated = from_json(&schema, "{\"frame\":\"1\",\"frame\":\"2\",\"delta_millis\":16}", &mut budget).map(|_| ());
    let fault = repeated.map_err(|error| error.fault());
    assert!(matches!(fault, Err(JsonFault::Text { fault: TextFault::RepeatedName, .. })));
}

#[test]
fn repeated_map_keys_are_refused_as_names() {
    // Catches a map read that lets a later member replace an earlier key.
    let schema = SchemaNodes::of(&<BTreeMap<u32, u8> as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    let repeated = from_json(&schema, "{\"1\":1,\"1\":2}", &mut budget).map(|_| ());
    let fault = repeated.map_err(|error| error.fault());
    assert!(matches!(fault, Err(JsonFault::Text { fault: TextFault::RepeatedName, .. })));
}

#[test]
fn enums_name_variants_and_forms() {
    // Catches an enum read that guesses a variant instead of refusing.
    let schema = SchemaNodes::of(&<Shape as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    let unknown = from_json(&schema, "\"Blob\"", &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert_eq!(unknown, Err(JsonFault::UnknownVariant));
    let mut budget = Budget::new(1024);
    let empty = from_json(&schema, "{\"Dot\":{}}", &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert_eq!(empty, Err(JsonFault::Expected(JsonForm::String)));
    let mut budget = Budget::new(1024);
    let bare = from_json(&schema, "\"Circle\"", &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert_eq!(bare, Err(JsonFault::Expected(JsonForm::Object)));
    let mut budget = Budget::new(1024);
    let two = from_json(&schema, "{\"Circle\":1,\"Dot\":2}", &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert_eq!(two, Err(JsonFault::Expected(JsonForm::Variant)));
}

#[test]
fn minus_zero_reads_as_zero() {
    // Catches an integer read that refuses `-0` although RFC 8259 allows it.
    let narrow = SchemaNodes::of(&<u8 as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    assert_eq!(from_json(&narrow, "-0", &mut budget), Ok(Vec::from([0x00])));
    let wide = SchemaNodes::of(&<i32 as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    assert_eq!(from_json(&wide, "-0", &mut budget), Ok(Vec::from([0x00, 0x00, 0x00, 0x00])));
}

#[test]
fn missing_option_is_none_and_missing_other_is_refused() {
    // Catches a reader that treats every missing member as none.
    let player = SchemaNodes::of(&<Player as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    assert_eq!(
        from_json(&player, "{\"id\":\"1\"}", &mut budget),
        Ok(Vec::from([0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]))
    );
    let mut budget = Budget::new(1024);
    let missing = from_json(&player, "{\"name\":\"x\"}", &mut budget);
    let Err(error) = missing else {
        panic!("id is missing");
    };
    assert_eq!(error.fault(), JsonFault::MissingField);
    assert_eq!(error.path().to_string(), "id");
    let holder = SchemaNodes::of(&<Holder as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    let empty = from_json(&holder, "{}", &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert_eq!(empty, Err(JsonFault::MissingField));
}

#[test]
fn members_out_of_order_read_in_declared_order() {
    // Catches a reader that writes bytes in member order instead of declared order.
    let schema = SchemaNodes::of(&<Tick as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    let swapped = from_json(&schema, "{\"delta_millis\":16,\"frame\":\"1\"}", &mut budget);
    let mut budget = Budget::new(1024);
    let ordered = from_json(&schema, "{\"frame\":\"1\",\"delta_millis\":16}", &mut budget);
    assert_eq!(swapped, ordered);
}

#[test]
fn maps_sort_into_key_order() {
    // Catches a map read that keeps member order instead of sorting by key.
    let numeric = SchemaNodes::of(&<BTreeMap<u32, bool> as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    assert_eq!(
        from_json(&numeric, "{\"10\":false,\"2\":true}", &mut budget),
        Ok(Vec::from([0x02, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x0a, 0x00, 0x00, 0x00, 0x00]))
    );
    let signed = SchemaNodes::of(&<BTreeMap<i8, u8> as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    assert_eq!(
        from_json(&signed, "{\"1\":1,\"-1\":2}", &mut budget),
        Ok(Vec::from([0x02, 0x00, 0x00, 0x00, 0xff, 0x02, 0x01, 0x01]))
    );
}

#[test]
fn bare_values_are_not_wrapped_options() {
    // Catches an option read that accepts a bare value where an array belongs.
    let schema = SchemaNodes::of(&<Option<Option<u8>> as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    let bare = from_json(&schema, "7", &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert_eq!(bare, Err(JsonFault::Expected(JsonForm::Array)));
    let mut budget = Budget::new(1024);
    let pair = from_json(&schema, "[1,2]", &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert_eq!(pair, Err(JsonFault::WrongLength { expected: 1, found: 2 }));
}

#[test]
fn nan_string_reads_quiet_f32() {
    // Catches a name read that parses instead of naming the quiet NaN.
    let schema = SchemaNodes::of(&<f32 as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    assert_eq!(from_json(&schema, "\"NaN\"", &mut budget), Ok(Vec::from([0x00, 0x00, 0xc0, 0x7f])));
}

#[test]
fn deep_paths_name_players_keys_and_variants() {
    // Catches a walk that loses the path on its way back out.
    let world = SchemaNodes::of(&<World as Schema>::SHAPE);
    let text = "{\"players\":[{\"id\":\"1\",\"name\":null},{\"id\":\"2\",\"name\":null},{\"id\":\"3\",\"name\":null},{\"id\":7,\"name\":null}]}";
    let mut budget = Budget::new(1 << 20);
    let bad = from_json(&world, text, &mut budget);
    let Err(error) = bad else {
        panic!("fourth id is a number");
    };
    assert_eq!(error.fault(), JsonFault::Expected(JsonForm::Digits));
    assert_eq!(error.path().to_string(), "players[3].id");
    let maps = SchemaNodes::of(&<BTreeMap<u32, bool> as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    let keyed = from_json(&maps, "{\"2\":true,\"10\":1}", &mut budget);
    let Err(error) = keyed else {
        panic!("map value is a number");
    };
    assert_eq!(error.path().to_string(), "[\"10\"]");
    let shapes = SchemaNodes::of(&<Shape as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    let variant = from_json(&shapes, "{\"Box\":{\"w\":true,\"h\":3}}", &mut budget);
    let Err(error) = variant else {
        panic!("variant field is a boolean");
    };
    assert_eq!(error.path().to_string(), "Box.w");
}

#[test]
fn deep_nesting_is_refused_without_recursion() {
    // Catches a walk that recurses on its JSON input instead of its schema.
    let mut text = "[".repeat(100_000);
    text.push('1');
    text.push_str(&"]".repeat(100_000));
    let schema = SchemaNodes::of(&<u8 as Schema>::SHAPE);
    let mut budget = Budget::new(usize::MAX);
    let deep = from_json(&schema, &text, &mut budget);
    let Err(error) = deep else {
        panic!("array is not a number");
    };
    assert_eq!(error.fault(), JsonFault::Expected(JsonForm::Number));
    assert!(error.path().is_root());
    drop(text);
}

#[test]
fn byte_sequences_have_one_form() {
    // Catches a bytes read that accepts an array of numbers.
    let schema = SchemaNodes::of(&<Vec<u8> as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    let array = from_json(&schema, "[1,2,3]", &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert_eq!(array, Err(JsonFault::Expected(JsonForm::Hex)));
}

#[test]
fn empty_budget_refuses_any_text() {
    // Catches a reader that holds values it never charged.
    let schema = SchemaNodes::of(&<u8 as Schema>::SHAPE);
    let mut budget = Budget::new(0);
    let refused = from_json(&schema, "1", &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert!(matches!(refused, Err(JsonFault::Decode(DecodeError::BudgetExceeded { .. }))));
    let text = "1".to_string();
    let _ = text;
}
