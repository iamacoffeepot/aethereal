//! The section's tests: the schema of a state as a constant writes it, and what a host refuses of a section it reads.

use super::*;
use aethereal_data::{Kind, Schema, SchemaNode, to_bytes};
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;

/// A point, which the state below holds in several places, so its node is shared.
#[derive(Plain)]
#[kind(name = "aether.test.point")]
struct Point {
    x: f32,
    y: f32,
}

/// A point's layout under other names: the same kind as a point, as names are not in a kind's identity (ADR-0006).
#[derive(Plain)]
struct Spot {
    across: f32,
    up: f32,
}

/// A point's names over another layout.
#[derive(Plain)]
struct Wide {
    x: f64,
    y: f32,
}

/// A mark, of each kind of variant.
#[derive(Plain)]
enum Mark {
    Bare,
    At(Point, u8),
    Named { point: Point, label: String },
}

/// A state of every shape a schema has but a JSON value, some held twice and some nested in others.
#[derive(Plain)]
struct Everything {
    on: bool,
    count: i64,
    ratio: f64,
    label: String,
    bytes: Vec<u8>,
    maybe: Option<Point>,
    points: Vec<Point>,
    corners: [Point; 4],
    marks: BTreeMap<String, Mark>,
    pair: (u8, Point),
    again: Point,
}

/// A state of every shape a schema has, a JSON value among them, which no plain type here holds.
const EVERYTHING: &Shape = &Shape::Tuple { elements: &[Everything::SHAPE, Shape::Json] };
const ROOM: usize = state::shapes(EVERYTHING);
const LEN: usize = state::write::<ROOM>(&mut [], 0, EVERYTHING);
const WRITTEN: [u8; LEN] = {
    let mut out = [0; LEN];
    state::write::<ROOM>(&mut out, 0, EVERYTHING);

    out
};

/// A section of the one type of a point's name and id, whose state is of the schema `state`.
fn section_of_a_point(state: SchemaNodes) -> Vec<u8> {
    let point = Made {
        name: Point::NAME,
        id: KindId::of::<Point>(),
        keyed: Key::Singleton,
        spawner: Spawned::Anyone,
        begins: vec![],
        handlers: vec![],
        ends: vec![],
        state,
    };
    let Ok(section) = to_bytes(&vec![point]) else {
        panic!("a section encodes");
    };

    section
}

#[test]
fn a_state_written_in_a_constant_is_the_flat_schema_its_shape_makes() {
    // Catches the constant writer of a state's schema and `SchemaNodes::of` drifting apart: a shape given a node when
    // one equal to it has one, children written in another order, or a selector numbered otherwise than the case it
    // stands for, so a host refuses the section, or reads a schema other than the type's.
    let Ok(made) = to_bytes(&SchemaNodes::of(EVERYTHING)) else {
        panic!("a flat schema encodes");
    };

    assert_eq!(WRITTEN.to_vec(), made);
}

#[test]
fn a_section_is_refused_for_a_state_of_another_layout_than_its_id_and_taken_for_one_renamed() {
    // Catches a section taken whose type's state is of another layout than its id, by which a moving actor's state
    // would be read (ADR-0054), and a check that compares names, which a kind's identity leaves out (ADR-0006), so a
    // field renamed refuses its module.
    let mut budget = Budget::new(1 << 16);
    let wide = read(&section_of_a_point(SchemaNodes::of(&Wide::SHAPE)), &mut budget).err();
    let spot = read(&section_of_a_point(SchemaNodes::of(&Spot::SHAPE)), &mut budget).map(|made| made.len());

    assert_eq!((wide, spot), (Some(Unfit::Misstated(Point::NAME)), Ok(1)));
}

#[test]
fn a_section_whose_state_does_not_read_back_is_refused() {
    // Catches a state's schema read without the checks a flat schema is read with, so a node naming one after it
    // reaches the walk a moving actor's state is read by (ADR-0003, ADR-0054).
    let state = SchemaNodes::of(&Point::SHAPE);
    let (Ok(written), Ok(forward)) = (to_bytes(&state), to_bytes(&vec![SchemaNode::Sequence(1), SchemaNode::Boolean]))
    else {
        panic!("a schema encodes");
    };
    let section = section_of_a_point(state);
    let kept = section.len() - written.len();
    let unread: Vec<u8> = section.iter().copied().take(kept).chain(forward).collect();

    assert_eq!(read(&unread, &mut Budget::new(1 << 16)).err(), Some(Unfit::Undeclared));
}
