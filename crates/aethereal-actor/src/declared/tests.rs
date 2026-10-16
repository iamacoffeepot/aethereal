//! The declarations' tests: which handler an item reaches, what a declaration lets a turn do, and what the engine
//! reads of it.

use super::*;
use crate::Effect;
use crate::{
    Above, Asked, Beginning, Chain, Ending, ErasedActorRef, Fault, Item, Level, Mail, MailId, Runs, SeatNumber, Seated,
    Table, To, Turn, Turning, What,
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

    fn path(&self, _: ActorId) -> Option<ActorPath> {
        None
    }

    fn child(&self, parent: ActorId, _: &PathSegment) -> ActorId {
        parent
    }

    fn is_live(&self, id: ActorId) -> bool {
        self.seat(id).is_some()
    }

    fn child_seat(&self, _: ActorId) -> Option<SeatNumber> {
        None
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

/// An actor that handles kept mail, sends it on to keepers and to any actor that handles it, and falls back on the
/// rest, logging which handler it reached.
#[derive(Plain)]
struct Keeper;

impl Kind for Keeper {
    const NAME: KindName = KindName::new("aether.test.keeper");
}

impl Actor for Keeper {
    type Config = Kept;
    type Key = ();
    type Handles = (Kept, Others);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Kept, _: &mut Beginning<'_, ()>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Kept> for Keeper {
    type Sends = (ToType<Self, Kept>, ToAny<(Kept,), Kept>);

    fn handle(&mut self, _: Kept, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        turn.log(Level::Info, "kept");

        Ok(())
    }
}

impl Handles<Others> for Keeper {
    type Sends = ();

    fn handle(&mut self, _: Mail<'_>, turn: &mut Turn<'_, ()>) -> Result<(), Fault> {
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

impl Actor for Strict {
    type Config = Kept;
    type Key = ();
    type Handles = (Kept,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Kept, _: &mut Beginning<'_, ()>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Kept> for Strict {
    type Sends = ();

    fn handle(&mut self, _: Kept, _: &mut Turn<'_, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// An actor that takes any mail through its fallback, and lists no kind.
#[derive(Plain)]
struct Sink;

impl Kind for Sink {
    const NAME: KindName = KindName::new("aether.test.sink");
}

impl Actor for Sink {
    type Config = Kept;
    type Key = ();
    type Handles = (Others,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Kept, _: &mut Beginning<'_, ()>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Others> for Sink {
    type Sends = ();

    fn handle(&mut self, _: Mail<'_>, _: &mut Turn<'_, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

static PATH: ActorPath = ActorPath::new("aether.test.keeper");
const ME: Seated = Seated { id: ActorId::new(1), seat: SeatNumber::new(0) };
const OTHER: Seated = Seated { id: ActorId::new(2), seat: SeatNumber::new(1) };
const CHAIN: Chain = Chain { root: MailId { sender: OTHER.id, count: 1 }, depth: 1 };

/// What an actor of type `A` asked for when it was handed mail of `kind` from `OTHER`.
fn handed<A: Actor>(actor: A, kind: KindId) -> Vec<What> {
    let table = Typed(Vec::new());
    let mut asked = Asked::new();
    let mut actor: Box<dyn Runs> = Box::new(actor);
    let turning = Turning::new(ME, &PATH, Above::Chassis, &table, &mut asked, 0, OTHER, CHAIN);
    let mail = Mail::new(kind, &[], OTHER, CHAIN.root, 1, 0);
    let handled = actor.handle(Item::Mail(mail), turning);
    assert!(handled.is_ok());

    asked.into_parts().0.into_iter().map(Effect::into_what).collect()
}

/// What a keeper's handler of kept mail asks for when it runs `then` with the actors of `table`.
fn keeping(table: &Typed, then: impl FnOnce(&mut Turn<'_, <Keeper as Handles<Kept>>::Sends>)) -> Vec<What> {
    let mut asked = Asked::new();
    let kept = Role::Kind(KindId::of::<Kept>());
    let turning = Turning::new(ME, &PATH, Above::Chassis, table, &mut asked, 0, OTHER, CHAIN);
    then(&mut turning.declaring::<Keeper, <Keeper as Handles<Kept>>::Sends>(kept));

    asked.into_parts().0.into_iter().map(Effect::into_what).collect()
}

/// The log line an effect asks for.
fn line(text: &str) -> What {
    What::Log { level: Level::Info, text: String::from(text) }
}

#[test]
fn mail_reaches_the_handler_of_its_kind_and_the_fallback_takes_the_rest() {
    // Catches a fallback that shadows a kind the actor lists, and an unlisted kind not handed to the fallback.
    assert_eq!(handed(Keeper, KindId::of::<Kept>()), [line("kept")]);
    assert_eq!(handed(Keeper, KindId::of::<Stray>()), [line("other")]);
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
        assert!(turn.to::<Keeper>(To::Held(ErasedActorRef::new(OTHER))).send(&Kept).is_ok());
    });

    assert!(matches!(asked[..], [What::Send { to: OTHER, .. }]));
}

#[test]
#[should_panic(expected = "false declaration")]
fn send_to_an_actor_of_another_type_stops_the_engine() {
    // Catches a reference to an actor of one type sent mail its declaration gives to another.
    let table = Typed(vec![(OTHER, Declared::of::<Strict>())]);
    keeping(&table, |turn| {
        let _ = turn.to::<Keeper>(To::Held(ErasedActorRef::new(OTHER))).send(&Kept);
    });
}

#[test]
fn send_to_any_reaches_a_type_that_lists_every_kind() {
    // Catches a protocol send refused to a type that lists its kinds.
    let table = Typed(vec![(OTHER, Declared::of::<Strict>())]);
    let asked = keeping(&table, |turn| {
        assert!(turn.to_any::<(Kept,)>(To::Held(ErasedActorRef::new(OTHER))).send(&Kept).is_ok());
    });

    assert!(matches!(asked[..], [What::Send { to: OTHER, .. }]));
}

#[test]
#[should_panic(expected = "false declaration")]
fn send_to_any_by_a_type_that_only_falls_back_stops_the_engine() {
    // Catches a fallback counted as handling a protocol's kinds.
    let table = Typed(vec![(OTHER, Declared::of::<Sink>())]);
    keeping(&table, |turn| {
        let _ = turn.to_any::<(Kept,)>(To::Held(ErasedActorRef::new(OTHER))).send(&Kept);
    });
}

#[test]
#[should_panic(expected = "does not handle endings")]
fn watch_by_a_type_that_does_not_handle_endings_stops_the_engine() {
    // Catches a watch whose ending could never be handed to its watcher.
    let table = Typed(vec![(OTHER, Declared::of::<Strict>())]);
    keeping(&table, |turn| turn.watch(ErasedActorRef::new(OTHER)));
}

#[test]
fn declaration_lists_each_handler_with_the_sends_its_impl_names() {
    // Catches a handler's sends listed under another handler, or one dropped from the tuple the engine reads.
    const KEPT: KindId = KindId::of::<Kept>();
    const KEEPER: &KindName = &Keeper::NAME;
    const SENDS: &[Sending] =
        &[Sending::new(KEPT, Target::Type(KEEPER)), Sending::new(KEPT, Target::Handling(&[KEPT]))];
    let keeper = Declared::of::<Keeper>();
    let handlers = [Handler::new(Role::Kind(KEPT), SENDS), Handler::new(Role::Others, &[])];

    assert_eq!(keeper.name(), &Keeper::NAME);
    assert_eq!(keeper.handlers(), handlers);
    assert!(keeper.takes(KindId::of::<Stray>()));
    assert!(!keeper.lists(KindId::of::<Stray>()));
    assert!(!Declared::of::<Strict>().handles_endings());
}
