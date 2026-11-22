//! The declarations' tests: which handler an item reaches, what a declaration lets a turn do, and what the engine
//! reads of it.

use super::*;
use crate::{
    Above, Anyone, Anywhere, Asked, Chain, ChildOf, Due, ErasedActorRef, Fault, Instances, Item, Keeps, Level, Mail,
    MailId, Mailed, Recursive, Runs, SeatNumber, Seated, SenderType, SentAs, Spawner, Table, Tie, Turn, Turning, actor,
    section,
};
use aethereal_data::{ActorId, ActorPath, Keyed, Kind, PathSegment, Plain};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// A table that holds the actors listed, each of the type its declaration names.
struct Typed(Vec<(Seated, &'static Declared)>);

impl Typed {
    /// The seat of the actor listed under `id`.
    fn seat(&self, id: ActorId) -> Option<SeatNumber> {
        self.0.iter().find(|(made, _)| made.id == id).map(|(made, _)| made.seat)
    }
}

impl Table for Typed {
    fn resolve(&self, _: &ActorPath) -> Option<Seated> {
        None
    }

    fn reach(&self, _: &ActorPath, _: KindId) -> Seated {
        Seated { id: ActorId::new(0), seat: SeatNumber::NOWHERE }
    }

    fn reach_child(&self, _: ActorId, _: &PathSegment, _: KindId) -> Option<Seated> {
        None
    }

    fn child(&self, parent: ActorId, _: &PathSegment) -> ActorId {
        parent
    }

    fn is_live(&self, id: ActorId) -> bool {
        self.seat(id).is_some()
    }

    fn held(&self, id: ActorId) -> bool {
        self.is_live(id)
    }

    fn child_seat(&self, _: ActorId) -> Option<SeatNumber> {
        None
    }

    fn blob(&self, _: crate::BlobId) -> Option<&[u8]> {
        None
    }

    fn blob_hash(&self, _: crate::BlobId) -> Option<crate::BlobHash> {
        None
    }

    /// Nothing is published to a table of typed actors.
    #[cfg(feature = "std")]
    fn head(&self, _: &KindName) -> Option<crate::Bytes> {
        None
    }

    #[cfg(feature = "std")]
    fn published(&self) -> u64 {
        0
    }

    fn now(&self) -> crate::Time {
        crate::Time::ZERO
    }

    fn runs_stage(&self, _: KindId) -> bool {
        true
    }

    fn declared(&self, id: ActorId) -> Option<&'static Declared> {
        self.0.iter().find(|(made, _)| made.id == id).map(|(_, declared)| *declared)
    }
}

/// Mail a keeper handles.
#[derive(Plain)]
#[kind(name = "aether.test.kept")]
struct Kept;

/// A request a keeper answers with a weight.
#[derive(Plain)]
#[kind(name = "aether.test.weigh")]
struct Weigh;

/// A keeper's answer to a weighing.
#[derive(Plain)]
#[kind(name = "aether.test.weight")]
struct Weight;

/// Mail no actor here lists.
#[derive(Plain)]
#[kind(name = "aether.test.stray")]
struct Stray;

/// An actor that handles kept mail, answers a weighing, and falls back on the rest, logging which handler it reached,
/// and depends on the shelf.
#[derive(Plain)]
#[kind(name = "aether.test.keeper")]
struct Keeper;

#[actor(parent = Anywhere, depends(Shelf))]
impl Keeper {
    fn begin(_: Kept) -> Result<Self, Fault> {
        Ok(Self)
    }

    #[tell]
    fn kept(&mut self, _: Kept, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        turn.log(Level::Info, "kept");

        Ok(())
    }

    #[request]
    fn weigh(&mut self, _: Weigh) -> Result<Weight, Fault> {
        Ok(Weight)
    }

    #[others]
    fn others(&mut self, _: Mail<'_>, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        turn.log(Level::Info, "other");

        Ok(())
    }
}

/// An actor that handles kept mail and nothing else, and falls back on nothing.
#[derive(Plain)]
#[kind(name = "aether.test.strict")]
struct Strict;

#[actor(parent = Keeper)]
impl Strict {
    fn begin(_: Kept) -> Result<Self, Fault> {
        Ok(Self)
    }

    #[tell]
    fn kept(&mut self, _: Kept) -> Result<(), Fault> {
        Ok(())
    }
}

/// A root a keeper depends on, which handles nothing.
#[derive(Plain)]
#[kind(name = "aether.test.shelf")]
struct Shelf;

#[actor]
impl Shelf {
    fn begin(_: Kept) -> Result<Self, Fault> {
        Ok(Self)
    }
}

/// A keyed place beneath the shelf, which handles nothing.
#[derive(Plain)]
#[kind(name = "aether.test.slot")]
struct Slot;

#[actor(key = u64, parent = Shelf)]
impl Slot {
    fn begin(_: Kept) -> Result<Self, Fault> {
        Ok(Self)
    }
}

/// An actor that takes any mail through its fallback, and lists no kind.
#[derive(Plain)]
#[kind(name = "aether.test.sink")]
struct Sink;

#[actor(key = u64, parent = Recursive)]
impl Sink {
    fn begin(_: Kept) -> Result<Self, Fault> {
        Ok(Self)
    }

    #[others]
    fn others(&mut self, _: Mail<'_>) -> Result<(), Fault> {
        Ok(())
    }
}

static PATH: ActorPath = ActorPath::new("aether.test.keeper");
const ME: Seated = Seated { id: ActorId::new(1), seat: SeatNumber::new(0) };
const OTHER: Seated = Seated { id: ActorId::new(2), seat: SeatNumber::new(1) };
const CHAIN: Chain = Chain { root: MailId { sender: OTHER.id, count: 1 }, depth: 1 };

/// The lines an actor of type `A` logged when it was handed mail of `kind` from `OTHER`.
fn handed<A: ChildOf>(actor: A, kind: KindId) -> Vec<String> {
    let table = Typed(Vec::new());
    let mut asked = Asked::new();
    let mut actor: Box<dyn Runs> = Box::new(actor);
    let mut instances = Instances::default();
    let turning = Turning::new(
        ME,
        &PATH,
        Above::Chassis,
        &table,
        &(),
        &mut asked,
        &mut instances,
        Keeps::Claimed,
        0,
        OTHER,
        CHAIN,
        Tie::TELL,
        Due::Never,
    );
    // A keeper takes its mail from anyone, so the type its sender sent it as is never read.
    let sent_as = SenderType::Stamped(SentAs::Engine);
    let mail = Mail::new(kind, &[], &crate::PinSource::NOTHING, OTHER, sent_as, CHAIN, 0);
    let handled = actor.turn(Item::Mail(mail), turning);
    assert!(handled.is_ok());

    asked.lines().iter().map(|line| String::from(asked.text(line))).collect()
}

/// The mail a keeper's handler of kept mail sends when it runs `then` with the actors of `table`.
fn keeping(table: &Typed, then: impl FnOnce(&mut Turn<'_, Keeper>)) -> Vec<Mailed> {
    let mut asked = Asked::new();
    let mut instances = Instances::default();
    let turning = Turning::new(
        ME,
        &PATH,
        Above::Chassis,
        table,
        &(),
        &mut asked,
        &mut instances,
        Keeps::Claimed,
        0,
        OTHER,
        CHAIN,
        Tie::TELL,
        Due::Never,
    );
    let sender = ErasedActorRef::new(OTHER);
    then(&mut turning.declaring::<Keeper, Anyone>(Declared::of::<Keeper>(), sender));

    asked.into_parts().0.mailed
}

#[test]
fn mail_reaches_the_handler_of_its_kind_and_the_fallback_takes_the_rest() {
    // Catches a fallback that shadows a kind the actor lists, and an unlisted kind not handed to the fallback.
    assert_eq!(handed(Keeper, KindId::of::<Kept>()), ["kept"]);
    assert_eq!(handed(Keeper, KindId::of::<Stray>()), ["other"]);
}

#[test]
#[should_panic(expected = "false declaration")]
fn mail_no_handler_takes_stops_the_engine() {
    // Catches mail of a kind its receiver neither lists nor falls back on, handed to some handler anyway or dropped.
    handed(Strict, KindId::of::<Stray>());
}

#[test]
fn a_cast_finds_a_type_that_lists_every_kind_and_not_one_that_only_falls_back() {
    // Catches a fallback counted as handling a protocol's kinds, and a type that lists them refused.
    let cast = |declared| {
        let table = Typed(vec![(OTHER, declared)]);
        let mut found = None;
        keeping(&table, |turn| found = Some(turn.cast::<(Kept,)>(ErasedActorRef::new(OTHER)).is_some()));

        found
    };

    assert_eq!(cast(Declared::of::<Strict>()), Some(true));
    assert_eq!(cast(Declared::of::<Sink>()), Some(false));
}

#[test]
#[should_panic(expected = "two types of one root")]
fn a_declaration_that_depends_on_two_types_of_one_root_does_not_hold() {
    // Catches a type built that depends on the shelf and a slot beneath it, where a typed path beneath the shelf could
    // be found at either entry, so a send there would not be inferred (ADR-0059).
    const DEPENDS: &[&TypeSegment] = &[Last::<Shelf>::SEGMENT, Last::<Slot>::SEGMENT];
    let twice = Declared::new(
        Keeper::NAME,
        KindId::of::<Keeper>(),
        StateSchema::of::<Keeper>(),
        KindId::of::<Kept>(),
        Keyed::Singleton,
        Spawner::Anyone,
        DEPENDS,
        &[],
        &[],
    );

    let _ = twice.checked();
}

#[test]
fn declaration_lists_each_handler_its_answers_its_config_and_the_path_to_each_type_it_depends_on() {
    // Catches a handler dropped from the tuple the engine reads, a request's answer left out or a tell given one, the
    // type read as its own config, and a type's depends read as another's or not at all, which the spawn check, a spawn
    // by name, a cast to a named protocol, and a guest's host read (ADR-0016, ADR-0022, ADR-0059).
    const KEPT: KindId = KindId::of::<Kept>();
    const WEIGHT: &[KindId] = &[KindId::of::<Weight>()];
    let keeper = Declared::of::<Keeper>();
    let weigh = Handler::answering(KindId::of::<Weigh>(), WEIGHT);
    let handlers = [Handler::new(Role::Kind(KEPT)), weigh, Handler::new(Role::Others)];

    assert_eq!(keeper.name(), &Keeper::NAME);
    assert_eq!(keeper.config(), KEPT);
    assert_eq!(keeper.handlers(), handlers);
    assert_eq!(keeper.depends(), [Last::<Shelf>::SEGMENT]);
    assert!(keeper.takes(KindId::of::<Stray>()));
    assert!(!keeper.lists(KindId::of::<Stray>()));
    assert!(!Declared::of::<Strict>().handles_endings());
}

#[test]
fn the_spawn_check_finds_a_dependency_of_singletons_not_live_and_passes_a_keyed_one() {
    // Catches the spawn check reading a keyed place, which actors fill and empty as the program runs, as missing, and
    // one that misses the shelf a keeper depends on (ADR-0022, ADR-0059).
    const SLOTTED: &[&TypeSegment] = &[Last::<Slot>::SEGMENT];
    let slotted = Declared::new(
        Keeper::NAME,
        KindId::of::<Keeper>(),
        StateSchema::of::<Keeper>(),
        KindId::of::<Kept>(),
        Keyed::Singleton,
        Spawner::Anyone,
        SLOTTED,
        &[],
        &[],
    );
    let nothing_live = |_: &ActorPath, _: KindId| false;

    assert_eq!(Declared::of::<Keeper>().missing(nothing_live), Some(Last::<Shelf>::SEGMENT));
    assert_eq!(Declared::of::<Keeper>().missing(|_, _| true), None);
    assert_eq!(slotted.missing(nothing_live), None);
}

#[test]
fn a_section_written_in_a_constant_reads_back_as_what_its_types_declare() {
    // Catches the constant writer of a guest's section and the plain types a host reads it as drifting apart: a field
    // written in another order, or a selector numbered otherwise than the enum it stands for (a singleton read as keyed,
    // or one type's spawner read as another's), so a host reads a declaration other than the one the guest was built
    // with (ADR-0015, ADR-0022); a type's config kind or a handler's answer dropped or read as another field (ADR-0059);
    // or a state's schema read as another type's, or not kept (ADR-0054).
    const TYPES: &[&Declared] =
        &[Declared::of::<Keeper>(), Declared::of::<Strict>(), Declared::of::<Sink>(), Declared::of::<Slot>()];
    const ROOM: usize = section::room(TYPES);
    const LEN: usize = section::len::<ROOM>(TYPES);
    const BYTES: [u8; LEN] = section::bytes::<LEN, ROOM>(TYPES);

    let mut budget = aethereal_data::Budget::new(1 << 16);
    let Ok(made) = aethereal_data::from_bytes::<Vec<section::Made>>(&BYTES, &mut budget) else {
        panic!("a section reads back");
    };

    let leaked: Vec<&Declared> = made.iter().map(section::Made::leak).collect();
    let states = |types: &[&Declared]| types.iter().map(|declared| declared.state().nodes()).collect::<Vec<_>>();
    assert_eq!(leaked, TYPES);
    assert_eq!(states(&leaked), states(TYPES));
}
