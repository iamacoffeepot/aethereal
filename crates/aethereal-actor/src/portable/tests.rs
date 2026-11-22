use super::*;
use crate::{BlobId, Bytes, ErasedActorRef, Portability, ProtocolRef, SeatNumber, Seated, pins};
use aethereal_data::{ActorId, Bound, Encode, Plain};
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

/// Mail a screen's model is poked with.
#[derive(Plain)]
#[kind(name = "aether.test.poke")]
struct Poke;

/// Mail its model is shut with, which the model's next version takes as well.
#[derive(Plain)]
#[kind(name = "aether.test.shut")]
struct Shut;

/// The model a screen's first version refers to, which takes pokes.
type ModelOne = (Poke,);

/// The model its next version refers to, whose kinds changed: it takes shuts too.
type Model = (Poke, Shut);

/// A watch held inside a screen's first version.
#[derive(Bound)]
struct WatchOne {
    target: ProtocolRef<ModelOne>,
    since: u32,
}

/// The watch as the next version holds it.
#[derive(Bound, Debug, PartialEq)]
struct Watch {
    target: ProtocolRef<Model>,
    since: u32,
}

/// The watch as a version that also widened `since` holds it.
#[derive(Bound)]
struct WatchWide {
    target: ProtocolRef<ModelOne>,
    since: u64,
}

/// A screen's state as its first version wrote it: references to its model, bare and nested.
#[derive(Bound)]
struct ScreenOne {
    model: ProtocolRef<ModelOne>,
    watchers: Vec<ProtocolRef<ModelOne>>,
    watch: WatchOne,
}

/// Its next version, which keeps every field while the model's kinds change.
#[derive(Bound, Portability)]
struct Screen {
    model: ProtocolRef<Model>,
    watchers: Vec<ProtocolRef<Model>>,
    watch: Watch,
}

/// Its watch, as a version that changed `since` beside the model's kinds wrote it.
#[derive(Bound)]
struct WidenedScreen {
    watch: WatchWide,
}

/// Its model, as a version that held it with no protocol wrote it.
#[derive(Bound)]
struct ErasedScreen {
    model: ErasedActorRef,
}

/// The actor at `id`, in a seat of the same number.
const fn seated(id: u64) -> Seated {
    Seated { id: ActorId::new(id), seat: SeatNumber::new(id) }
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

#[test]
fn a_field_that_differs_only_in_its_references_protocols_is_read() {
    // Catches a field refused because the protocol its reference names changed with its kinds, though its bytes did
    // not: bare, inside a sequence, and inside a struct, so a comparison that forgets the protocol only at the top
    // still refuses the nested ones.
    let old = ScreenOne {
        model: ProtocolRef::found(seated(1)),
        watchers: Vec::from([ProtocolRef::found(seated(2)), ProtocolRef::found(seated(3))]),
        watch: WatchOne { target: ProtocolRef::found(seated(4)), since: 9 },
    };

    let Ok(screen) = moved::<Screen, _>(&old, 1024) else {
        panic!("the state moves");
    };

    assert_eq!(screen.model, Some(ProtocolRef::found(seated(1))));
    assert_eq!(screen.watchers, Some(Vec::from([ProtocolRef::found(seated(2)), ProtocolRef::found(seated(3))])));
    assert_eq!(screen.watch, Some(Watch { target: ProtocolRef::found(seated(4)), since: 9 }));
}

#[test]
fn a_field_that_differs_in_more_than_its_references_protocols_refuses_the_move() {
    // Catches a comparison that stops looking once it meets a reference, so a field whose protocol and another part
    // both changed is read as the new type; and one that sees through the reference to the bytes it is written as, so
    // a reference no protocol was proven for is read as one proven for the model.
    let widened = moved::<Screen, _>(
        &WidenedScreen { watch: WatchWide { target: ProtocolRef::found(seated(4)), since: 9 } },
        1024,
    );
    let erased = moved::<Screen, _>(&ErasedScreen { model: ProtocolRef::<ModelOne>::found(seated(1)).erase() }, 1024);

    assert_eq!(widened.err(), Some(TakeOverError::Mismatch { field: "watch" }));
    assert_eq!(erased.err(), Some(TakeOverError::Mismatch { field: "model" }));
}
