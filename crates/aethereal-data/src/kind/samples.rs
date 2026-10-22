//! Kinds the `kind` and `same_layout` tests share (ADR-0006).

use crate::{Budget, FieldNode, Plain, SchemaNode, SchemaNodes, from_bytes, to_bytes};
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::iter::once;

/// Two bytes the layout and id tests hash.
#[derive(Plain)]
#[kind(name = "aether.audio.note_on")]
pub struct NoteOn {
    pitch: u8,
    velocity: u8,
}

/// Two floats; not a kind, only a shape the tests share.
#[derive(Plain)]
pub struct Point {
    x: f32,
    y: f32,
}

/// A struct touching every leaf shape the tests hash.
#[derive(Plain)]
#[kind(name = "test.path")]
pub struct Path {
    label: String,
    points: Vec<Point>,
    origin: Point,
    closed: bool,
    weights: [f64; 3],
    parent: Option<u64>,
    raw: Vec<u8>,
}

/// An enum with an empty, a named, and an unnamed variant.
#[derive(Plain)]
#[kind(name = "test.command")]
pub enum Command {
    Stop,
    #[plain(selector = 7)]
    Move {
        dx: i32,
        dy: i32,
    },
    Say(String, u16),
}

/// A struct holding maps and a set.
#[derive(Plain)]
#[kind(name = "test.ledger")]
pub struct Ledger {
    balances: BTreeMap<u64, i64>,
    notes: BTreeMap<String, (u32, bool)>,
    marks: BTreeSet<u16>,
}

/// Two floats under other names; the `Point` its copies use.
#[derive(Plain)]
pub struct Spot {
    across: f32,
    up: f32,
}

/// `Path` with every field and struct renamed and `Point` replaced by `Spot`.
#[derive(Plain)]
#[kind(name = "test.path")]
pub struct Route {
    title: String,
    stops: Vec<Spot>,
    start: Spot,
    done: bool,
    factors: [f64; 3],
    previous: Option<u64>,
    blob: Vec<u8>,
}

/// `Command` renamed, with named and unnamed fields swapped on purpose.
#[derive(Plain)]
#[kind(name = "test.command")]
pub enum Renamed {
    Halt,
    #[plain(selector = 7)]
    Go(i32, i32),
    Speak {
        text: String,
        volume: u16,
    },
}

/// `Command` with the middle selector changed.
#[derive(Plain)]
#[kind(name = "test.command")]
pub enum Reselected {
    Stop,
    #[plain(selector = 8)]
    Move {
        dx: i32,
        dy: i32,
    },
    Say(String, u16),
}

/// `NoteOn` with the second byte widened.
#[derive(Plain)]
#[kind(name = "aether.audio.note_on")]
pub struct Wide {
    pitch: u8,
    velocity: u16,
}

/// `NoteOn` under another name.
#[derive(Plain)]
#[kind(name = "aether.audio.note_off")]
pub struct NoteOff {
    pitch: u8,
    velocity: u8,
}

/// A flat schema of `depth` structs over `leaf`: each has two fields, and both name the level below.
pub fn doubling(depth: u32, prefix: &str, leaf: SchemaNode) -> SchemaNodes {
    let mut levels = Vec::new();
    for level in 0..depth {
        let name = format!("{prefix}{level}");
        let fields = Vec::from([
            FieldNode { name: String::from("left"), shape: level },
            FieldNode { name: String::from("right"), shape: level },
        ]);
        levels.push(SchemaNode::Struct { name, fields });
    }
    let nodes: Vec<SchemaNode> = once(leaf).chain(levels).collect();
    let Ok(bytes) = to_bytes(&nodes) else {
        panic!("a doubling schema encodes");
    };
    let Ok(schema) = from_bytes::<SchemaNodes>(&bytes, &mut Budget::new(1 << 20)) else {
        panic!("a doubling schema decodes");
    };

    schema
}
