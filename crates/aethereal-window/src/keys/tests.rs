//! The keys' cascade's tests: which entries each key and typed text reaches through the `Keys` entries a frame listed,
//! and how a key that is down keeps them; and, in an engine, that a window sends its keys only through the entries
//! the subscriber of its probes lists, and never to a raw subscriber.

use super::*;
use crate::event::{Mode, Subscribe, Surface};
use crate::host::{Opening, WindowFeed};
use crate::perform::{Input, Perform};
use crate::pointing::tests::{ProbeOrder, Prober, found};
use crate::window::Window;
use crate::windows::{Heard, Windowing, Windows};
use aethereal_actor::{
    Actor, ActorRef, Beginning, ChildOf, Ending, Fault, Handles, Path, SeatNumber, Seated, Tells, Top, Turn, path,
};
use aethereal_data::{ActorId, ActorPath, Kind, PathSegment};
use aethereal_substrate::Engine;
use aethereal_substrate::harness::{Bench, Harness, Steps, Turned, Unheard};
use std::num::NonZeroUsize;

/// The one window.
const WINDOW: WindowId = WindowId::nth(0);

/// The actor numbered `number`, as an entry names it.
fn actor(number: u64) -> ProtocolRef<KeyHearer> {
    ProtocolRef::found(Seated { id: ActorId::new(number), seat: SeatNumber::new(number) })
}

/// The entry `entry` of `actor`'s list, hearing `keys`, blocking them from the entries beneath it as `blocks` says.
fn heeds(actor: ProtocolRef<KeyHearer>, entry: u32, keys: KeySet, blocks: bool) -> KeysEntry {
    KeysEntry { actor, entry, keys, blocks }
}

/// A cascade whose frame listed `entries`, the last called first.
fn listing(entries: Vec<KeysEntry>) -> KeyCascade {
    let mut cascade = KeyCascade::default();
    cascade.listed(KeysListed { entries });

    cascade
}

/// The key at `code` going down, as the window hears it.
fn down(code: &str) -> Event {
    Event::KeyDown { code: String::from(code), key: String::from(code), repeat: false }
}

/// The key at `code` repeating as it is held.
fn repeat(code: &str) -> Event {
    Event::KeyDown { code: String::from(code), key: String::from(code), repeat: true }
}

/// The key at `code` coming up.
fn up(code: &str) -> Event {
    Event::KeyUp { code: String::from(code), key: String::from(code) }
}

/// `text` typed.
fn typed(text: &str) -> Event {
    Event::Text { text: String::from(text) }
}

/// Whom `events`, heard in order, reach through `cascade`: each delivery as the actor and the entry it went through,
/// with the event.
fn reached(cascade: &mut KeyCascade, events: &[Event]) -> Vec<(ProtocolRef<KeyHearer>, u32, KeyEvent)> {
    events
        .iter()
        .flat_map(|event| cascade.heard(WINDOW, event))
        .map(|(hearer, keyed)| (hearer, keyed.entry, keyed.event))
        .collect()
}

/// `event`, as the cascade delivers it.
fn keyed(event: &Event) -> KeyEvent {
    match event.clone() {
        Event::KeyDown { code, key, repeat } => KeyEvent::KeyDown { code, key, repeat },
        Event::KeyUp { code, key } => KeyEvent::KeyUp { code, key },
        Event::Text { text } => KeyEvent::Text { text },
        other => panic!("{other:?} is not the keys'"),
    }
}

#[test]
fn a_key_reaches_down_to_the_topmost_entry_that_holds_it_and_blocks_past_those_that_do_not_hold_it() {
    // Catches a key delivered only to the top entry, so a console's shortcut that lets keys through hides the game's
    // movement beneath it; one delivered past an entry that blocks, so typing in a text field walks the character; and
    // an entry that blocks other keys stopping this one, so the overlay's backtick, at the top, silences every key.
    let (overlay, shortcut, field, game) = (actor(1), actor(2), actor(3), actor(4));
    let mut cascade = listing(vec![
        heeds(overlay, 0, KeySet::of(["Backquote"]), true),
        heeds(shortcut, 2, KeySet::of(["KeyA", "F5"]), false),
        heeds(field, 1, KeySet::All, true),
        heeds(game, 0, KeySet::All, false),
    ]);

    let sent = reached(&mut cascade, &[down("KeyA")]);

    let pressed = keyed(&down("KeyA"));
    assert_eq!(sent, vec![(shortcut, 2, pressed.clone()), (field, 1, pressed)]);
}

#[test]
fn a_key_no_entry_above_blocks_falls_through_to_every_entry_that_holds_it() {
    // Catches entries that let keys through treated as blocking, so a game's movement is lost under any screen that
    // also listens; and a key sent to an entry whose set does not hold it.
    let (hud, game) = (actor(1), actor(2));
    let mut cascade =
        listing(vec![heeds(hud, 3, KeySet::of(["Tab"]), false), heeds(game, 0, KeySet::of(["KeyW", "Tab"]), false)]);

    let sent = reached(&mut cascade, &[down("Tab"), down("KeyW")]);

    let (tab, w) = (keyed(&down("Tab")), keyed(&down("KeyW")));
    assert_eq!(sent, vec![(hud, 3, tab.clone()), (game, 0, tab), (game, 0, w)]);
}

#[test]
fn typed_text_follows_the_key_that_typed_it() {
    // Catches text sent through the entries as keys are, by some key of its own: the backtick's text would reach the
    // game beneath the overlay, which declared only the backtick, and a text field's typing would not reach it.
    let (overlay, game) = (actor(1), actor(2));
    let mut cascade =
        listing(vec![heeds(overlay, 0, KeySet::of(["Backquote"]), true), heeds(game, 0, KeySet::All, false)]);

    let sent = reached(&mut cascade, &[down("Backquote"), typed("`"), down("KeyW"), typed("w")]);

    let expected = vec![
        (overlay, 0, keyed(&down("Backquote"))),
        (overlay, 0, keyed(&typed("`"))),
        (game, 0, keyed(&down("KeyW"))),
        (game, 0, keyed(&typed("w"))),
    ];
    assert_eq!(sent, expected);
}

#[test]
fn a_key_that_is_down_keeps_whom_its_press_reached_however_the_entries_change() {
    // Catches a key's repeats and release sent through the entries as they stand when they come: a character walking
    // with W held as a console opens would never hear W come up, and walk on; and a key pressed in the console and
    // released after it closed would reach the character as a release it never pressed.
    let (console, game) = (actor(1), actor(2));
    let game_only = vec![heeds(game, 0, KeySet::All, false)];
    let console_over = vec![heeds(console, 0, KeySet::All, true), heeds(game, 0, KeySet::All, false)];
    let mut cascade = listing(game_only.clone());
    let walking = reached(&mut cascade, &[down("KeyW")]);
    cascade.listed(KeysListed { entries: console_over });

    let sent = reached(&mut cascade, &[repeat("KeyW"), typed("w"), up("KeyW"), down("Enter")]);
    cascade.listed(KeysListed { entries: game_only });
    let released = reached(&mut cascade, &[up("Enter")]);

    assert_eq!(walking, vec![(game, 0, keyed(&down("KeyW")))]);
    let expected = vec![
        (game, 0, keyed(&repeat("KeyW"))),
        (game, 0, keyed(&typed("w"))),
        (game, 0, keyed(&up("KeyW"))),
        (console, 0, keyed(&down("Enter"))),
    ];
    assert_eq!(sent, expected);
    assert_eq!(released, vec![(console, 0, keyed(&up("Enter")))]);
}

#[test]
fn the_window_losing_the_keyboard_forgets_the_keys_down() {
    // Catches keys kept as down once the window lost the keyboard, whose releases never come: the next press of W,
    // after a console opened above the game, would go as the old press did, to the game.
    let (console, game) = (actor(1), actor(2));
    let mut cascade = listing(vec![heeds(game, 0, KeySet::All, false)]);
    reached(&mut cascade, &[down("KeyW"), Event::FocusLost]);
    cascade.listed(KeysListed { entries: vec![heeds(console, 0, KeySet::All, true)] });

    let sent = reached(&mut cascade, &[down("KeyW")]);

    assert_eq!(sent, vec![(console, 0, keyed(&down("KeyW")))]);
}

/// An actor at the window, keyed by its number, that subscribes to it raw as it begins and handles the keys its
/// entries are sent: what it is sent is read from the turns the engine recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.typist")]
struct Typist;

impl ChildOf for Typist {
    type Parent = Top;
}

impl Actor for Typist {
    type Config = Self;
    type Key = u64;
    type Handles = (Event, Keyed);
    type BeginSends = (Tells<Path<Window>, Subscribe>,);
    type EndSends = ();

    fn begin(typist: Self, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.to(path::<Window>(WINDOW)).send(&Subscribe)?;

        Ok(typist)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Event> for Typist {
    type Sends = ();

    fn handle(&mut self, _: Event, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Keyed> for Typist {
    type Sends = ();

    fn handle(&mut self, _: Keyed, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// What a typist was sent: an event raw, or a key through an entry.
#[derive(Debug, Clone, PartialEq)]
enum Sent {
    Raw(Event),
    Keyed(Keyed),
}

impl Sent {
    /// What `turned` handed its typist.
    fn of(turned: &Turned) -> Option<Self> {
        if let Ok(event) = turned.read() {
            return Some(Self::Raw(event));
        }

        turned.read().ok().map(Self::Keyed)
    }
}

/// The typist numbered `0`.
const FIRST: u64 = 0;

/// The typist numbered `1`.
const SECOND: u64 = 1;

/// A harness over an engine with the window root, one window opened as it was set up, the prober, subscribed to the
/// window's probes, and the two typists, what they were sent as they subscribed dropped; with the window's feed.
fn typed_at() -> Result<(Harness<Bench>, WindowFeed, ActorRef<Prober>), Unheard> {
    let mut setup = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).with_cycle::<()>();
    let mut opening = Opening::new(setup.source::<Heard>(), setup.controller::<Windowing>());
    let surface = Surface { width: 640, height: 480, scale: 1.0 };
    let feed = opening.open("window", surface, Mode::Windowed, true);
    let (config, _hosting) = opening.config();
    let engine = setup
        .add_root::<Windows>(&(), config)?
        .add_root::<Prober>(&(), Prober)?
        .add_root::<Typist>(&FIRST, Typist)?
        .add_root::<Typist>(&SECOND, Typist)?;
    let mut harness = Harness::bare(engine.ready())?;
    harness.host().engine().record_turns();
    let prober = ActorRef::<Prober>::found(found::<Prober>(&mut harness, path::<Prober>(()).names()));
    harness.tell(prober, &ProbeOrder::Subscribe)?;
    harness.settle_turns();

    Ok((harness, feed, prober))
}

/// The entry `entry` of the typist numbered `typist`'s list, as the engine's table has it, hearing `keys`.
fn typist(harness: &mut Harness<Bench>, typist: u64, entry: u32, keys: KeySet, blocks: bool) -> KeysEntry {
    let actor = ProtocolRef::found(found::<Typist>(harness, path::<Typist>(typist).names()));

    KeysEntry { actor, entry, keys, blocks }
}

/// Pushes `events` onto the window's feed, as its host heard them, in order.
fn push(feed: &WindowFeed, events: &[Event]) {
    for event in events {
        assert_eq!(feed.push(event.clone()), Ok(()), "the window root holds its source");
    }
}

/// Steps until no actor has anything to run, and answers with what each typist was sent meanwhile, the first's then
/// the second's, each in the order it was sent.
fn heard(harness: &mut Harness<Bench>) -> [Vec<Sent>; 2] {
    let turns = harness.settle_turns();

    [FIRST, SECOND].map(|typist| {
        let at = ActorPath::from_root(&PathSegment::new(Typist::NAME, &typist));
        turns.iter().filter(|turned| turned.path == at).filter_map(Sent::of).collect()
    })
}

/// `event` sent through the entry `entry` of the window.
fn through(entry: u32, event: &Event) -> Sent {
    Sent::Keyed(Keyed { window: WINDOW, entry, event: keyed(event) })
}

#[test]
fn a_window_sends_its_keys_only_through_the_entries_its_prober_lists_and_never_raw() -> Result<(), Unheard> {
    // Catches keys still sent to raw subscribers, which would hear what a declaration above them blocked; entries any
    // actor sends taken in, so any actor could take the keys; and input performed by a tool sent another way than the
    // host's.
    let (mut harness, feed, prober) = typed_at()?;
    let pointer = Event::PointerMoved { x: 1.0, y: 2.0 };
    push(&feed, &[down("KeyA"), typed("a"), pointer.clone()]);

    assert_eq!(heard(&mut harness), [vec![Sent::Raw(pointer.clone())], vec![Sent::Raw(pointer)]], "nothing declares");

    let first = typist(&mut harness, FIRST, 2, KeySet::All, true);
    let second = typist(&mut harness, SECOND, 0, KeySet::All, false);
    let listed = KeysListed { entries: vec![first, second] };
    let window = ActorRef::<Window>::found(found::<Window>(&mut harness, path::<Window>(WINDOW).names()));
    harness.tell(window, &listed)?;
    harness.settle_turns();
    push(&feed, &[up("KeyA"), down("KeyB")]);

    assert_eq!(heard(&mut harness), [vec![], vec![]], "entries the harness sent are not the prober's");

    harness.tell(prober, &ProbeOrder::List(listed))?;
    harness.settle_turns();
    push(&feed, &[down("KeyC"), typed("c")]);

    assert_eq!(heard(&mut harness), [vec![through(2, &down("KeyC")), through(2, &typed("c"))], vec![]]);

    let performed = harness.ask(window, &Perform { inputs: vec![Input::Text { text: String::from("d") }] })?;
    let heard = heard(&mut harness);
    harness.answer(performed)?;

    assert_eq!(heard, [vec![through(2, &typed("d"))], vec![]], "input performed goes as the host's does");

    Ok(())
}
