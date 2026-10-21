//! Resources as a step's turns keep, give, and take them, and as the engine drops them (ADR-0010).

use super::super::*;
use crate::Entry;
use aethereal_actor::Time;
use aethereal_actor::{Actor, ChildOf, Ending, Fault, Handle, Handles, Recursive, To, ToType, Turn};
use aethereal_data::{Bound, Kind, KindName, Plain, TypedKey};
use std::any;
use std::cell::{Cell, RefCell};
use std::mem;

thread_local! {
    /// What this thread's keepers have said they did, in order.
    static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// How many lamps have been dropped on this thread.
    static OUT: Cell<u64> = const { Cell::new(0) };
}

/// Notes that a keeper did something.
fn say(line: String) {
    SAID.with_borrow_mut(|said| said.push(line));
}

/// What has been said since this was last asked.
fn said() -> Vec<String> {
    SAID.with_borrow_mut(mem::take)
}

/// How many lamps have been dropped on this thread.
fn out() -> u64 {
    OUT.get()
}

/// A resource: a value that is not data, which counts its drops on the thread that drops it.
struct Lamp {
    light: u64,
}

impl Resource for Lamp {}

/// What the log records a dropped lamp as.
fn lamp() -> &'static str {
    any::type_name::<Lamp>()
}

impl Drop for Lamp {
    fn drop(&mut self) {
        OUT.set(OUT.get() + 1);
    }
}

/// A keeper's number, the lamps it is given, and the orders it sends itself as it begins.
#[derive(Bound)]
struct KeeperConfig {
    number: u64,
    lamps: Vec<Given<Lamp>>,
    orders: Vec<String>,
}

impl Kind for KeeperConfig {
    const NAME: KindName = KindName::new("aether.test.keeper_config");
}

/// An order a keeper sends itself.
#[derive(Plain)]
struct Order {
    text: String,
}

impl Kind for Order {
    const NAME: KindName = KindName::new("aether.test.order");
}

/// Lamps passed from one keeper to another in mail.
#[derive(Bound)]
struct Pass {
    lamps: Vec<Given<Lamp>>,
}

impl Kind for Pass {
    const NAME: KindName = KindName::new("aether.test.pass");
}

/// An actor that keeps lamps, as its orders say:
///
/// - `make L` makes a lamp of light `L` and keeps it, and `shine` says the light of each it keeps;
/// - `spawn N` gives every lamp to the keeper `N` it spawns beneath it, in its config;
/// - `pass N` mails every lamp to the keeper `N` beneath it, and `root N` to the root keeper `N`;
/// - `loose` gives every lamp and sends none, and `slip N` mails every lamp to the root keeper `N` and fails;
/// - `twice` mails every lamp to itself twice, and `fan` to the keeper `50` it spawns, twice in one send, saying what
///   was refused;
/// - `close` closes it, and its closing turn says the light of each lamp it still keeps.
///
/// It takes every lamp it is given, in its config or in mail, but for the keeper `99`, which takes none.
#[derive(Bound)]
struct Keeper {
    number: u64,
    lamps: Vec<Handle<Lamp>>,
}

impl Kind for Keeper {
    const NAME: KindName = KindName::new("aether.test.keeper");
}

impl Keeper {
    /// Every lamp it keeps, given up.
    fn give_all<S>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<Vec<Given<Lamp>>, Fault> {
        mem::take(&mut self.lamps).into_iter().map(|lamp| turn.give(lamp)).collect()
    }

    /// The order `text` sent to itself.
    fn order(text: &str) -> Order {
        Order { text: String::from(text) }
    }
}

impl ChildOf for Keeper {
    type Parent = Recursive;
}

impl Actor for Keeper {
    type Config = KeeperConfig;
    type Key = u64;
    type Handles = (Order, Pass);
    type BeginSends = (ToType<Self, Order>,);
    type EndSends = ();

    fn begin(config: KeeperConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let KeeperConfig { number, lamps, orders } = config;
        let refuses = number == 99;
        let taken = if refuses {
            Vec::new()
        } else {
            lamps.into_iter().map(|lamp| turn.take(lamp)).collect::<Result<_, _>>()?
        };
        say(format!("{number} began with {}", taken.len()));
        for text in orders {
            turn.to(turn.me()).send(&Order { text })?;
        }

        Ok(Self { number, lamps: taken })
    }

    fn end(mut self, turn: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        for lamp in &mut self.lamps {
            say(format!("{} last {}", self.number, turn.open(lamp)?.light));
        }

        Ok(())
    }
}

impl Handles<Order> for Keeper {
    type Sends = (ToType<Self, Order>, ToType<Self, Pass>);

    fn handle(&mut self, order: Order, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let words: Vec<&str> = order.text.split(' ').collect();
        match words[..] {
            ["make", light] => {
                let light = light.parse().unwrap_or_default();
                self.lamps.push(turn.keep(Lamp { light })?);
            }
            ["shine"] => {
                for lamp in &mut self.lamps {
                    say(format!("{} shines {}", self.number, turn.open(lamp)?.light));
                }
            }
            ["spawn", number] => {
                let number = number.parse().unwrap_or_default();
                let lamps = self.give_all(turn)?;
                let config = KeeperConfig { number, lamps, orders: vec![String::from("shine")] };
                let _ = turn.spawn::<Self>(&number, config);
            }
            ["pass", number] => {
                let number: u64 = number.parse().unwrap_or_default();
                let lamps = self.give_all(turn)?;
                turn.send(To::ChildInstanced(Self::NAME, number.to_name()), &Pass { lamps })?;
            }
            ["root", number] => {
                let number: u64 = number.parse().unwrap_or_default();
                let lamps = self.give_all(turn)?;
                let path = ActorPath::from_root(&PathSegment::new(Self::NAME, &number));
                let Some(root) = turn.resolve(&path) else {
                    return Err(Fault::new(&Self::order("no such root"))?);
                };
                turn.send(To::Held(root), &Pass { lamps })?;
            }
            ["slip", number] => {
                let number: u64 = number.parse().unwrap_or_default();
                let lamps = self.give_all(turn)?;
                let path = ActorPath::from_root(&PathSegment::new(Self::NAME, &number));
                if let Some(root) = turn.resolve(&path) {
                    turn.send(To::Held(root), &Pass { lamps })?;
                }
                return Err(Fault::new(&Self::order("slipped"))?);
            }
            ["loose"] => {
                self.give_all(turn)?;
            }
            ["twice"] => {
                let lamps = self.give_all(turn)?;
                let pass = Pass { lamps };
                let me = turn.me();
                let first = turn.to(me).send(&pass).is_ok();
                let second = turn.to(me).send(&pass).is_ok();
                say(format!("{} sent {first} then {second}", self.number));
            }
            ["fan"] => {
                let lamps = self.give_all(turn)?;
                let config = KeeperConfig { number: 50, lamps: Vec::new(), orders: Vec::new() };
                let Ok(child) = turn.spawn(&50, config) else {
                    return Err(Fault::new(&Self::order("unborn"))?);
                };
                let sent = turn.to(&[child, child]).send(&Pass { lamps }).is_ok();
                say(format!("{} fanned {sent}", self.number));
            }
            ["close"] => turn.close(),
            _ => {}
        }

        Ok(())
    }
}

impl Handles<Pass> for Keeper {
    type Sends = ();

    fn handle(&mut self, pass: Pass, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let refuses = self.number == 99;
        if refuses {
            say(format!("{} left {}", self.number, pass.lamps.len()));
            return Ok(());
        }
        for lamp in pass.lamps {
            self.lamps.push(turn.take(lamp)?);
        }
        say(format!("{} holds {}", self.number, self.lamps.len()));

        Ok(())
    }
}

/// A clock that never moves.
struct Stopped;

impl Clock for Stopped {
    fn nanos(&self) -> u64 {
        0
    }
}

/// An engine in four parts whose parts each run at most `hops` fused hops a step.
fn engine(hops: usize) -> Engine {
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };

    Engine::new(parts, 1024, 1 << 20, 1 << 20, hops).ready()
}

/// Adds the root keeper `number`, given `lamps` by the chassis, which sends itself `orders`.
fn add(engine: &mut Engine, number: u64, lamps: Vec<Given<Lamp>>, orders: &[&str]) -> Seated {
    let orders = orders.iter().map(|text| String::from(*text)).collect();
    let Ok(id) = engine.add_root::<Keeper>(&number, KeeperConfig { number, lamps, orders }) else {
        panic!("the keeper begins");
    };
    let Some(seat) = engine.table().seat(id) else {
        panic!("the keeper is live");
    };

    Seated { id, seat }
}

/// Steps the engine until it is idle, and answers with everything said.
fn settle(engine: &mut Engine) -> Vec<String> {
    let mut heard = Vec::new();
    for _ in 0..16 {
        engine.step(Time::ZERO, &Stopped);
        heard.extend(said());
    }

    heard
}

/// The names of the resources the log of `actor` records as dropped.
fn dropped(engine: &Engine, actor: Seated) -> Vec<&'static str> {
    engine
        .log(actor)
        .filter_map(|entry| match entry {
            Entry::Dropped(name) => Some(*name),
            _ => None,
        })
        .collect()
}

#[test]
fn a_resource_the_chassis_hands_a_root_goes_with_mail_to_an_actor_in_another_part() {
    // Catches a resource left behind when the mail that carries it is routed to another part, one handed to its
    // receiver before or after the wrong mail, one a fused hop overtakes, and the chassis's resources handed to the
    // wrong root.
    for hops in [0, 64] {
        OUT.set(0);
        let mut engine = engine(hops);
        let lamp = engine.hand(Lamp { light: 7 });
        let other = engine.hand(Lamp { light: 8 });
        let giver = add(&mut engine, 1, vec![lamp], &["root 2", "shine"]);
        let taker = add(&mut engine, 2, vec![other], &[]);
        assert_ne!(giver.seat.index() % 4, taker.seat.index() % 4, "the roots sit in two parts");
        said();

        let heard = settle(&mut engine);
        let heard: Vec<&str> = heard.iter().map(String::as_str).collect();
        assert_eq!(heard, ["2 holds 2"], "hops={hops}");
        assert_eq!(out(), 0, "hops={hops}");
    }
}

#[test]
fn a_resource_is_dropped_after_its_owners_closing_turn() {
    // Catches what an actor kept dropped before its closing turn can use it, or never dropped once it has gone.
    let mut engine = engine(0);
    add(&mut engine, 1, Vec::new(), &["make 5", "close"]);
    said();

    let heard = settle(&mut engine);

    assert_eq!(heard, ["1 last 5"]);
    assert_eq!(out(), 1);
}

#[test]
fn a_child_takes_from_its_config_what_its_parent_gave_it_and_what_it_leaves_is_dropped() {
    // Catches a resource in a child's config the child cannot take as it begins, one taken by a child it was not
    // given to, and one the child leaves that is kept, or dropped with no record in the parent's log.
    let mut engine = engine(0);
    let parent = add(&mut engine, 1, Vec::new(), &["make 3", "spawn 2", "make 4", "spawn 99"]);
    said();

    let heard = settle(&mut engine);

    assert_eq!(heard, ["2 began with 1", "99 began with 0", "2 shines 3"]);
    assert_eq!(out(), 1, "the lamp the keeper 99 left is dropped");
    assert_eq!(dropped(&engine, parent), [lamp()]);
}

#[test]
fn a_resource_its_receiver_does_not_take_is_dropped_as_its_log_records() {
    // Catches a resource left beside its receiver for good when the turn of its mail does not take it.
    let mut engine = engine(0);
    let parent = add(&mut engine, 1, Vec::new(), &["spawn 99", "make 6", "pass 99"]);
    said();

    let heard = settle(&mut engine);

    assert_eq!(heard, ["99 began with 0", "99 left 1"]);
    assert_eq!(out(), 1);
    let id = crate::child_id(parent.id, &PathSegment::new(Keeper::NAME, &99));
    let Some(seat) = engine.table().seat(id) else {
        panic!("the keeper 99 is live");
    };
    assert_eq!(dropped(&engine, Seated { id, seat }), [lamp()]);
}

#[test]
fn a_resource_no_mail_carries_or_whose_receiver_has_gone_is_dropped_as_its_givers_log_records() {
    // Catches a resource given and sent nowhere kept for good, and one sent to an actor that closed before the mail
    // was handed to it kept beside an empty seat.
    let mut engine = engine(0);
    let giver = add(&mut engine, 1, Vec::new(), &["make 1", "loose", "make 2", "root 2"]);
    add(&mut engine, 2, Vec::new(), &["close"]);
    said();

    settle(&mut engine);

    assert_eq!(out(), 2);
    assert_eq!(dropped(&engine, giver), [lamp(), lamp()]);
}

#[test]
fn a_resource_goes_with_one_piece_of_mail_and_no_more() {
    // Catches one resource carried by two pieces of mail, which two receivers would both take, and mail that carries
    // one fused, so its turn runs before the resource has reached its receiver.
    for hops in [0, 64] {
        OUT.set(0);
        let mut engine = engine(hops);
        add(&mut engine, 1, Vec::new(), &["make 1", "twice", "make 2", "fan"]);
        said();

        let heard = settle(&mut engine);

        assert_eq!(heard, ["1 sent true then false", "50 began with 0", "1 fanned false", "1 holds 1"], "hops={hops}");
        assert_eq!(out(), 1, "the lamp no send carried is dropped, hops={hops}");
    }
}

#[test]
fn a_resource_given_in_a_turn_that_fails_is_dropped_as_its_givers_log_records() {
    // Catches what a failed turn gave handed to its receiver, though the mail that carried it is dropped.
    let mut engine = engine(0);
    let giver = add(&mut engine, 1, Vec::new(), &["make 1", "slip 2"]);
    add(&mut engine, 2, Vec::new(), &[]);
    said();

    let heard = settle(&mut engine);

    assert_eq!(heard, [""; 0]);
    assert_eq!(out(), 1);
    assert_eq!(dropped(&engine, giver), [lamp()]);
}

#[test]
fn every_resource_has_a_ticket_no_other_resource_of_the_engine_has() {
    // Catches tickets each writer counts alike, so a resource the chassis handed in and one kept in a turn share a
    // ticket, and the owner of both opens one through the handle to the other.
    let mut engine = engine(0);
    let lamp = engine.hand(Lamp { light: 8 });
    add(&mut engine, 1, vec![lamp], &["make 5", "shine"]);
    said();

    let heard = settle(&mut engine);

    assert_eq!(heard, ["1 shines 8", "1 shines 5"]);
}
