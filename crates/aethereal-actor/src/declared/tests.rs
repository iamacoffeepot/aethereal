//! The declarations' tests: which handler an item reaches, what a declaration lets a turn do, and what the engine
//! reads of it.

use super::*;
use crate::{
    Above, Actor, Anywhere, Asked, Beginning, Chain, ChildOf, Ending, ErasedActorRef, Fault, Instances, Item, Keeps,
    Level, Mail, MailId, Mailed, Recursive, Runs, SeatNumber, Seated, Spawner, Table, Tie, To, Turn, Turning, section,
};
use aethereal_data::{ActorId, ActorPath, Kind, PathSegment, Plain};
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
struct Kept;

impl Kind for Kept {
    const NAME: KindName = KindName::new("aether.test.kept");
}

/// Mail no actor here lists.
#[derive(Plain)]
struct Stray;

impl Kind for Stray {
    const NAME: KindName = KindName::new("aether.test.stray");
}

/// An actor that handles kept mail, sends it on to keepers, to any actor that handles it, and back to its sender, and
/// falls back on the rest, logging which handler it reached.
#[derive(Plain)]
struct Keeper;

impl Kind for Keeper {
    const NAME: KindName = KindName::new("aether.test.keeper");
}

impl ChildOf for Keeper {
    type Parent = Anywhere;
}

impl Actor for Keeper {
    type Config = Kept;
    type Key = ();
    type Handles = (Kept, Others);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Kept, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Kept> for Keeper {
    type Sends = (ToType<Self, Kept>, ToAny<(Kept,), Kept>, ToSender<Kept>);

    fn handle(&mut self, _: Kept, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.log(Level::Info, "kept");

        Ok(())
    }
}

impl Handles<Others> for Keeper {
    type Sends = ();

    fn handle(&mut self, _: Mail<'_>, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        turn.log(Level::Info, "other");

        Ok(())
    }
}

/// An actor that handles kept mail and nothing else, and falls back on nothing.
#[derive(Plain)]
struct Strict;

impl Kind for Strict {
    const NAME: KindName = KindName::new("aether.test.strict");
}

impl ChildOf for Strict {
    type Parent = Keeper;
}

impl Actor for Strict {
    type Config = Kept;
    type Key = ();
    type Handles = (Kept,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Kept, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Kept> for Strict {
    type Sends = ();

    fn handle(&mut self, _: Kept, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// An actor that takes any mail through its fallback, and lists no kind.
#[derive(Plain)]
struct Sink;

impl Kind for Sink {
    const NAME: KindName = KindName::new("aether.test.sink");
}

impl ChildOf for Sink {
    type Parent = Recursive;
}

impl Actor for Sink {
    type Config = Kept;
    type Key = u64;
    type Handles = (Others,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Kept, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Others> for Sink {
    type Sends = ();

    fn handle(&mut self, _: Mail<'_>, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
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
    );
    let mail = Mail::new(kind, &[], &crate::PinSource::NOTHING, OTHER, CHAIN.root, 1, 0);
    let handled = actor.turn(Item::Mail(mail), turning);
    assert!(handled.is_ok());

    asked.lines().iter().map(|line| String::from(asked.text(line))).collect()
}

/// The mail a keeper's handler of kept mail sends when it runs `then` with the actors of `table`.
fn keeping(table: &Typed, then: impl FnOnce(&mut Turn<'_, Keeper, <Keeper as Handles<Kept>>::Sends>)) -> Vec<Mailed> {
    let mut asked = Asked::new();
    let kept = Role::Kind(KindId::of::<Kept>());
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
    );
    then(&mut turning.declaring::<Keeper, <Keeper as Handles<Kept>>::Sends>(kept, Declared::of::<Keeper>()));

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
fn send_to_a_type_reaches_an_actor_of_that_type() {
    // Catches a send to the type its handler declares refused, or its effect lost.
    let table = Typed(vec![(OTHER, Declared::of::<Keeper>())]);
    let asked = keeping(&table, |turn| {
        assert!(turn.send(To::Held(ErasedActorRef::new(OTHER)), &Kept).is_ok());
    });

    assert!(matches!(asked[..], [Mailed { to: OTHER, .. }]));
}

#[test]
#[should_panic(expected = "false declaration")]
fn send_to_an_actor_of_another_type_stops_the_engine() {
    // Catches a reference to an actor of one type sent mail its declaration gives to another.
    let table = Typed(vec![(OTHER, Declared::of::<Strict>())]);
    keeping(&table, |turn| {
        let _ = turn.send(To::Held(ErasedActorRef::new(OTHER)), &Kept);
    });
}

#[test]
#[should_panic(expected = "false declaration")]
fn send_like_a_reply_before_it_is_checked_all_the_same() {
    // Catches a send skipping its check because the turn's last send, a reply that is never checked, went to the same
    // actor with the same kind: the two would join one run, and the send would reach a type it does not declare.
    let table = Typed(vec![(OTHER, Declared::of::<Strict>())]);
    keeping(&table, |turn| {
        assert!(turn.reply(&Kept).is_ok());
        let _ = turn.send(To::Held(ErasedActorRef::new(OTHER)), &Kept);
    });
}

#[test]
fn send_to_any_reaches_a_type_that_lists_every_kind() {
    // Catches a protocol send refused to a type that lists its kinds.
    let table = Typed(vec![(OTHER, Declared::of::<Strict>())]);
    let asked = keeping(&table, |turn| {
        assert!(turn.send_to_any(To::Held(ErasedActorRef::new(OTHER)), &Kept).is_ok());
    });

    assert!(matches!(asked[..], [Mailed { to: OTHER, .. }]));
}

#[test]
#[should_panic(expected = "false declaration")]
fn send_to_any_by_a_type_that_only_falls_back_stops_the_engine() {
    // Catches a fallback counted as handling a protocol's kinds.
    let table = Typed(vec![(OTHER, Declared::of::<Sink>())]);
    keeping(&table, |turn| {
        let _ = turn.send_to_any(To::Held(ErasedActorRef::new(OTHER)), &Kept);
    });
}

/// A send of a stray to any actor that takes kept mail: of a kind its protocol does not hold.
const STRAY_TO_KEEPERS: &[Sending] = &[<ToAny<(Kept,), Stray> as Marker>::SENDING];

#[test]
#[should_panic(expected = "the protocol does not hold")]
fn a_declaration_whose_handler_sends_a_protocol_a_kind_outside_it_does_not_hold() {
    // Catches a type built that declares, in a handler, a send of a kind to actors that need not take it.
    const HANDLERS: &[Handler] = &[Handler::new(Role::Kind(KindId::of::<Kept>()), STRAY_TO_KEEPERS)];
    let sending =
        Declared::new(Keeper::NAME, KindId::of::<Keeper>(), Keyed::Singleton, Spawner::Anyone, &[], HANDLERS, &[]);

    let _ = sending.checked();
}

#[test]
#[should_panic(expected = "the protocol does not hold")]
fn a_declaration_whose_begin_sends_a_protocol_a_kind_outside_it_does_not_hold() {
    // Catches the check of a protocol's kinds left out of a type's `begin`.
    let beginning = Declared::new(
        Keeper::NAME,
        KindId::of::<Keeper>(),
        Keyed::Singleton,
        Spawner::Anyone,
        STRAY_TO_KEEPERS,
        &[],
        &[],
    );

    let _ = beginning.checked();
}

#[test]
#[should_panic(expected = "the protocol does not hold")]
fn a_declaration_whose_closing_turn_sends_a_protocol_a_kind_outside_it_does_not_hold() {
    // Catches the check of a protocol's kinds left out of a type's closing turn.
    let ending = Declared::new(
        Keeper::NAME,
        KindId::of::<Keeper>(),
        Keyed::Singleton,
        Spawner::Anyone,
        &[],
        &[],
        STRAY_TO_KEEPERS,
    );

    let _ = ending.checked();
}

#[test]
fn declaration_lists_each_handler_with_the_sends_its_impl_names() {
    // Catches a handler's sends listed under another handler, or one dropped from the tuple the engine reads.
    const KEPT: KindId = KindId::of::<Kept>();
    const KEEPER: &KindName = &Keeper::NAME;
    const SENDS: &[Sending] = &[
        Sending::new(KEPT, Target::Type(KEEPER, KindId::of::<Keeper>())),
        Sending::new(KEPT, Target::Handling(&[KEPT])),
        Sending::new(KEPT, Target::Sender),
    ];
    let keeper = Declared::of::<Keeper>();
    let handlers = [Handler::new(Role::Kind(KEPT), SENDS), Handler::new(Role::Others, &[])];

    assert_eq!(keeper.name(), &Keeper::NAME);
    assert_eq!(keeper.handlers(), handlers);
    assert!(keeper.takes(KindId::of::<Stray>()));
    assert!(!keeper.lists(KindId::of::<Stray>()));
    assert!(!Declared::of::<Strict>().handles_endings());
}

#[test]
fn a_section_written_in_a_constant_reads_back_as_what_its_types_declare() {
    // Catches the constant writer of a guest's section and the plain types a host reads it as drifting apart: a field
    // written in another order, or a selector numbered otherwise than the enum it stands for (a singleton read as keyed,
    // or one type's spawner read as another's), so a host reads a declaration other than the one the guest was built
    // with (ADR-0015, ADR-0022).
    const TYPES: &[&Declared] = &[Declared::of::<Keeper>(), Declared::of::<Strict>(), Declared::of::<Sink>()];
    const LEN: usize = section::len(TYPES);
    const BYTES: [u8; LEN] = section::bytes::<LEN>(TYPES);

    let mut budget = aethereal_data::Budget::new(1 << 16);
    let Ok(made) = aethereal_data::from_bytes::<Vec<section::Made>>(&BYTES, &mut budget) else {
        panic!("a section reads back");
    };

    let leaked: Vec<&Declared> = made.iter().map(section::Made::leak).collect();
    assert_eq!(leaked, TYPES);
}
