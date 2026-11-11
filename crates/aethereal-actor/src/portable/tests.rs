use super::*;
use crate::{BlobId, Bytes, Portability, pins};
use aethereal_data::{Bound, Encode, Plain};
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Plain)]
struct SaveOne {
    slot: u8,
    label: String,
}

#[derive(Plain, Debug, PartialEq)]
struct Save {
    index: u8,
    title: String,
}

/// A HUD's state as its first version wrote it.
#[derive(Plain)]
struct HudOne {
    hp: u32,
    cooldowns: Vec<u32>,
    health: f32,
    save: SaveOne,
}

/// The HUD's next version: `cooldowns` removed, `hp` renamed, `show_cooldowns` new, and the rest reordered.
#[derive(Plain, Portability)]
struct Hud {
    save: Save,
    health: f32,
    show_cooldowns: bool,
    hit_points: u32,
}

/// Its health, as a version that changed the field's type without naming it anew wrote it.
#[derive(Plain)]
struct Counted {
    health: u32,
}

/// Two names, each well inside a read limit of 100 bytes and together past it.
#[derive(Plain, Portability)]
struct Names {
    first: String,
    second: String,
}

/// A state holding two references.
#[derive(Bound)]
struct Pictured {
    icon: Bytes,
    banner: Bytes,
}

/// Its next version, which keeps the icon and leaves the banner behind.
#[derive(Bound, Portability)]
struct Iconed {
    icon: Bytes,
}

/// `T`'s portable form of `old`, as `old`'s own type wrote it, read within `read_limit` bytes.
fn moved<T: Portability, Old: Encode<Within> + Schema>(old: &Old, read_limit: usize) -> Result<T::Form, TakeOverError> {
    let Ok(written) = Written::of(old) else {
        panic!("the old state is written");
    };

    T::read(&OldState::new(&written, &SchemaNodes::of(&Old::SHAPE), read_limit))
}

/// Each blob's net change of pins among `changes`.
fn net(changes: &[pins::Change], blob: BlobId) -> i64 {
    changes.iter().filter(|(id, _)| *id == blob).map(|(_, by)| by).sum()
}

#[test]
fn each_field_is_read_by_its_name_whatever_the_old_state_held_around_it() {
    // Catches a field read from where its new version places it, or from the wrong bytes once a field before it was
    // removed; a field whose type was renamed inside, with fields renamed, taken for another; and a renamed field read
    // by its position or its type, which a version must name in `take_over` instead.
    let old = HudOne {
        hp: 30,
        cooldowns: Vec::from([4, 5, 6]),
        health: 40.5,
        save: SaveOne { slot: 2, label: String::from("north") },
    };

    let Ok(hud) = moved::<Hud, _>(&old, 1024) else {
        panic!("the state moves");
    };

    assert_eq!(hud.save, Some(Save { index: 2, title: String::from("north") }));
    assert_eq!(hud.health, Some(40.5));
    assert_eq!(hud.show_cooldowns, None);
    assert_eq!(hud.hit_points, None);
}

#[test]
fn a_field_of_another_layout_refuses_the_move() {
    // Catches a field matched by its name alone, so a `u32`'s bits are read as an `f32`, and one read as absent, so
    // its value is replaced by a default no one chose.
    let moved = moved::<Hud, _>(&Counted { health: 1_000 }, 1024);

    assert_eq!(moved.err(), Some(TakeOverError::Mismatch { field: "health" }));
}

#[test]
fn the_read_limit_holds_for_the_whole_state() {
    // Catches each field read within a limit of its own, so a state of many fields reads past the engine's limit.
    let names = Names { first: "a".repeat(60), second: "b".repeat(60) };

    let within = moved::<Names, _>(&names, 200).map(|form| form.first.is_some() && form.second.is_some());
    let past = moved::<Names, _>(&names, 100).err();

    assert_eq!(within, Ok(true));
    assert!(matches!(past, Some(TakeOverError::Unreadable(DecodeError::BudgetExceeded { .. }))), "{past:?}");
}

#[test]
fn a_reference_kept_is_a_pin_of_its_own_and_one_left_behind_goes_with_the_old_state() {
    // Catches a reference read across with no pin of its own, freed once the old state goes; and a reference in a
    // field left behind kept pinned by the read, so its blob outlives the state that held it.
    let (icon, banner) = (BlobId::new(21, 3), BlobId::new(21, 4));
    pins::open(21, Vec::new());
    let old = Pictured { icon: Bytes::pin(icon), banner: Bytes::pin(banner) };

    let read = moved::<Iconed, _>(&old, 1024);
    drop(old);
    let kept = pins::close();
    let read_icon = read.as_ref().ok().and_then(|form| form.icon.as_ref()).map(Bytes::id);
    pins::open(21, Vec::new());
    drop(read);
    let released = pins::close();

    assert_eq!(read_icon, Some(icon));
    assert_eq!((net(&kept, icon), net(&kept, banner)), (1, 0));
    assert_eq!(net(&released, icon), -1);
}
