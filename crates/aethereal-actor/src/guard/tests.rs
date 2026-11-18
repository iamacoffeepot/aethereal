//! The guards' tests: which senders the library's guards that compare seats admit, and where dispatch hands what a
//! guard refuses.

use super::{Itself, Parent};
use crate::{
    Above, Actor, Asked, Beginning, Chain, ChildOf, Declared, Due, Effect, Ending, Fault, Handles, Instances, Item,
    Keeps, Level, Mail, MailId, Runs, SeatNumber, Seated, Table, Tie, Top, Turn, Turning, Unreadable, What,
};
use aethereal_data::{ActorId, ActorPath, KindId, PathSegment, Plain};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// A table that holds the actors listed, each of the type its declaration names.
struct Typed(Vec<(Seated, &'static Declared)>);

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
        self.declared(id).is_some()
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

    #[cfg(feature = "std")]
    fn head(&self, _: &aethereal_data::KindName) -> Option<crate::Bytes> {
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
        self.0.iter().find(|(listed, _)| listed.id == id).map(|(_, declared)| *declared)
    }
}

/// Mail a door takes only from itself.
#[derive(Plain)]
#[kind(name = "aether.test.knock")]
struct Knock;

/// Mail a door takes only from the hall it stands in.
#[derive(Plain)]
#[kind(name = "aether.test.shut")]
struct Shut;

/// The parent of doors, which handles nothing.
#[derive(Plain)]
#[kind(name = "aether.test.hall")]
struct Hall;

impl ChildOf for Hall {
    type Parent = Top;
}

impl Actor for Hall {
    type Config = Self;
    type Key = u64;
    type Handles = ();
    type Depends = ();
    type Spawns = ();

    fn begin(hall: Self, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        Ok(hall)
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

/// A door in a hall, which logs each piece of mail its handlers take.
#[derive(Plain)]
#[kind(name = "aether.test.door")]
struct Door;

impl ChildOf for Door {
    type Parent = Hall;
}

impl Actor for Door {
    type Config = Self;
    type Key = u64;
    type Handles = (Knock, Shut);
    type Depends = ();
    type Spawns = ();

    fn begin(door: Self, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        Ok(door)
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Knock> for Door {
    type From = Itself;
    type Returns = ();

    fn handle(&mut self, _: Knock, turn: &mut Turn<'_, Self, Itself>) -> Result<(), Fault> {
        turn.log(Level::Info, "knocked");

        Ok(())
    }
}

impl Handles<Shut> for Door {
    type From = Parent;
    type Returns = ();

    fn handle(&mut self, _: Shut, turn: &mut Turn<'_, Self, Parent>) -> Result<(), Fault> {
        turn.log(Level::Info, "shut");

        Ok(())
    }
}

static PATH: ActorPath = ActorPath::new("aether.test.door");
const HALL: Seated = Seated { id: ActorId::new(1), seat: SeatNumber::new(0) };
/// Another hall, beside the door's own.
const OTHER_HALL: Seated = Seated { id: ActorId::new(2), seat: SeatNumber::new(1) };
const DOOR: Seated = Seated { id: ActorId::new(3), seat: SeatNumber::new(2) };
/// Another door in the same hall: a sibling of the door's type.
const SIBLING: Seated = Seated { id: ActorId::new(4), seat: SeatNumber::new(3) };

/// What a door was handed: the lines it logged, and the effects its turn asked for.
struct Handed {
    lines: Vec<String>,
    effects: Vec<Effect>,
}

impl Handed {
    /// Whether the turn asked the engine to treat its mail as refused: to answer a request `Unreadable`, and otherwise
    /// drop it.
    fn was_refused(&self) -> bool {
        self.effects.iter().any(|effect| *effect.what() == What::Unread(Unreadable::Refused))
    }
}

/// What the door that stands beneath `HALL` did with mail of `kind` from `from`, with every actor here in the table.
fn handed(kind: KindId, from: Seated) -> Handed {
    let hall = Declared::of::<Hall>();
    let door = Declared::of::<Door>();
    let table = Typed(vec![(HALL, hall), (OTHER_HALL, hall), (DOOR, door), (SIBLING, door)]);
    let mut asked = Asked::new();
    let mut actor: Box<dyn Runs> = Box::new(Door);
    let mut instances = Instances::default();
    let chain = Chain { root: MailId { sender: from.id, count: 1 }, depth: 1 };
    let above = Above::Actor(HALL, hall);
    let turning = Turning::new(
        DOOR,
        &PATH,
        above,
        &table,
        &(),
        &mut asked,
        &mut instances,
        Keeps::Claimed,
        0,
        from,
        chain,
        Tie::TELL,
        Due::Never,
    );
    let mail = Mail::new(kind, &[], &crate::PinSource::NOTHING, from, chain.root, 1, 0);
    let turned = actor.turn(Item::Mail(mail), turning);
    assert!(turned.is_ok(), "a refusal does not fail the turn");

    let lines = asked.lines().iter().map(|line| String::from(asked.text(line))).collect();

    Handed { lines, effects: asked.effects().to_vec() }
}

#[test]
fn itself_admits_the_receivers_own_seat_and_refuses_a_sibling_of_its_type() {
    // Catches `Itself` compared by type alone, which admits a sibling in another seat, and a refused sender handed to
    // the handler, or dropped without the refusal the engine answers a request `Unreadable` for.
    let own = handed(KindId::of::<Knock>(), DOOR);
    let sibling = handed(KindId::of::<Knock>(), SIBLING);

    assert_eq!(own.lines, ["knocked"]);
    assert!(!own.was_refused());
    assert!(sibling.was_refused());
    assert!(sibling.lines.iter().all(|line| line.starts_with("refused aether.test.knock")), "{:?}", sibling.lines);
}

#[test]
fn parent_admits_the_actor_above_by_seat_and_refuses_another_of_its_type() {
    // Catches `Parent` compared by type alone, which admits any hall where the door stands in one.
    let own = handed(KindId::of::<Shut>(), HALL);
    let other = handed(KindId::of::<Shut>(), OTHER_HALL);

    assert_eq!(own.lines, ["shut"]);
    assert!(!own.was_refused());
    assert!(other.was_refused());
}
