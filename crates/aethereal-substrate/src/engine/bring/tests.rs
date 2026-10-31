//! What the chassis brings in through sources, as the engine hands it to their owners (ADR-0010).

use super::super::*;
use crate::{Closed, Entry, Feed};
use aethereal_actor::Time;
use aethereal_actor::{
    Actor, ChildOf, Controller, Ending, Fault, Handle, Handles, Recursive, Source, ToType, Top, Turn,
};
use aethereal_data::{Bound, Kind, Plain};
use std::mem;

/// The pointer moved: what the chassis brings a window.
#[derive(Plain)]
#[kind(name = "aether.test.moved")]
struct Moved {
    x: u64,
}

/// A resource the chassis hands in, which an event carries to a window (ADR-0019).
struct Lamp;

impl Resource for Lamp {}

/// What the chassis brings that holds a resource it handed in, and whether its window takes it.
#[derive(Bound)]
#[kind(name = "aether.test.lit")]
struct Lit {
    lamp: Given<Lamp>,
    take: bool,
}

/// A window's number, the sources it is given, and the orders it sends itself as it begins.
#[derive(Bound)]
#[kind(name = "aether.test.window_config")]
struct WindowConfig {
    number: u64,
    pointers: Vec<Given<Source<Moved>>>,
    lights: Vec<Given<Source<Lit>>>,
    orders: Vec<Order>,
}

/// An order a window sends itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.window_order")]
enum Order {
    /// Gives every pointer source to this window, which it spawns beneath it.
    Spawn(u64),
    /// Closes it.
    Close,
}

/// What a window keeps of what its sources brought it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
enum Heard {
    /// A move to `x`, whether it came from the window itself, and whether it is the first of its chain.
    Moved { x: u64, from_me: bool, first: bool },
    /// A lamp, and whether the window took it.
    Lit { took: bool },
}

/// A move to `x` that came from the window itself, the first of its chain: what every event is.
const fn moved(x: u64) -> Heard {
    Heard::Moved { x, from_me: true, first: true }
}

/// An actor that holds sources and keeps what each brings it, and does as each [`Order`] says.
#[derive(Bound)]
#[kind(name = "aether.test.window")]
struct Window {
    pointers: Vec<Handle<Source<Moved>>>,
    lights: Vec<Handle<Source<Lit>>>,
    /// What its sources brought it, in order.
    heard: Vec<Heard>,
}

impl ChildOf for Window {
    type Parent = Recursive;
}

impl Actor for Window {
    type Config = WindowConfig;
    type Key = u64;
    type Handles = (Moved, Order, Lit);
    type BeginSends = (ToType<Self, Order>,);
    type EndSends = ();

    fn begin(config: WindowConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let WindowConfig { pointers, lights, orders, .. } = config;
        let pointers = pointers.into_iter().map(|pointer| turn.take(pointer)).collect::<Result<_, _>>()?;
        let lights = lights.into_iter().map(|light| turn.take(light)).collect::<Result<_, _>>()?;
        for order in orders {
            turn.to(turn.me()).send(&order)?;
        }

        Ok(Self { pointers, lights, heard: Vec::new() })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Moved> for Window {
    type Sends = ();

    fn handle(&mut self, moved: Moved, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let is_from_me = turn.sender() == turn.me().erase();
        let chain = turn.chain();
        let is_first = chain.depth == 0 && chain.root.sender == turn.me().id();
        self.heard.push(Heard::Moved { x: moved.x, from_me: is_from_me, first: is_first });

        Ok(())
    }
}

impl Handles<Order> for Window {
    type Sends = ();

    fn handle(&mut self, order: Order, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        match order {
            Order::Spawn(number) => {
                let given = mem::take(&mut self.pointers).into_iter().map(|pointer| turn.give(pointer));
                let pointers = given.collect::<Result<_, _>>()?;
                let config = WindowConfig { number, pointers, lights: Vec::new(), orders: Vec::new() };
                let _ = turn.spawn::<Self>(&number, config);
            }
            Order::Close => turn.close(),
        }

        Ok(())
    }
}

impl Handles<Lit> for Window {
    type Sends = ();

    fn handle(&mut self, lit: Lit, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let took = lit.take && turn.take(lit.lamp).is_ok();
        self.heard.push(Heard::Lit { took });

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

/// Adds the root window `number`, given `pointers` and `lights` by the chassis, which sends itself `orders`.
fn add(
    engine: &mut Engine,
    number: u64,
    pointers: Vec<Given<Source<Moved>>>,
    lights: Vec<Given<Source<Lit>>>,
    orders: &[Order],
) -> Seated {
    let orders = orders.to_vec();
    let Ok(id) = engine.add_root::<Window>(&number, WindowConfig { number, pointers, lights, orders }) else {
        panic!("the window begins");
    };
    let Some(seat) = engine.table().seat(id) else {
        panic!("the window is live");
    };

    Seated { id, seat }
}

/// Steps the engine until it is idle.
fn settle(engine: &mut Engine) {
    for _ in 0..16 {
        engine.step(Time::ZERO, &Stopped);
    }
}

/// What the live window `window` has heard, as of the last step.
fn heard(engine: &Engine, window: Seated) -> &[Heard] {
    let Some(state) = engine.state::<Window>(window) else {
        panic!("the window is live");
    };

    &state.heard
}

/// The live window `number` beneath `parent`.
fn beneath(engine: &Engine, parent: Seated, number: u64) -> Seated {
    let id = crate::child_id(parent.id, &PathSegment::new(Window::NAME, &number));
    let Some(seat) = engine.table().seat(id) else {
        panic!("the child is live");
    };

    Seated { id, seat }
}

/// Pushes a move to each of `xs` onto `feed`.
fn push(feed: &Feed<Moved>, xs: &[u64]) {
    for x in xs {
        let Ok(()) = feed.push(Moved { x: *x }) else {
            panic!("the feed is open");
        };
    }
}

#[test]
fn what_the_chassis_pushes_reaches_the_sources_owner_from_itself_in_order_once_brought() {
    // Catches an event delivered before it is brought, or lost when pushed before its root was made; events of one
    // source out of the order pushed; one that names a sender other than its owner, or belongs to a chain it did not
    // begin; and two sources of one owner whose mail shares a count, which would lose one.
    for hops in [0, 64] {
        let mut engine = engine(hops);
        let (pointer, feed) = engine.source::<Moved>();
        let (other, second) = engine.source::<Moved>();
        push(&feed, &[1]);
        engine.bring();
        let window = add(&mut engine, 1, vec![pointer, other], Vec::new(), &[]);
        push(&feed, &[2, 3]);
        push(&second, &[9]);
        settle(&mut engine);
        assert_eq!(heard(&engine, window), [], "hops={hops}");

        engine.bring();
        settle(&mut engine);

        assert_eq!(heard(&engine, window), [moved(1), moved(2), moved(3), moved(9)], "hops={hops}");
    }
}

#[test]
fn what_is_brought_goes_to_whoever_holds_the_source_as_it_is_brought() {
    // Catches what is brought delivered to the source's first owner after it has given the source away, and what was
    // brought before the give sent on to the new owner.
    for hops in [0, 64] {
        let mut engine = engine(hops);
        let (pointer, feed) = engine.source::<Moved>();
        let first = add(&mut engine, 1, vec![pointer], Vec::new(), &[Order::Spawn(2)]);
        push(&feed, &[1]);
        engine.bring();
        settle(&mut engine);
        let second = beneath(&engine, first, 2);

        assert_eq!(heard(&engine, first), [moved(1)], "hops={hops}");
        assert_eq!(heard(&engine, second), [], "hops={hops}");

        push(&feed, &[2]);
        engine.bring();
        settle(&mut engine);

        assert_eq!(heard(&engine, first), [moved(1)], "hops={hops}");
        assert_eq!(heard(&engine, second), [moved(2)], "hops={hops}");
    }
}

#[test]
fn a_feed_closes_once_its_source_is_dropped_with_its_owner() {
    // Catches a feed left open after its source's owner has gone, which the chassis would go on feeding, and what was
    // pushed for a gone owner delivered to the actor that sits in its seat next.
    let mut engine = engine(0);
    let (pointer, feed) = engine.source::<Moved>();
    add(&mut engine, 1, vec![pointer], Vec::new(), &[Order::Close]);
    settle(&mut engine);
    push(&feed, &[1]);

    engine.bring();
    let next = add(&mut engine, 2, Vec::new(), Vec::new(), &[]);
    settle(&mut engine);

    assert!(feed.is_closed());
    assert_eq!(feed.push(Moved { x: 2 }), Err(Closed));
    assert_eq!(heard(&engine, next), []);
}

#[test]
fn an_event_carries_a_resource_the_chassis_handed_in_to_the_sources_owner() {
    // Catches an event that holds a resource the chassis handed in dropped, as one that holds any resource was; one
    // delivered with the resource left in the engine's keeping, so its owner cannot take it; and a resource its owner
    // did not take kept past the turn that handled the event, or dropped with no record; and a resource put on its way
    // with another event's mail, dropped when that one is handled (ADR-0019). The first event's window does not take
    // its lamp, and the second's does.
    let mut engine = engine(0);
    let (light, feed) = engine.source::<Lit>();
    let (pointer, moves) = engine.source::<Moved>();
    let window = add(&mut engine, 1, vec![pointer], vec![light], &[]);
    push(&moves, &[7]);
    for take in [false, true] {
        let lamp = engine.hand(Lamp);
        let Ok(()) = feed.push(Lit { lamp, take }) else {
            panic!("the feed is open");
        };
    }

    engine.bring();
    settle(&mut engine);

    let logged = |is: fn(&Entry) -> bool| engine.log(window).filter(|entry| is(entry)).count();
    let unbrought = logged(|entry| matches!(entry, Entry::Unbrought(_)));
    let dropped = logged(|entry| matches!(entry, Entry::Dropped(_)));
    assert_eq!(heard(&engine, window)[..2], [Heard::Lit { took: false }, Heard::Lit { took: true }]);
    assert_eq!((unbrought, dropped), (0, 1), "the lamp not taken is dropped and recorded");
}

/// What a capability asks its host for.
#[derive(Debug, PartialEq, Eq)]
enum Make {
    /// The window of this number.
    Window(u64),
}

/// An order a windows capability sends itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.windows_order")]
enum Asking {
    /// Asks its host for the window of this number.
    Window(u64),
    /// Closes it.
    Close,
}

/// What a windows capability is made from: its controller, and the orders it sends itself as it begins.
#[derive(Bound)]
#[kind(name = "aether.test.windows_config")]
struct WindowsConfig {
    control: Given<Controller<Make>>,
    orders: Vec<Asking>,
}

/// A capability that asks its host for the windows `1` and `2` as it begins, and does as each [`Asking`] says.
#[derive(Bound)]
#[kind(name = "aether.test.windows")]
struct Windows {
    control: Handle<Controller<Make>>,
}

impl ChildOf for Windows {
    type Parent = Top;
}

impl Actor for Windows {
    type Config = WindowsConfig;
    type Key = ();
    type Handles = (Asking,);
    type BeginSends = (ToType<Self, Asking>,);
    type EndSends = ();

    fn begin(config: WindowsConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let WindowsConfig { control, orders } = config;
        let mut control = turn.take(control)?;
        let controller = turn.open(&mut control)?;
        controller.push(Make::Window(1));
        controller.push(Make::Window(2));
        for order in orders {
            turn.to(turn.me()).send(&order)?;
        }

        Ok(Self { control })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Asking> for Windows {
    type Sends = ();

    fn handle(&mut self, asking: Asking, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        match asking {
            Asking::Window(number) => turn.open(&mut self.control)?.push(Make::Window(number)),
            Asking::Close => turn.close(),
        }

        Ok(())
    }
}

#[test]
fn the_host_takes_what_a_capability_asks_through_its_controller_in_the_order_asked() {
    // Catches requests lost on their way to the host, taken out of the order pushed, or taken before the turn that
    // pushed them ran; and a host never told the controller is gone once its capability has ended.
    let mut engine = engine(0);
    let (control, requests) = engine.controller::<Make>();
    let orders = vec![Asking::Window(3), Asking::Close];
    let Ok(_) = engine.add_root::<Windows>(&(), WindowsConfig { control, orders }) else {
        panic!("the capability begins");
    };

    let as_it_began = (requests.take(), requests.is_dropped());
    settle(&mut engine);
    let once_it_ended = (requests.take(), requests.is_dropped());

    assert_eq!(as_it_began, (vec![Make::Window(1), Make::Window(2)], false));
    assert_eq!(once_it_ended, (vec![Make::Window(3)], true));
}
