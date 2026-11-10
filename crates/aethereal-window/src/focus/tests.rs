//! Key focus's tests: while an actor holds a window's key focus, the window's keys, typed text, and wheel turns reach it
//! alone; a release, the holder's end, or a later take ends the hold, each holder told as it gains and loses it; and a
//! key pressed before a take, or under a hold since released, comes up to whoever heard it go down.

use super::*;
use crate::event::{Mode, Subscribe, Surface};
use crate::host::{Hosting, Opening, WindowFeed};
use crate::perform::{Input, Perform};
use crate::window::Window;
use crate::windows::{Heard, Windowing, Windows};
use aethereal_actor::{
    Actor, ActorRef, Beginning, ChildOf, Ending, Fault, Handles, Path, Table, Tells, Top, Turn, path,
};
use aethereal_data::{ActorPath, Kind, KindId, PathSegment};
use aethereal_substrate::Engine;
use aethereal_substrate::harness::{Bench, Harness, Steps, Turned, Unheard};
use std::num::NonZeroUsize;

/// What a test tells a reader to do at the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.order")]
enum Order {
    /// Subscribe to the window.
    Subscribe,
    /// Take the window's key focus.
    Take,
    /// Release the window's key focus.
    Release,
    /// End.
    End,
}

/// An actor at the window, keyed by its number, that does there what it is told: what it is sent is read from the
/// turns the engine recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.reader")]
struct Reader;

impl ChildOf for Reader {
    type Parent = Top;
}

impl Actor for Reader {
    type Config = Self;
    type Key = u64;
    type Handles = (Order, Event, KeyFocusGained, KeyFocusLost);
    type BeginSends = ();
    type EndSends = ();

    fn begin(reader: Self, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(reader)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Order> for Reader {
    type Sends =
        (Tells<Path<Window>, Subscribe>, Tells<Path<Window>, TakeKeyFocus>, Tells<Path<Window>, ReleaseKeyFocus>);

    fn handle(&mut self, order: Order, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let window = path::<Window>(WINDOW);
        match order {
            Order::Subscribe => {
                turn.to(window).send(&Subscribe)?;
            }
            Order::Take => {
                turn.to(window).send(&TakeKeyFocus)?;
            }
            Order::Release => {
                turn.to(window).send(&ReleaseKeyFocus)?;
            }
            Order::End => turn.close(),
        }

        Ok(())
    }
}

impl Handles<Event> for Reader {
    type Sends = ();

    fn handle(&mut self, _: Event, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<KeyFocusGained> for Reader {
    type Sends = ();

    fn handle(&mut self, _: KeyFocusGained, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<KeyFocusLost> for Reader {
    type Sends = ();

    fn handle(&mut self, _: KeyFocusLost, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// The one window, opened as the program was set up.
const WINDOW: WindowId = WindowId::nth(0);

/// The reader numbered `0`: in the tests, the one whose console takes the keys.
const FIRST: u64 = 0;

/// The reader numbered `1`: in the tests, the one whose character walks.
const SECOND: u64 = 1;

/// What a reader was sent.
#[derive(Debug, Clone, PartialEq)]
enum Sent {
    Event(Event),
    Gained(KeyFocusGained),
    Lost(KeyFocusLost),
}

impl Sent {
    /// What `turned` handed its reader: none for a test's order.
    fn of(turned: &Turned) -> Option<Self> {
        if let Ok(event) = turned.read() {
            return Some(Self::Event(event));
        }
        if let Ok(gained) = turned.read() {
            return Some(Self::Gained(gained));
        }

        turned.read().ok().map(Self::Lost)
    }
}

/// The window's notice that a reader gained its key focus.
const GAINED: Sent = Sent::Gained(KeyFocusGained { window: WINDOW });

/// The window's notice that a reader lost its key focus.
const LOST: Sent = Sent::Lost(KeyFocusLost { window: WINDOW });

/// The key at `code` going down, as the window sends it.
fn down(code: &str) -> Event {
    Event::KeyDown { code: String::from(code), key: String::from(code), repeat: false }
}

/// The key at `code` repeating as it is held, as the window sends it.
fn repeat(code: &str) -> Event {
    Event::KeyDown { code: String::from(code), key: String::from(code), repeat: true }
}

/// The key at `code` coming up, as the window sends it.
fn up(code: &str) -> Event {
    Event::KeyUp { code: String::from(code), key: String::from(code) }
}

/// `text` typed, as the window sends it.
fn typed(text: &str) -> Event {
    Event::Text { text: String::from(text) }
}

/// What the window's host pushes onto its feed, each event as `Sent` to a reader.
fn sent(events: &[Event]) -> Vec<Sent> {
    events.iter().cloned().map(Sent::Event).collect()
}

/// A harness over an engine with the window root, one window opened as it was set up, and the two readers, both
/// subscribed to the window, what they were sent as they subscribed dropped; with the window's feed, and what its host
/// keeps.
fn at_a_window() -> Result<(Harness<Bench>, WindowFeed, Hosting), Unheard> {
    let mut setup = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).with_cycle::<()>();
    let mut opening = Opening::new(setup.source::<Heard>(), setup.controller::<Windowing>());
    let surface = Surface { width: 640, height: 480, scale: 1.0 };
    let feed = opening.open("window", surface, Mode::Windowed, true);
    let (config, hosting) = opening.config();
    let engine = setup
        .add_root::<Windows>(&(), config)?
        .add_root::<Reader>(&FIRST, Reader)?
        .add_root::<Reader>(&SECOND, Reader)?;
    let mut harness = Harness::bare(engine.ready())?;
    harness.host().engine().record_turns();

    order(&mut harness, FIRST, Order::Subscribe)?;
    order(&mut harness, SECOND, Order::Subscribe)?;
    harness.settle_turns();

    Ok((harness, feed, hosting))
}

/// Tells the reader numbered `reader` to do `order`.
fn order(harness: &mut Harness<Bench>, reader: u64, order: Order) -> Result<(), Unheard> {
    let place = path::<Reader>(reader);
    let found = harness.host().engine().table().reach(place.names(), KindId::of::<Reader>());

    harness.tell(ActorRef::<Reader>::found(found), &order)
}

/// The window's actor.
fn window(harness: &mut Harness<Bench>) -> ActorRef<Window> {
    let place = path::<Window>(WINDOW);

    ActorRef::found(harness.host().engine().table().reach(place.names(), KindId::of::<Window>()))
}

/// Pushes `events` onto the window's feed, as its host heard them, in order.
fn push(feed: &WindowFeed, events: &[Event]) {
    for event in events {
        assert_eq!(feed.push(event.clone()), Ok(()), "the window root holds its source");
    }
}

/// Steps until no actor has anything to run, and answers with what each reader was sent meanwhile, the first's then
/// the second's, each in the order it was sent.
fn heard(harness: &mut Harness<Bench>) -> [Vec<Sent>; 2] {
    let turns = harness.settle_turns();

    [FIRST, SECOND].map(|reader| {
        let at = ActorPath::from_root(&PathSegment::new(Reader::NAME, &reader));
        turns.iter().filter(|turned| turned.path == at).filter_map(Sent::of).collect()
    })
}

#[test]
fn a_holder_is_sent_its_windows_keys_and_text_and_another_subscriber_only_the_rest() -> Result<(), Unheard> {
    // Catches keys, typed text, or input performed by a tool still sent to every subscriber while one holds the key
    // focus, so a console's typing walks the character; and a pointer's events, which key focus does not route, kept
    // from the other subscribers.
    let (mut harness, feed, _hosting) = at_a_window()?;
    order(&mut harness, FIRST, Order::Take)?;

    assert_eq!(heard(&mut harness), [vec![GAINED], vec![]]);

    let pointer = Event::PointerMoved { x: 1.0, y: 2.0 };
    push(&feed, &[down("KeyA"), typed("a"), up("KeyA"), pointer.clone()]);

    let first = sent(&[down("KeyA"), typed("a"), up("KeyA"), pointer.clone()]);
    assert_eq!(heard(&mut harness), [first, sent(&[pointer])]);

    let window = window(&mut harness);
    let performed = harness.ask(window, &Perform { inputs: vec![Input::Text { text: String::from("b") }] })?;
    let heard = heard(&mut harness);
    harness.answer(performed)?;

    assert_eq!(heard, [sent(&[typed("b")]), vec![]], "input performed is routed as the host's is");

    Ok(())
}

#[test]
fn a_wheel_turn_goes_to_the_holder_while_the_slot_is_held_and_to_everyone_when_it_is_empty() -> Result<(), Unheard> {
    // Catches a wheel turn sent to every subscriber while a console holds the key focus, so scrolling its scrollback
    // also zooms the game beneath it; and one still kept from the game once the console has given the focus up.
    let (mut harness, feed, _hosting) = at_a_window()?;
    order(&mut harness, FIRST, Order::Take)?;
    harness.settle_turns();
    let turn = [Event::Wheel { x: 1.0, y: 2.0, delta_x: 0.0, delta_y: -40.0 }];
    push(&feed, &turn);

    assert_eq!(heard(&mut harness), [sent(&turn), vec![]]);

    order(&mut harness, FIRST, Order::Release)?;
    harness.settle_turns();
    push(&feed, &turn);

    assert_eq!(heard(&mut harness), [sent(&turn), sent(&turn)]);

    Ok(())
}

#[test]
fn a_release_sends_the_keys_to_every_subscriber_again_and_one_by_another_changes_nothing() -> Result<(), Unheard> {
    // Catches a release that leaves the slot held, so keys never come back to the game; a holder not told it lost the
    // focus; and a release by an actor that does not hold the slot emptying it, or telling anyone.
    let (mut harness, feed, _hosting) = at_a_window()?;
    order(&mut harness, FIRST, Order::Take)?;
    harness.settle_turns();
    order(&mut harness, SECOND, Order::Release)?;
    push(&feed, &[typed("a")]);

    assert_eq!(heard(&mut harness), [sent(&[typed("a")]), vec![]]);

    order(&mut harness, FIRST, Order::Release)?;
    assert_eq!(heard(&mut harness), [vec![LOST], vec![]]);
    let keys = [down("KeyA"), typed("a"), up("KeyA")];
    push(&feed, &keys);

    assert_eq!(heard(&mut harness), [sent(&keys), sent(&keys)]);

    Ok(())
}

#[test]
fn the_latest_take_wins_and_the_holder_it_replaces_is_told() -> Result<(), Unheard> {
    // Catches a second take refused while the slot is held, so a chat box could not take the keys from an open console;
    // the replaced holder not told, so it still thinks it has the keys; a take by the holder telling it again; and keys
    // still sent to the replaced holder.
    let (mut harness, feed, _hosting) = at_a_window()?;
    order(&mut harness, FIRST, Order::Take)?;
    harness.settle_turns();
    order(&mut harness, FIRST, Order::Take)?;

    assert_eq!(heard(&mut harness), [vec![], vec![]], "a take by the holder sends nothing");

    order(&mut harness, SECOND, Order::Take)?;
    assert_eq!(heard(&mut harness), [vec![LOST], vec![GAINED]]);
    push(&feed, &[typed("a")]);

    assert_eq!(heard(&mut harness), [vec![], sent(&[typed("a")])]);

    Ok(())
}

#[test]
fn a_key_comes_up_to_whoever_heard_it_go_down_however_the_focus_moved_since() -> Result<(), Unheard> {
    // Catches a key's release routed by the slot as it is when the key comes up: a character walking with W held as a
    // console opens would never hear W come up and walk on, and a key pressed in the console and released after it
    // closed would reach the character as a release it never pressed. Its repeats are routed as its press was too.
    let (mut harness, feed, _hosting) = at_a_window()?;
    push(&feed, &[down("KeyW")]);
    harness.settle_turns();
    order(&mut harness, FIRST, Order::Take)?;
    harness.settle_turns();
    let walking = [repeat("KeyW"), up("KeyW")];
    push(&feed, &walking);

    assert_eq!(heard(&mut harness), [sent(&walking), sent(&walking)]);

    push(&feed, &[down("Enter")]);
    harness.settle_turns();
    order(&mut harness, FIRST, Order::Release)?;
    harness.settle_turns();
    push(&feed, &[up("Enter")]);

    assert_eq!(heard(&mut harness), [sent(&[up("Enter")]), vec![]]);

    Ok(())
}

#[test]
fn a_holder_that_ends_empties_the_slot() -> Result<(), Unheard> {
    // Catches the slot kept by a holder that ended, which would send the window's keys to no one for the rest of the
    // program.
    let (mut harness, feed, _hosting) = at_a_window()?;
    order(&mut harness, FIRST, Order::Take)?;
    harness.settle_turns();
    order(&mut harness, FIRST, Order::End)?;
    harness.settle_turns();
    push(&feed, &[typed("a")]);

    assert_eq!(heard(&mut harness), [vec![], sent(&[typed("a")])]);

    Ok(())
}

#[test]
fn a_window_that_closes_tells_its_holder_before_its_subscribers_hear_it_closed() -> Result<(), Unheard> {
    // Catches a holder never told its window closed, which would keep a console believing it has the keys of a window
    // that is gone; and the notice sent after `Closed`, which its subscribers hear last.
    let (mut harness, _feed, hosting) = at_a_window()?;
    order(&mut harness, FIRST, Order::Take)?;
    harness.settle_turns();
    assert_eq!(hosting.closed(WINDOW), Ok(()));

    assert_eq!(heard(&mut harness), [vec![LOST, Sent::Event(Event::Closed)], sent(&[Event::Closed])]);

    Ok(())
}
