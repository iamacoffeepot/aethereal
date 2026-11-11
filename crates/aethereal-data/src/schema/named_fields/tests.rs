use super::*;
use crate::{Plain, Schema, to_bytes};
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Plain)]
enum Mood {
    Calm,
    #[plain(selector = 4)]
    Torn {
        left: u8,
        right: String,
    },
}

#[derive(Plain)]
struct Mark;

/// A field of every shape whose bytes vary, each before another field.
#[derive(Plain)]
struct Varied {
    first: u32,
    note: Option<String>,
    mood: Mood,
    scores: BTreeMap<u32, String>,
    marks: Vec<Mark>,
    pairs: Vec<(u8, u16)>,
    names: [String; 2],
    last: u64,
}

fn varied() -> Varied {
    Varied {
        first: 7,
        note: Some(String::from("kept")),
        mood: Mood::Torn { left: 3, right: String::from("both") },
        scores: BTreeMap::from([(1, String::from("one")), (9, String::from("nine"))]),
        marks: Vec::from([Mark, Mark, Mark]),
        pairs: Vec::from([(1, 2), (3, 4)]),
        names: [String::from("a"), String::from("bc")],
        last: 0x0102_0304_0506_0708,
    }
}

#[derive(Plain)]
struct SaveOne {
    slot: u8,
    label: String,
}

#[derive(Plain)]
struct SaveTwo {
    index: u8,
    title: String,
}

#[derive(Plain)]
struct SaveWide {
    slot: u16,
    label: String,
}

#[derive(Plain)]
struct Saved {
    save: SaveOne,
}

/// The bytes `value` encodes to.
fn bytes_of<T: Plain>(value: &T) -> Vec<u8> {
    to_bytes(value).unwrap_or_default()
}

#[test]
fn each_field_s_bytes_begin_where_the_fields_before_it_end() {
    // Catches a walk that misjudges where a value of varying size ends (an option, an enum's variant, a map's pairs, a
    // run of values that take no bytes or the same bytes each), so every field after it is read from the wrong bytes.
    let value = varied();
    let schema = SchemaNodes::of(&Varied::SHAPE);
    let bytes = bytes_of(&value);
    let Ok(mut fields) = NamedFields::of(&schema, &bytes) else {
        panic!("the bytes are a value of the schema");
    };

    assert_eq!(fields.field("first", &u32::SHAPE), FieldBytes::Same(&bytes_of(&value.first)));
    assert_eq!(fields.field("note", &Option::<String>::SHAPE), FieldBytes::Same(&bytes_of(&value.note)));
    assert_eq!(fields.field("mood", &Mood::SHAPE), FieldBytes::Same(&bytes_of(&value.mood)));
    assert_eq!(fields.field("scores", &BTreeMap::<u32, String>::SHAPE), FieldBytes::Same(&bytes_of(&value.scores)));
    assert_eq!(fields.field("marks", &Vec::<Mark>::SHAPE), FieldBytes::Same(&bytes_of(&value.marks)));
    assert_eq!(fields.field("pairs", &Vec::<(u8, u16)>::SHAPE), FieldBytes::Same(&bytes_of(&value.pairs)));
    assert_eq!(fields.field("names", &<[String; 2]>::SHAPE), FieldBytes::Same(&bytes_of(&value.names)));
    assert_eq!(fields.field("last", &u64::SHAPE), FieldBytes::Same(&bytes_of(&value.last)));
}

#[test]
fn a_field_is_found_by_its_name_and_matched_by_its_layout() {
    // Catches a field matched by its name alone, so a changed type reads its bytes as something else; one matched with
    // the names inside it, so a renamed type inside a field loses its value; and a field found by its position.
    let value = Saved { save: SaveOne { slot: 2, label: String::from("north") } };
    let schema = SchemaNodes::of(&Saved::SHAPE);
    let bytes = bytes_of(&value);
    let Ok(mut fields) = NamedFields::of(&schema, &bytes) else {
        panic!("the bytes are a value of the schema");
    };

    assert_eq!(fields.field("save", &SaveTwo::SHAPE), FieldBytes::Same(&bytes));
    assert_eq!(fields.field("save", &SaveWide::SHAPE), FieldBytes::OtherLayout);
    assert_eq!(fields.field("saved", &SaveTwo::SHAPE), FieldBytes::Absent);
}

#[test]
fn bytes_that_are_no_struct_or_more_than_one_are_refused() {
    // Catches an enum's bytes read as a struct with no fields, which throws its value away, and bytes past the last
    // field ignored, which reads a value written with another schema as this one.
    let mood = SchemaNodes::of(&Mood::SHAPE);
    let calm = bytes_of(&Mood::Calm);
    let saved = SchemaNodes::of(&Saved::SHAPE);
    let mut longer = bytes_of(&Saved { save: SaveOne { slot: 2, label: String::new() } });
    longer.push(0);

    assert_eq!(NamedFields::of(&mood, &calm).err(), Some(DecodeError::NotAStruct));
    assert_eq!(NamedFields::of(&saved, &longer).err(), Some(DecodeError::TrailingBytes { count: 1 }));
}
