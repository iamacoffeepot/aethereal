//! Tests for the bytes-to-JSON walk.

use super::to_json;
use crate::json::rows::rows;
use crate::{Budget, DecodeError, JsonFault, Plain, Schema, SchemaNodes};
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Plain)]
struct FloatHolder(f64);

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
struct Unit;

#[test]
fn every_row_writes_its_json() {
    // Catches a shape whose writer drifts from the table.
    for row in rows() {
        let mut budget = Budget::new(1 << 20);
        let text = to_json(&row.schema, &row.bytes, &mut budget);
        assert_eq!(text, Ok(row.json.clone()), "row {}", row.json);
    }
}

#[test]
fn bad_bytes_are_refused() {
    // Catches a writer that trusts the bytes instead of decoding them.
    let truncated = SchemaNodes::of(&<u32 as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    assert_eq!(to_json(&truncated, &[0x01, 0x02], &mut budget).map(|_| ()), Err(DecodeError::UnexpectedEnd.into()));

    let trailing = SchemaNodes::of(&<u8 as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    assert_eq!(
        to_json(&trailing, &[0x01, 0x02], &mut budget).map(|_| ()),
        Err(DecodeError::TrailingBytes { count: 1 }.into())
    );

    let boolean = SchemaNodes::of(&<bool as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    assert_eq!(to_json(&boolean, &[0x02], &mut budget).map(|_| ()), Err(DecodeError::InvalidBoolean(2).into()));

    let option = SchemaNodes::of(&<Option<u8> as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    assert_eq!(to_json(&option, &[0x02], &mut budget).map(|_| ()), Err(DecodeError::InvalidOption(2).into()));

    let string = SchemaNodes::of(&<String as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    assert_eq!(
        to_json(&string, &[0x01, 0x00, 0x00, 0x00, 0xff], &mut budget).map(|_| ()),
        Err(DecodeError::InvalidUtf8.into())
    );
}

#[test]
fn unknown_selector_is_refused() {
    // Catches a writer that trusts a selector instead of finding its variant.
    let found = rows().into_iter().find(|row| row.json == "\"Dot\"");
    let Some(row) = found else {
        panic!("shape row is pinned");
    };
    let mut budget = Budget::new(1024);
    assert_eq!(
        to_json(&row.schema, &[0x07, 0x00, 0x00, 0x00], &mut budget).map(|_| ()),
        Err(JsonFault::UnknownSelector(7).into())
    );
}

#[test]
fn unordered_map_keys_are_refused() {
    // Catches a writer that trusts the key order instead of checking it.
    let schema = SchemaNodes::of(&<BTreeMap<u8, u8> as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    let first = to_json(&schema, &[0x02, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0x02], &mut budget)
        .map(|_| ())
        .map_err(|error| error.fault());
    assert_eq!(first, Err(JsonFault::Decode(DecodeError::KeyOrder)));
    let mut budget = Budget::new(1024);
    let second = to_json(&schema, &[0x02, 0x00, 0x00, 0x00, 0x02, 0x01, 0x01, 0x02], &mut budget)
        .map(|_| ())
        .map_err(|error| error.fault());
    assert_eq!(second, Err(JsonFault::Decode(DecodeError::KeyOrder)));
}

#[test]
fn dirty_nan_writes_quoted_nan() {
    // Catches a NaN check that looks at bits instead of `is_nan`.
    let schema = SchemaNodes::of(&<FloatHolder as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    assert_eq!(
        to_json(&schema, &[0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0x7f], &mut budget),
        Ok(String::from("\"NaN\""))
    );
}

#[test]
fn deep_path_names_players() {
    // Catches a walk that loses the path on its way back out.
    let schema = SchemaNodes::of(&<World as Schema>::SHAPE);
    let bytes = Vec::from([
        0x04, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00,
    ]);
    let mut budget = Budget::new(1 << 20);
    let result = to_json(&schema, &bytes, &mut budget);
    let Err(error) = result else {
        panic!("fourth player ends early");
    };
    assert_eq!(error.path().to_string(), "players[3].id");
}

#[test]
fn huge_count_does_not_preallocate() {
    // Catches a walk that reserves capacity from a count it does not trust.
    let schema = SchemaNodes::of(&<Vec<u16> as Schema>::SHAPE);
    let mut budget = Budget::new(1 << 20);
    let huge = to_json(&schema, &[0xff, 0xff, 0xff, 0xff], &mut budget).map(|_| ()).map_err(|error| error.fault());
    assert_eq!(huge, Err(JsonFault::Decode(DecodeError::UnexpectedEnd)));
}

#[test]
fn empty_elements_still_charge_budget() {
    // Catches four bytes of input writing gigabytes of text.
    let unit_schema = SchemaNodes::of(&<Vec<Unit> as Schema>::SHAPE);
    let mut budget = Budget::new(1024);
    let unit_result = to_json(&unit_schema, &[0xff, 0xff, 0xff, 0xff], &mut budget);
    let unit_fault = unit_result.map(|_| ()).map_err(|error| error.fault());
    assert!(matches!(unit_fault, Err(JsonFault::Decode(DecodeError::BudgetExceeded { .. }))));

    let empty_schema = SchemaNodes::of(&<Vec<()> as Schema>::SHAPE);
    let mut empty_budget = Budget::new(1024);
    let empty_result = to_json(&empty_schema, &[0xff, 0xff, 0xff, 0xff], &mut empty_budget);
    let empty_fault = empty_result.map(|_| ()).map_err(|error| error.fault());
    assert!(matches!(empty_fault, Err(JsonFault::Decode(DecodeError::BudgetExceeded { .. }))));
}

#[test]
fn budget_exactly_covers_tick() {
    // Catches text that is written and never charged.
    let found = rows().into_iter().find(|row| row.json.contains("delta_millis"));
    let Some(row) = found else {
        panic!("tick row is pinned");
    };
    let len = row.json.len();
    let mut budget = Budget::new(len);
    assert_eq!(to_json(&row.schema, &row.bytes, &mut budget), Ok(row.json.clone()));
    assert_eq!(budget.remaining(), 0);
    let mut short = Budget::new(len - 1);
    assert!(to_json(&row.schema, &row.bytes, &mut short).is_err());
}
