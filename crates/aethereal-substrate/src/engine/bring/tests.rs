//! What the chassis brings in through sources, as the engine hands it to their owners (ADR-0010).

use super::super::*;
use crate::{Closed, Entry, Feed};
use aethereal_actor::Time;
use aethereal_actor::{Controller, Ending, Fault, Handle, Handles, Source, ToType, ToWithoutMail, Turn};
use aethereal_data::{Bound, Kind, KindName, Plain};
use std::cell::RefCell;
use std::mem;

thread_local! {
    /// What this thread's windows have said they heard, in order.
    static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Notes what a window heard.
fn say(line: String) {
    SAID.with_borrow_mut(|said| said.push(line));
}

/// What has been said since this was last asked.
fn said() -> Vec<String> {
    SAID.with_borrow_mut(mem::take)
}

/// The pointer moved: what the chassis brings a window.
#[derive(Plain)]
struct Moved {
    x: u64,
}

impl Kind for Moved {
    const NAME: KindName = KindName::new("aether.test.moved");
}

/// A resource the chassis hands in, which an event carries to a window (ADR-0019).
struct Lamp;

impl Resource for Lamp {}

/// What the chassis brings that holds a resource it handed in, and whether its window takes it.
#[derive(Bound)]
struct Lit {
    lamp: Given<Lamp>,
    take: bool,
}

impl Kind for Lit {
    const NAME: KindName = KindName::new("aether.test.lit");
}

/// A window's number, the sources it is given, and the orders it sends itself as it begins.
#[derive(Bound)]
struct WindowConfig {
    number: u64,
    pointers: Vec<Given<Source<Moved>>>,
    lights: Vec<Given<Source<Lit>>>,
    orders: Vec<String>,
}

impl Kind for WindowConfig {
    const NAME: KindName = KindName::new("aether.test.window_config");
}

/// An order a window sends itself.
#[derive(Plain)]
struct Order {
    text: String,
}

impl Kind for Order {
    const NAME: KindName = KindName::new("aether.test.window_order");
}

/// An actor that holds sources and says what each brings it, whether it came from itself, and whether it is the first
/// of its chain. `spawn N` gives every pointer source to the window `N` it spawns beneath it, and `close` closes it.
#[derive(Bound)]
struct Window {
    number: u64,
    pointers: Vec<Handle<Source<Moved>>>,
    lights: Vec<Handle<Source<Lit>>>,
}

impl Kind for Window {
    const NAME: KindName = KindName::new("aether.test.window");
}

impl Actor for Window {
    type Config = WindowConfig;
    type Key = u64;
    type Handles = (Moved, Order, Lit);
    type BeginSends = (ToType<Self, Order>,);
    type EndSends = ();

    fn begin(config: WindowConfig, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        let WindowConfig { number, pointers, lights, orders } = config;
        let pointers = pointers.into_iter().map(|pointer| turn.take(pointer)).collect::<Result<_, _>>()?;
        let lights = lights.into_iter().map(|light| turn.take(light)).collect::<Result<_, _>>()?;
        for text in orders {
            let me = turn.me();
            turn.send(ToWithoutMail::Held(me), &Order { text })?;
        }

        Ok(Self { number, pointers, lights })
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Moved> for Window {
    type Sends = ();

    fn handle(&mut self, moved: Moved, turn: &mut Turn<'_, ()>) -> Result<(), Fault> {
        let is_from_me = turn.sender() == turn.me();
        let chain = turn.chain();
        let is_first = chain.depth == 0 && chain.root.sender == turn.me().id();
        say(format!("{} moved {} from me {is_from_me} first {is_first}", self.number, moved.x));

        Ok(())
    }
}

impl Handles<Order> for Window {
    type Sends = ();

    fn handle(&mut self, order: Order, turn: &mut Turn<'_, ()>) -> Result<(), Fault> {
        let words: Vec<&str> = order.text.split(' ').collect();
        match words[..] {
            ["spawn", number] => {
                let number = number.parse().unwrap_or_default();
                let given = mem::take(&mut self.pointers).into_iter().map(|pointer| turn.give(pointer));
                let pointers = given.collect::<Result<_, _>>()?;
                let config = WindowConfig { number, pointers, lights: Vec::new(), orders: Vec::new() };
                let _ = turn.spawn::<Self>(&number, config);
            }
            ["close"] => turn.close(),
            _ => {}
        }

        Ok(())
    }
}

impl Handles<Lit> for Window {
    type Sends = ();

    fn handle(&mut self, lit: Lit, turn: &mut Turn<'_, ()>) -> Result<(), Fault> {
        let took = lit.take && turn.take(lit.lamp).is_ok();
        say(format!("{} lit, and took the lamp: {took}", self.number));

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

    Engine::new(parts, 1024, 1 << 20, 1 << 20, hops)
}

/// Adds the root window `number`, given `pointers` and `lights` by the chassis, which sends itself `orders`.
fn add(
    engine: &mut Engine,
    number: u64,
    pointers: Vec<Given<Source<Moved>>>,
    lights: Vec<Given<Source<Lit>>>,
    orders: &[&str],
) -> Seated {
    let orders = orders.iter().map(|text| String::from(*text)).collect();
    let Ok(id) = engine.add_root::<Window>(&number, WindowConfig { number, pointers, lights, orders }) else {
        panic!("the window begins");
    };
    let Some(seat) = engine.table().seat(id) else {
        panic!("the window is live");
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
        add(&mut engine, 1, vec![pointer, other], Vec::new(), &[]);
        push(&feed, &[2, 3]);
        push(&second, &[9]);
        let early = settle(&mut engine);

        engine.bring();
        let heard = settle(&mut engine);

        assert_eq!(early, Vec::<String>::new(), "hops={hops}");
        assert_eq!(
            heard,
            [
                "1 moved 1 from me true first true",
                "1 moved 2 from me true first true",
                "1 moved 3 from me true first true",
                "1 moved 9 from me true first true",
            ],
            "hops={hops}"
        );
    }
}

#[test]
fn what_is_brought_goes_to_whoever_holds_the_source_as_it_is_brought() {
    // Catches what is brought delivered to the source's first owner after it has given the source away, and what was
    // brought before the give sent on to the new owner.
    for hops in [0, 64] {
        let mut engine = engine(hops);
        let (pointer, feed) = engine.source::<Moved>();
        add(&mut engine, 1, vec![pointer], Vec::new(), &["spawn 2"]);
        push(&feed, &[1]);
        engine.bring();
        let before = settle(&mut engine);

        push(&feed, &[2]);
        engine.bring();
        let after = settle(&mut engine);

        assert_eq!(before, ["1 moved 1 from me true first true"], "hops={hops}");
        assert_eq!(after, ["2 moved 2 from me true first true"], "hops={hops}");
    }
}

#[test]
fn a_feed_closes_once_its_source_is_dropped_with_its_owner() {
    // Catches a feed left open after its source's owner has gone, which the chassis would go on feeding, and what was
    // pushed for a gone owner delivered to the actor that sits in its seat next.
    let mut engine = engine(0);
    let (pointer, feed) = engine.source::<Moved>();
    add(&mut engine, 1, vec![pointer], Vec::new(), &["close"]);
    settle(&mut engine);
    push(&feed, &[1]);

    engine.bring();
    add(&mut engine, 2, Vec::new(), Vec::new(), &[]);
    let heard = settle(&mut engine);

    assert!(feed.is_closed());
    assert_eq!(feed.push(Moved { x: 2 }), Err(Closed));
    assert_eq!(heard, Vec::<String>::new());
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
    let heard = settle(&mut engine);

    let logged = |is: fn(&Entry) -> bool| engine.log(window).filter(|entry| is(entry)).count();
    let unbrought = logged(|entry| matches!(entry, Entry::Unbrought(_)));
    let dropped = logged(|entry| matches!(entry, Entry::Dropped(_)));
    assert_eq!(heard[..2], ["1 lit, and took the lamp: false", "1 lit, and took the lamp: true"]);
    assert_eq!((unbrought, dropped), (0, 1), "the lamp not taken is dropped and recorded");
}

/// What a capability asks its host for.
#[derive(Debug, PartialEq, Eq)]
enum Make {
    Window(String),
}

/// What a windows capability is made from: its controller, and the orders it sends itself as it begins.
#[derive(Bound)]
struct WindowsConfig {
    control: Given<Controller<Make>>,
    orders: Vec<String>,
}

impl Kind for WindowsConfig {
    const NAME: KindName = KindName::new("aether.test.windows_config");
}

/// A capability that asks its host for the windows `a` and `b` as it begins, and for one more on each order it sends
/// itself, but `close`, which closes it.
#[derive(Bound)]
struct Windows {
    control: Handle<Controller<Make>>,
}

impl Kind for Windows {
    const NAME: KindName = KindName::new("aether.test.windows");
}

impl Actor for Windows {
    type Config = WindowsConfig;
    type Key = ();
    type Handles = (Order,);
    type BeginSends = (ToType<Self, Order>,);
    type EndSends = ();

    fn begin(config: WindowsConfig, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        let WindowsConfig { control, orders } = config;
        let mut control = turn.take(control)?;
        let controller = turn.open(&mut control)?;
        controller.push(Make::Window(String::from("a")));
        controller.push(Make::Window(String::from("b")));
        for text in orders {
            let me = turn.me();
            turn.send(ToWithoutMail::Held(me), &Order { text })?;
        }

        Ok(Self { control })
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Order> for Windows {
    type Sends = ();

    fn handle(&mut self, order: Order, turn: &mut Turn<'_, ()>) -> Result<(), Fault> {
        if order.text == "close" {
            turn.close();
            return Ok(());
        }
        turn.open(&mut self.control)?.push(Make::Window(order.text));

        Ok(())
    }
}

#[test]
fn the_host_takes_what_a_capability_asks_through_its_controller_in_the_order_asked() {
    // Catches requests lost on their way to the host, taken out of the order pushed, or taken before the turn that
    // pushed them ran; and a host never told the controller is gone once its capability has ended.
    let mut engine = engine(0);
    let (control, requests) = engine.controller::<Make>();
    let orders = vec![String::from("c"), String::from("close")];
    let Ok(_) = engine.add_root::<Windows>(&(), WindowsConfig { control, orders }) else {
        panic!("the capability begins");
    };
    let window = |title: &str| Make::Window(String::from(title));

    let as_it_began = (requests.take(), requests.is_dropped());
    settle(&mut engine);
    let once_it_ended = (requests.take(), requests.is_dropped());

    assert_eq!(as_it_began, (vec![window("a"), window("b")], false));
    assert_eq!(once_it_ended, (vec![window("c")], true));
}
