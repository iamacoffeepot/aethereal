//! Resources as a step's turns keep, give, and take them, and as the engine drops them (ADR-0010).

use super::super::*;
use crate::Entry;
use aethereal_actor::Time;
use aethereal_actor::{Actor, ChildOf, Ending, Fault, Handle, Handles, Recursive, ToAny, ToType, Turn, child};
use aethereal_data::{Bound, Kind, Plain};
use std::any;
use std::mem;
use std::sync::{Arc, Weak};

/// A resource: a value that is not data, which holds a test's witness for as long as it is not dropped.
struct Lamp {
    light: u64,
    _witness: Arc<()>,
}

impl Resource for Lamp {}

/// A lamp of light `light`, held to by `witness`.
fn lit(witness: &Arc<()>, light: u64) -> Lamp {
    Lamp { light, _witness: Arc::clone(witness) }
}

/// How many lamps held to by `witness` have not been dropped.
fn lamps(witness: &Arc<()>) -> usize {
    Arc::strong_count(witness) - 1
}

/// What makes a keeper's lamps: a resource the chassis hands a root, which holds no lamp's place in the count.
struct Maker {
    witness: Weak<()>,
}

impl Resource for Maker {}

/// What the log records a dropped lamp as.
fn lamp() -> &'static str {
    any::type_name::<Lamp>()
}

/// A keeper's number, the lamps it is given, what makes its lamps, and the orders it sends itself as it begins.
#[derive(Bound)]
#[kind(name = "aether.test.keeper_config")]
struct KeeperConfig {
    number: u64,
    lamps: Vec<Given<Lamp>>,
    makers: Vec<Given<Maker>>,
    orders: Vec<Order>,
    takes: Takes,
}

/// What a keeper does with a lamp it is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Takes {
    /// It takes every lamp, in its config or in mail.
    Every,
    /// It takes none.
    None,
}

/// An order a keeper sends itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.order")]
enum Order {
    /// Makes a lamp of this light and keeps it.
    Make(u64),
    /// Keeps the light of each lamp it keeps.
    Shine,
    /// Gives what makes its lamps to this keeper, which it spawns beneath it, and which makes a lamp of light `5` and
    /// closes.
    Heir(u64),
    /// Gives every lamp to this keeper, which it spawns beneath it, in its config.
    Spawn(u64),
    /// Spawns this keeper beneath it, which takes no lamp, giving it every lamp in its config.
    SpawnRefusing(u64),
    /// Mails every lamp to this keeper beneath it.
    Pass(u64),
    /// Mails every lamp to this root keeper.
    Root(u64),
    /// Gives every lamp and sends none.
    Loose,
    /// Mails every lamp to this root keeper, and fails.
    Slip(u64),
    /// Mails every lamp to itself twice, keeping what was refused.
    Twice,
    /// Mails every lamp to the keeper `50` it spawns, twice in one send, keeping what was refused.
    Fan,
    /// Closes it, and its closing turn tells its parent the light of each lamp it still keeps.
    Close,
}

/// Why a keeper's turn failed.
#[derive(Plain)]
#[kind(name = "aether.test.keeper_trouble")]
enum Trouble {
    NoSuchRoot,
    Slipped,
    Unborn,
}

/// Lamps passed from one keeper to another in mail.
#[derive(Bound)]
#[kind(name = "aether.test.pass")]
struct Pass {
    lamps: Vec<Given<Lamp>>,
}

/// The light of a lamp the keeper `number` still kept in its closing turn, told to its parent: its state goes with it.
#[derive(Plain)]
#[kind(name = "aether.test.last")]
struct Last {
    number: u64,
    light: u64,
}

/// What a keeper did, as it keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
enum Did {
    /// It began, and took this many lamps.
    Began(u64),
    /// A lamp it keeps shone with this light.
    Shone(u64),
    /// Mail carried this many lamps, which it left.
    Left(u64),
    /// It holds this many lamps, once mail brought it more.
    Holds(u64),
    /// Whether the first send of its lamps, and then the second, went.
    Sent(bool, bool),
    /// Whether the send of its lamps to two receivers went.
    Fanned(bool),
    /// The keeper beneath it closed with a lamp of this light.
    Last { number: u64, light: u64 },
}

/// An actor that keeps lamps, as each [`Order`] says, and takes what it is given as [`Takes`] says.
#[derive(Bound)]
#[kind(name = "aether.test.keeper")]
struct Keeper {
    number: u64,
    takes: Takes,
    lamps: Vec<Handle<Lamp>>,
    makers: Vec<Handle<Maker>>,
    /// What it did, in order.
    did: Vec<Did>,
}

impl Keeper {
    /// Every lamp it keeps, given up.
    fn give_all<S>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<Vec<Given<Lamp>>, Fault> {
        mem::take(&mut self.lamps).into_iter().map(|lamp| turn.give(lamp)).collect()
    }

    /// Spawns the keeper `number` beneath it with every lamp in its config, taking them as `takes` says.
    fn spawn_with_lamps<S>(&mut self, turn: &mut Turn<'_, Self, S>, number: u64, takes: Takes) -> Result<(), Fault> {
        let lamps = self.give_all(turn)?;
        let config = KeeperConfig { number, lamps, makers: Vec::new(), orders: vec![Order::Shine], takes };
        let _ = turn.spawn::<Self>(&number, config);

        Ok(())
    }
}

impl ChildOf for Keeper {
    type Parent = Recursive;
}

impl Actor for Keeper {
    type Config = KeeperConfig;
    type Key = u64;
    type Handles = (Order, Pass, Last);
    type BeginSends = (ToType<Self, Order>,);
    type EndSends = (ToType<Self, Last>,);

    fn begin(config: KeeperConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let KeeperConfig { number, lamps, makers, orders, takes } = config;
        let refuses = takes == Takes::None;
        let taken = if refuses {
            Vec::new()
        } else {
            lamps.into_iter().map(|lamp| turn.take(lamp)).collect::<Result<_, _>>()?
        };
        let makers = makers.into_iter().map(|maker| turn.take(maker)).collect::<Result<_, _>>()?;
        let did = vec![Did::Began(taken.len().try_into().unwrap_or(u64::MAX))];
        for order in orders {
            turn.to(turn.me()).send(&order)?;
        }

        Ok(Self { number, takes, lamps: taken, makers, did })
    }

    fn end(mut self, turn: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        let number = self.number;
        for lamp in &mut self.lamps {
            let light = turn.open(lamp)?.light;
            if let Some(parent) = turn.parent() {
                turn.to(parent).send(&Last { number, light })?;
            }
        }

        Ok(())
    }
}

impl Handles<Order> for Keeper {
    type Sends = (ToType<Self, Order>, ToType<Self, Pass>, ToAny<(Pass,), Pass>);

    fn handle(&mut self, order: Order, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        match order {
            Order::Make(light) => {
                let mut witnesses = Vec::new();
                for maker in &mut self.makers {
                    witnesses.extend(turn.open(maker)?.witness.upgrade());
                }
                for witness in witnesses {
                    self.lamps.push(turn.keep(lit(&witness, light))?);
                }
            }
            Order::Shine => {
                for lamp in &mut self.lamps {
                    self.did.push(Did::Shone(turn.open(lamp)?.light));
                }
            }
            Order::Heir(number) => {
                let given = mem::take(&mut self.makers).into_iter().map(|maker| turn.give(maker));
                let makers = given.collect::<Result<_, _>>()?;
                let orders = vec![Order::Make(5), Order::Close];
                let config = KeeperConfig { number, lamps: Vec::new(), makers, orders, takes: Takes::Every };
                let _ = turn.spawn::<Self>(&number, config);
            }
            Order::Spawn(number) => self.spawn_with_lamps(turn, number, Takes::Every)?,
            Order::SpawnRefusing(number) => self.spawn_with_lamps(turn, number, Takes::None)?,
            Order::Pass(number) => {
                let lamps = self.give_all(turn)?;
                turn.to(child::<Self>(number)).send(&Pass { lamps })?;
            }
            Order::Root(number) => {
                let lamps = self.give_all(turn)?;
                let path = ActorPath::from_root(&PathSegment::new(Self::NAME, &number));
                let Some(root) = turn.resolve(&path).and_then(|found| turn.cast(found)) else {
                    return Err(Fault::new(&Trouble::NoSuchRoot)?);
                };
                turn.to(root).send(&Pass { lamps })?;
            }
            Order::Slip(number) => {
                let lamps = self.give_all(turn)?;
                let path = ActorPath::from_root(&PathSegment::new(Self::NAME, &number));
                if let Some(root) = turn.resolve(&path).and_then(|found| turn.cast(found)) {
                    turn.to(root).send(&Pass { lamps })?;
                }
                return Err(Fault::new(&Trouble::Slipped)?);
            }
            Order::Loose => {
                self.give_all(turn)?;
            }
            Order::Twice => {
                let lamps = self.give_all(turn)?;
                let pass = Pass { lamps };
                let me = turn.me();
                let first = turn.to(me).send(&pass).is_ok();
                let second = turn.to(me).send(&pass).is_ok();
                self.did.push(Did::Sent(first, second));
            }
            Order::Fan => {
                let lamps = self.give_all(turn)?;
                let config = KeeperConfig {
                    number: 50,
                    lamps: Vec::new(),
                    makers: Vec::new(),
                    orders: Vec::new(),
                    takes: Takes::Every,
                };
                let Ok(child) = turn.spawn(&50, config) else {
                    return Err(Fault::new(&Trouble::Unborn)?);
                };
                let sent = turn.to(&[child, child]).send(&Pass { lamps }).is_ok();
                self.did.push(Did::Fanned(sent));
            }
            Order::Close => turn.close(),
        }

        Ok(())
    }
}

impl Handles<Pass> for Keeper {
    type Sends = ();

    fn handle(&mut self, pass: Pass, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let refuses = self.takes == Takes::None;
        if refuses {
            self.did.push(Did::Left(pass.lamps.len().try_into().unwrap_or(u64::MAX)));
            return Ok(());
        }
        for lamp in pass.lamps {
            self.lamps.push(turn.take(lamp)?);
        }
        self.did.push(Did::Holds(self.lamps.len().try_into().unwrap_or(u64::MAX)));

        Ok(())
    }
}

impl Handles<Last> for Keeper {
    type Sends = ();

    fn handle(&mut self, last: Last, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.did.push(Did::Last { number: last.number, light: last.light });

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

/// Adds the root keeper `number`, given `lamps` by the chassis and what makes lamps held to by `witness`, which sends
/// itself `orders`.
fn add(engine: &mut Engine, witness: &Arc<()>, number: u64, lamps: Vec<Given<Lamp>>, orders: &[Order]) -> Seated {
    let orders = orders.to_vec();
    let makers = vec![engine.hand(Maker { witness: Arc::downgrade(witness) })];
    let config = KeeperConfig { number, lamps, makers, orders, takes: Takes::Every };
    let Ok(id) = engine.add_root::<Keeper>(&number, config) else {
        panic!("the keeper begins");
    };
    let Some(seat) = engine.table().seat(id) else {
        panic!("the keeper is live");
    };

    Seated { id, seat }
}

/// Steps the engine until it is idle.
fn settle(engine: &mut Engine) {
    for _ in 0..16 {
        engine.step(Time::ZERO, &Stopped);
    }
}

/// What the live keeper `keeper` has done, as of the last step.
fn did(engine: &Engine, keeper: Seated) -> &[Did] {
    let Some(state) = engine.state::<Keeper>(keeper) else {
        panic!("the keeper is live");
    };

    &state.did
}

/// The live keeper `number` beneath `parent`.
fn beneath(engine: &Engine, parent: Seated, number: u64) -> Seated {
    let id = crate::child_id(parent.id, &PathSegment::new(Keeper::NAME, &number));
    let Some(seat) = engine.table().seat(id) else {
        panic!("the keeper is live");
    };

    Seated { id, seat }
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
        let witness = Arc::new(());
        let mut engine = engine(hops);
        let lamp = engine.hand(lit(&witness, 7));
        let other = engine.hand(lit(&witness, 8));
        let giver = add(&mut engine, &witness, 1, vec![lamp], &[Order::Root(2), Order::Shine]);
        let taker = add(&mut engine, &witness, 2, vec![other], &[]);
        assert_ne!(giver.seat.index() % 4, taker.seat.index() % 4, "the roots sit in two parts");

        settle(&mut engine);

        assert_eq!(did(&engine, giver), [Did::Began(1)], "hops={hops}");
        assert_eq!(did(&engine, taker), [Did::Began(1), Did::Holds(2)], "hops={hops}");
        assert_eq!(lamps(&witness), 2, "hops={hops}");
    }
}

#[test]
fn a_resource_is_dropped_after_its_owners_closing_turn() {
    // Catches what an actor kept dropped before its closing turn can use it, or never dropped once it has gone.
    let witness = Arc::new(());
    let mut engine = engine(0);
    let parent = add(&mut engine, &witness, 1, Vec::new(), &[Order::Heir(2)]);

    settle(&mut engine);

    assert_eq!(did(&engine, parent), [Did::Began(0), Did::Last { number: 2, light: 5 }]);
    assert_eq!(lamps(&witness), 0);
}

#[test]
fn a_child_takes_from_its_config_what_its_parent_gave_it_and_what_it_leaves_is_dropped() {
    // Catches a resource in a child's config the child cannot take as it begins, one taken by a child it was not
    // given to, and one the child leaves that is kept, or dropped with no record in the parent's log.
    let witness = Arc::new(());
    let mut engine = engine(0);
    let parent = add(
        &mut engine,
        &witness,
        1,
        Vec::new(),
        &[Order::Make(3), Order::Spawn(2), Order::Make(4), Order::SpawnRefusing(99)],
    );

    settle(&mut engine);

    assert_eq!(did(&engine, beneath(&engine, parent, 2)), [Did::Began(1), Did::Shone(3)]);
    assert_eq!(did(&engine, beneath(&engine, parent, 99)), [Did::Began(0)]);
    assert_eq!(lamps(&witness), 1, "the lamp the keeper 99 left is dropped");
    assert_eq!(dropped(&engine, parent), [lamp()]);
}

#[test]
fn a_resource_its_receiver_does_not_take_is_dropped_as_its_log_records() {
    // Catches a resource left beside its receiver for good when the turn of its mail does not take it.
    let witness = Arc::new(());
    let mut engine = engine(0);
    let parent =
        add(&mut engine, &witness, 1, Vec::new(), &[Order::SpawnRefusing(99), Order::Make(6), Order::Pass(99)]);

    settle(&mut engine);

    let receiver = beneath(&engine, parent, 99);
    assert_eq!(did(&engine, receiver), [Did::Began(0), Did::Left(1)]);
    assert_eq!(lamps(&witness), 0);
    assert_eq!(dropped(&engine, receiver), [lamp()]);
}

#[test]
fn a_resource_no_mail_carries_or_whose_receiver_has_gone_is_dropped_as_its_givers_log_records() {
    // Catches a resource given and sent nowhere kept for good, and one sent to an actor that closed before the mail
    // was handed to it kept beside an empty seat.
    let witness = Arc::new(());
    let mut engine = engine(0);
    let giver =
        add(&mut engine, &witness, 1, Vec::new(), &[Order::Make(1), Order::Loose, Order::Make(2), Order::Root(2)]);
    add(&mut engine, &witness, 2, Vec::new(), &[Order::Close]);

    settle(&mut engine);

    assert_eq!(lamps(&witness), 0);
    assert_eq!(dropped(&engine, giver), [lamp(), lamp()]);
}

#[test]
fn a_resource_goes_with_one_piece_of_mail_and_no_more() {
    // Catches one resource carried by two pieces of mail, which two receivers would both take, and mail that carries
    // one fused, so its turn runs before the resource has reached its receiver.
    for hops in [0, 64] {
        let witness = Arc::new(());
        let mut engine = engine(hops);
        let keeper =
            add(&mut engine, &witness, 1, Vec::new(), &[Order::Make(1), Order::Twice, Order::Make(2), Order::Fan]);

        settle(&mut engine);

        let expected = [Did::Began(0), Did::Sent(true, false), Did::Fanned(false), Did::Holds(1)];
        assert_eq!(did(&engine, keeper), expected, "hops={hops}");
        assert_eq!(did(&engine, beneath(&engine, keeper, 50)), [Did::Began(0)], "hops={hops}");
        assert_eq!(lamps(&witness), 1, "the lamp no send carried is dropped, hops={hops}");
    }
}

#[test]
fn a_resource_given_in_a_turn_that_fails_is_dropped_as_its_givers_log_records() {
    // Catches what a failed turn gave handed to its receiver, though the mail that carried it is dropped.
    let witness = Arc::new(());
    let mut engine = engine(0);
    let giver = add(&mut engine, &witness, 1, Vec::new(), &[Order::Make(1), Order::Slip(2)]);
    let receiver = add(&mut engine, &witness, 2, Vec::new(), &[]);

    settle(&mut engine);

    assert_eq!(did(&engine, receiver), [Did::Began(0)]);
    assert_eq!(lamps(&witness), 0);
    assert_eq!(dropped(&engine, giver), [lamp()]);
}

#[test]
fn every_resource_has_a_ticket_no_other_resource_of_the_engine_has() {
    // Catches tickets each writer counts alike, so a resource the chassis handed in and one kept in a turn share a
    // ticket, and the owner of both opens one through the handle to the other.
    let witness = Arc::new(());
    let mut engine = engine(0);
    let lamp = engine.hand(lit(&witness, 8));
    let keeper = add(&mut engine, &witness, 1, vec![lamp], &[Order::Make(5), Order::Shine]);

    settle(&mut engine);

    assert_eq!(did(&engine, keeper), [Did::Began(1), Did::Shone(8), Did::Shone(5)]);
}
