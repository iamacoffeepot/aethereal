//! One table of values both walks are tested against.

use crate::{Json, Key, Plain, Schema, SchemaNodes};
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use alloc::vec::Vec;

/// One pinned value in three forms.
pub(super) struct Row {
    /// The value's schema.
    pub(super) schema: SchemaNodes,
    /// The value's bytes, written by hand.
    pub(super) bytes: Vec<u8>,
    /// The value's JSON, written by hand.
    pub(super) json: String,
}

#[derive(Plain)]
struct Wrap(Option<u8>);

#[derive(Plain, Key)]
struct PlayerId(u64);

#[derive(Plain)]
struct Tick {
    frame: u64,
    delta_millis: u32,
}

#[derive(Plain)]
struct Span(u32, u32);

#[derive(Plain)]
struct Unit;

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

#[derive(Plain)]
struct Player {
    id: u64,
    name: Option<String>,
}

#[derive(Plain)]
struct World {
    players: Vec<Player>,
}

/// Every form pinned once, proved in both directions.
pub(super) fn rows() -> Vec<Row> {
    let mut all = scalar_rows();
    all.extend(option_rows());
    all.extend(sequence_rows());
    all.extend(map_rows());
    all.extend(struct_rows());
    all.extend(enum_rows());
    all.extend(json_rows());
    all
}

/// Booleans, integers, floats, strings, and bytes.
fn scalar_rows() -> Vec<Row> {
    alloc::vec![
        Row { schema: SchemaNodes::of(&<bool as Schema>::SHAPE), bytes: alloc::vec![0x01], json: String::from("true") },
        Row { schema: SchemaNodes::of(&<u8 as Schema>::SHAPE), bytes: alloc::vec![0xff], json: String::from("255") },
        Row { schema: SchemaNodes::of(&<i8 as Schema>::SHAPE), bytes: alloc::vec![0x80], json: String::from("-128") },
        Row {
            schema: SchemaNodes::of(&<u16 as Schema>::SHAPE),
            bytes: alloc::vec![0xff, 0xff],
            json: String::from("65535"),
        },
        Row {
            schema: SchemaNodes::of(&<i32 as Schema>::SHAPE),
            bytes: alloc::vec![0xff, 0xff, 0xff, 0xff],
            json: String::from("-1"),
        },
        Row {
            schema: SchemaNodes::of(&<u32 as Schema>::SHAPE),
            bytes: alloc::vec![0xff, 0xff, 0xff, 0xff],
            json: String::from("4294967295"),
        },
        Row {
            schema: SchemaNodes::of(&<u64 as Schema>::SHAPE),
            bytes: alloc::vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
            json: String::from("\"0\""),
        },
        Row {
            schema: SchemaNodes::of(&<u64 as Schema>::SHAPE),
            bytes: alloc::vec![0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
            json: String::from("\"18446744073709551615\""),
        },
        Row {
            schema: SchemaNodes::of(&<i64 as Schema>::SHAPE),
            bytes: alloc::vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80],
            json: String::from("\"-9223372036854775808\""),
        },
        Row {
            schema: SchemaNodes::of(&<f32 as Schema>::SHAPE),
            bytes: alloc::vec![0x00, 0x00, 0xc0, 0x3f],
            json: String::from("1.5"),
        },
        Row {
            schema: SchemaNodes::of(&<f64 as Schema>::SHAPE),
            bytes: alloc::vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40],
            json: String::from("2"),
        },
        Row {
            schema: SchemaNodes::of(&<f64 as Schema>::SHAPE),
            bytes: alloc::vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80],
            json: String::from("-0"),
        },
        Row {
            schema: SchemaNodes::of(&<f32 as Schema>::SHAPE),
            bytes: alloc::vec![0x00, 0x00, 0x80, 0x7f],
            json: String::from("\"Infinity\""),
        },
        Row {
            schema: SchemaNodes::of(&<f64 as Schema>::SHAPE),
            bytes: alloc::vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0xff],
            json: String::from("\"-Infinity\""),
        },
        Row {
            schema: SchemaNodes::of(&<f64 as Schema>::SHAPE),
            bytes: alloc::vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf8, 0x7f],
            json: String::from("\"NaN\""),
        },
        Row {
            schema: SchemaNodes::of(&<String as Schema>::SHAPE),
            bytes: alloc::vec![0x05, 0x00, 0x00, 0x00, 0x22, 0x5c, 0x0a, 0xc3, 0xa9],
            json: String::from("\"\\\"\\\\\\n\u{e9}\""),
        },
        Row {
            schema: SchemaNodes::of(&<Vec<u8> as Schema>::SHAPE),
            bytes: alloc::vec![0x03, 0x00, 0x00, 0x00, 0x00, 0xab, 0xff],
            json: String::from("\"00abff\""),
        },
        Row {
            schema: SchemaNodes::of(&<Vec<u8> as Schema>::SHAPE),
            bytes: alloc::vec![0x00, 0x00, 0x00, 0x00],
            json: String::from("\"\""),
        },
    ]
}

/// Options, wrapped and nested.
fn option_rows() -> Vec<Row> {
    alloc::vec![
        Row {
            schema: SchemaNodes::of(&<Option<u8> as Schema>::SHAPE),
            bytes: alloc::vec![0x00],
            json: String::from("null"),
        },
        Row {
            schema: SchemaNodes::of(&<Option<u8> as Schema>::SHAPE),
            bytes: alloc::vec![0x01, 0x07],
            json: String::from("7"),
        },
        Row {
            schema: SchemaNodes::of(&<Option<Option<u8>> as Schema>::SHAPE),
            bytes: alloc::vec![0x00],
            json: String::from("null"),
        },
        Row {
            schema: SchemaNodes::of(&<Option<Option<u8>> as Schema>::SHAPE),
            bytes: alloc::vec![0x01, 0x00],
            json: String::from("[null]"),
        },
        Row {
            schema: SchemaNodes::of(&<Option<Option<u8>> as Schema>::SHAPE),
            bytes: alloc::vec![0x01, 0x01, 0x07],
            json: String::from("[7]"),
        },
        Row {
            schema: SchemaNodes::of(&<Option<Option<Option<u8>>> as Schema>::SHAPE),
            bytes: alloc::vec![0x01, 0x01, 0x00],
            json: String::from("[[null]]"),
        },
        Row {
            schema: SchemaNodes::of(&<Option<Option<Option<u8>>> as Schema>::SHAPE),
            bytes: alloc::vec![0x01, 0x01, 0x01, 0x07],
            json: String::from("[[7]]"),
        },
        Row {
            schema: SchemaNodes::of(&<Option<Wrap> as Schema>::SHAPE),
            bytes: alloc::vec![0x00],
            json: String::from("null"),
        },
        Row {
            schema: SchemaNodes::of(&<Option<Wrap> as Schema>::SHAPE),
            bytes: alloc::vec![0x01, 0x00],
            json: String::from("[null]"),
        },
        Row {
            schema: SchemaNodes::of(&<Option<Wrap> as Schema>::SHAPE),
            bytes: alloc::vec![0x01, 0x01, 0x07],
            json: String::from("[7]"),
        },
        Row {
            schema: SchemaNodes::of(&<Option<(Option<u8>,)> as Schema>::SHAPE),
            bytes: alloc::vec![0x00],
            json: String::from("null"),
        },
        Row {
            schema: SchemaNodes::of(&<Option<(Option<u8>,)> as Schema>::SHAPE),
            bytes: alloc::vec![0x01, 0x00],
            json: String::from("[null]"),
        },
    ]
}

/// Sequences, fixed arrays, and sets.
fn sequence_rows() -> Vec<Row> {
    alloc::vec![
        Row {
            schema: SchemaNodes::of(&<Vec<u16> as Schema>::SHAPE),
            bytes: alloc::vec![0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x02, 0x00],
            json: String::from("[1,2]"),
        },
        Row {
            schema: SchemaNodes::of(&<Vec<u16> as Schema>::SHAPE),
            bytes: alloc::vec![0x00, 0x00, 0x00, 0x00],
            json: String::from("[]"),
        },
        Row {
            schema: SchemaNodes::of(&<[u16; 2] as Schema>::SHAPE),
            bytes: alloc::vec![0x01, 0x00, 0x02, 0x00],
            json: String::from("[1,2]"),
        },
        Row {
            schema: SchemaNodes::of(&<[u8; 3] as Schema>::SHAPE),
            bytes: alloc::vec![0x01, 0x02, 0x03],
            json: String::from("\"010203\""),
        },
        Row {
            schema: SchemaNodes::of(&<BTreeSet<u8> as Schema>::SHAPE),
            bytes: alloc::vec![0x02, 0x00, 0x00, 0x00, 0x01, 0x02],
            json: String::from("[1,2]"),
        },
    ]
}

/// Maps with every key shape.
fn map_rows() -> Vec<Row> {
    alloc::vec![
        Row {
            schema: SchemaNodes::of(&<BTreeMap<u32, bool> as Schema>::SHAPE),
            bytes: alloc::vec![0x02, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x0a, 0x00, 0x00, 0x00, 0x00],
            json: String::from("{\"2\":true,\"10\":false}"),
        },
        Row {
            schema: SchemaNodes::of(&<BTreeMap<i8, u8> as Schema>::SHAPE),
            bytes: alloc::vec![0x02, 0x00, 0x00, 0x00, 0xff, 0x01, 0x01, 0x02],
            json: String::from("{\"-1\":1,\"1\":2}"),
        },
        Row {
            schema: SchemaNodes::of(&<BTreeMap<String, u8> as Schema>::SHAPE),
            bytes: alloc::vec![
                0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x61, 0x01, 0x01, 0x00, 0x00, 0x00, 0x62, 0x02
            ],
            json: String::from("{\"a\":1,\"b\":2}"),
        },
        Row {
            schema: SchemaNodes::of(&<BTreeMap<PlayerId, u8> as Schema>::SHAPE),
            bytes: alloc::vec![0x01, 0x00, 0x00, 0x00, 0x2a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01],
            json: String::from("{\"42\":1}"),
        },
        Row {
            schema: SchemaNodes::of(&<BTreeMap<[u8; 2], u8> as Schema>::SHAPE),
            bytes: alloc::vec![0x01, 0x00, 0x00, 0x00, 0x00, 0xff, 0x01],
            json: String::from("{\"00ff\":1}"),
        },
        Row {
            schema: SchemaNodes::of(&<BTreeMap<u32, u8> as Schema>::SHAPE),
            bytes: alloc::vec![0x00, 0x00, 0x00, 0x00],
            json: String::from("{}"),
        },
    ]
}

/// Structs, tuples, and units.
fn struct_rows() -> Vec<Row> {
    alloc::vec![
        Row {
            schema: SchemaNodes::of(&<Tick as Schema>::SHAPE),
            bytes: alloc::vec![0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x10, 0x00, 0x00, 0x00],
            json: String::from("{\"frame\":\"18446744073709551615\",\"delta_millis\":16}"),
        },
        Row {
            schema: SchemaNodes::of(&<PlayerId as Schema>::SHAPE),
            bytes: alloc::vec![0x2a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
            json: String::from("\"42\""),
        },
        Row {
            schema: SchemaNodes::of(&<Span as Schema>::SHAPE),
            bytes: alloc::vec![0x03, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00],
            json: String::from("[3,9]"),
        },
        Row { schema: SchemaNodes::of(&<Unit as Schema>::SHAPE), bytes: alloc::vec![], json: String::from("{}") },
        Row {
            schema: SchemaNodes::of(&<(u8, bool) as Schema>::SHAPE),
            bytes: alloc::vec![0x01, 0x01],
            json: String::from("[1,true]"),
        },
        Row { schema: SchemaNodes::of(&<() as Schema>::SHAPE), bytes: alloc::vec![], json: String::from("[]") },
        Row {
            schema: SchemaNodes::of(&<World as Schema>::SHAPE),
            bytes: alloc::vec![
                0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00,
                0x61, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00
            ],
            json: String::from("{\"players\":[{\"id\":\"1\",\"name\":\"a\"},{\"id\":\"2\",\"name\":null}]}"),
        },
    ]
}

/// Enums in every layout.
fn enum_rows() -> Vec<Row> {
    alloc::vec![
        Row {
            schema: SchemaNodes::of(&<Shape as Schema>::SHAPE),
            bytes: alloc::vec![0x00, 0x00, 0x00, 0x00],
            json: String::from("\"Dot\""),
        },
        Row {
            schema: SchemaNodes::of(&<Shape as Schema>::SHAPE),
            bytes: alloc::vec![0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc0, 0x3f],
            json: String::from("{\"Circle\":1.5}"),
        },
        Row {
            schema: SchemaNodes::of(&<Shape as Schema>::SHAPE),
            bytes: alloc::vec![0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x40, 0x40],
            json: String::from("{\"Box\":{\"w\":2,\"h\":3}}"),
        },
        Row {
            schema: SchemaNodes::of(&<Shape as Schema>::SHAPE),
            bytes: alloc::vec![0x03, 0x00, 0x00, 0x00, 0x01, 0x02],
            json: String::from("{\"Pair\":[1,2]}"),
        },
        Row {
            schema: SchemaNodes::of(&<Shape as Schema>::SHAPE),
            bytes: alloc::vec![0x09, 0x00, 0x00, 0x00],
            json: String::from("\"Far\""),
        },
    ]
}

/// `text` laid out as a string: its byte count as four bytes little-endian, then its bytes.
fn counted(text: &str) -> Vec<u8> {
    let Ok(count) = u32::try_from(text.len()) else {
        panic!("a row's text fits a count");
    };
    let mut bytes = count.to_le_bytes().to_vec();
    bytes.extend_from_slice(text.as_bytes());

    bytes
}

/// JSON values carried as their text: written as they are, never as a string, and wrapped in an option.
fn json_rows() -> Vec<Row> {
    let nested = "{\"a\":[1,true,null],\"b\":{\"c\":\"x\\n\"}}";
    let mut held_null = alloc::vec![0x01];
    held_null.extend(counted("null"));
    alloc::vec![
        Row { schema: SchemaNodes::of(&<Json as Schema>::SHAPE), bytes: counted(nested), json: String::from(nested) },
        Row {
            schema: SchemaNodes::of(&<Json as Schema>::SHAPE),
            bytes: counted("\"a b\""),
            json: String::from("\"a b\""),
        },
        Row {
            schema: SchemaNodes::of(&<Option<Json> as Schema>::SHAPE),
            bytes: held_null,
            json: String::from("[null]"),
        },
    ]
}
