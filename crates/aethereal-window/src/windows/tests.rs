//! The window root's tests: what a host pushes for a window reaches that window's subscribers alone, in order, once it
//! says something new, after how the window stood, and nothing another actor sent reaches them; a window asked for
//! while the program runs is given the next id, is answered once its host has said, and closes as the root is asked,
//! or as its close button says; and a mode asked for is set once its host has, or at once when the window has it.

use super::*;
use crate::event::{Button, Mode, Subscribe, Surface};
use crate::host::{Hosting, Opening, WindowFeed};
use aethereal_actor::{ActorRef, Actors, Answered, Path, Table, Tells, Time, path};
use aethereal_data::{ActorPath, DecodeError, Encode, Kind, KindId, PathSegment};
use aethereal_substrate::harness::{Arrival, Askable, Bench, Harness, Steps, Unheard};
use aethereal_substrate::{Clock, Engine, Setup};
use std::num::NonZeroUsize;

/// A clock that reads the same time always: the tests' steps take no time.
struct Still;

impl Clock for Still {
    fn nanos(&self) -> u64 {
        0
    }
}

/// Brings in what was pushed, and steps until no actor has anything to run.
fn run(engine: &mut Engine) {
    engine.bring();
    while !engine.is_idle() {
        engine.step(Time::from_nanos(0), &Still);
        engine.bring();
    }
}

/// What the listener listens to: the window it subscribes to.
#[derive(Plain)]
#[kind(name = "aether.test.listens")]
struct Listens {
    window: WindowId,
}

/// What another actor sends the window root and the window, which is not the host's.
const FORGED: Event = Event::PointerMoved { x: -1.0, y: -1.0 };

/// An actor that subscribes to one window twice, and sends the window root and the window an event of its own. The
/// events it is sent are read from the turns the engine recorded. It stays an actor because what it does in its `begin`
/// is under test: a root made after the window root reaches each window's actor there, which the outside, whose first
/// mail is sent once every root has begun, cannot show.
#[derive(Bound)]
#[kind(name = "aether.test.listener")]
struct Listener;

impl ChildOf for Listener {
    type Parent = Top;
}

impl Actor for Listener {
    type Config = Listens;
    type Key = ();
    type Handles = (Event,);
    type BeginSends = (Tells<Path<Window>, Subscribe>, Tells<Path<Window>, Event>, Tells<Path<Windows>, Heard>);
    type EndSends = ();

    fn begin(listens: Listens, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let window = listens.window;
        turn.to(path::<Window>(window)).send(&Subscribe)?.send(&Subscribe)?.send(&FORGED)?;
        turn.to(path::<Windows>(())).send(&Heard::Event { window, event: FORGED })?;

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Event> for Listener {
    type Sends = ();

    fn handle(&mut self, _: Event, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// The events the listener was handed in the turns recorded since this was last asked, in order.
fn heard_by_the_listener(engine: &mut Engine) -> Vec<Event> {
    let listener = ActorPath::from_root(&PathSegment::new(Listener::NAME, &()));

    engine.take_turned().iter().filter(|turn| turn.path == listener).filter_map(|turn| turn.read().ok()).collect()
}

/// An engine of one worker, set up with no cycle.
fn setup() -> Setup<()> {
    Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).with_cycle::<()>()
}

/// The window opened `nth`.
const fn window(nth: u64) -> WindowId {
    WindowId::nth(nth)
}

#[test]
fn a_subscriber_hears_its_window_as_it_stood_then_what_its_host_pushed_that_says_something_new() {
    // Catches the window root sending an event to the wrong window's actor or to every window's, an event another actor
    // sent the root or the window sent on as though the host heard it, a subscriber not sent how the window stood,
    // its mode among it, or sent it twice for subscribing twice, a resize to the size the window has, a mode it is
    // shown in, or a focus it has sent on, a turn of the wheel dropped as though it said nothing new, and a window's
    // actor not made before a root made after the window root begins, so a subscription in its `begin` is lost.
    let mut setup = setup();
    let small = Surface { width: 640, height: 480, scale: 1.0 };
    let large = Surface { width: 1280, height: 960, scale: 2.0 };
    let mut opening = Opening::new(setup.source::<Heard>(), setup.controller::<Windowing>());
    let first = opening.open(small, Mode::Windowed, true);
    let second = opening.open(small, Mode::Maximized, false);
    let (mut config, _hosting) = opening.config();
    config.closing = Closing::ByProgram;
    let Ok(mut engine) = setup
        .add_root::<Windows>(&(), config)
        .and_then(|setup| setup.add_root::<Listener>(&(), Listens { window: second.window() }))
        .map(Setup::ready)
    else {
        panic!("the window root and its listener begin");
    };
    engine.record_turns();

    let pushed = [
        first.push(Event::KeyDown { code: String::from("KeyW"), key: String::from("w"), repeat: false }),
        second.push(Event::PointerMoved { x: 1.0, y: 2.0 }),
        second.push(Event::Resized(small)),
        second.push(Event::Resized(large)),
        second.push(Event::Resized(large)),
        second.push(Event::Moded(Mode::Maximized)),
        second.push(Event::Moded(Mode::Fullscreen)),
        second.push(Event::Moded(Mode::Fullscreen)),
        second.push(Event::FocusLost),
        second.push(Event::FocusGained),
        second.push(Event::FocusGained),
        first.push(Event::FocusLost),
        second.push(Event::FocusLost),
        second.push(Event::PointerDown { x: 1.0, y: 2.0, button: Button::Main }),
        second.push(Event::Wheel { x: 1.0, y: 2.0, delta_x: 0.0, delta_y: 80.0 }),
        second.push(Event::CloseRequested),
    ];
    assert!(pushed.iter().all(Result::is_ok), "the window root holds its source");
    run(&mut engine);

    let expected = [
        Event::Resized(small),
        Event::Moded(Mode::Maximized),
        Event::FocusLost,
        Event::PointerMoved { x: 1.0, y: 2.0 },
        Event::Resized(large),
        Event::Moded(Mode::Fullscreen),
        Event::FocusGained,
        Event::FocusLost,
        Event::PointerDown { x: 1.0, y: 2.0, button: Button::Main },
        Event::Wheel { x: 1.0, y: 2.0, delta_x: 0.0, delta_y: 80.0 },
        Event::CloseRequested,
    ];
    assert_eq!(heard_by_the_listener(&mut engine), expected);
}

/// What reached the outside, as a test reads it from the queue (ADR-0036): an answer to what it asked, an event of a
/// window, or mail of any other kind, which no test expects.
#[derive(Debug, Clone, PartialEq)]
enum Noted {
    Opened(Opened),
    Closed(Closed),
    ModeSet(ModeSet),
    Heard(Event),
    Other(KindId),
}

impl Noted {
    /// What `arrival` is.
    fn of(arrival: &Arrival) -> Result<Self, DecodeError> {
        if let Some(opened) = arrival.read()? {
            return Ok(Self::Opened(opened));
        }
        if let Some(closed) = arrival.read()? {
            return Ok(Self::Closed(closed));
        }
        if let Some(set) = arrival.read()? {
            return Ok(Self::ModeSet(set));
        }
        let heard = arrival.read()?.map_or_else(|| Self::Other(arrival.kind()), Self::Heard);

        Ok(heard)
    }
}

/// Steps until no actor has anything to run, and answers with what reached the outside since this was last asked, in
/// the order it arrived.
fn settled(harness: &mut Harness<Bench>) -> Result<Vec<Noted>, Unheard> {
    harness.settle();
    let noted = harness.arrived().iter().map(Noted::of).collect::<Result<_, _>>()?;

    Ok(noted)
}

/// `noted` parted, each part in order, into the events of a window and all else: what two actors sent the outside in
/// one step, as a window its events and the root its answers, arrives in an order no test holds to.
fn parted(noted: Vec<Noted>) -> (Vec<Noted>, Vec<Noted>) {
    noted.into_iter().partition(|noted| matches!(noted, Noted::Heard(_)))
}

/// A harness over a ready engine with the window root, `setup` windows opened as it was set up, each of `surface`, its
/// close buttons closing as `closing` says, and what its host keeps.
fn windowed(
    setup: u64,
    surface: Surface,
    closing: Closing,
) -> Result<(Harness<Bench>, Hosting, Vec<WindowFeed>), Unheard> {
    let mut made = self::setup();
    let mut opening = Opening::new(made.source::<Heard>(), made.controller::<Windowing>());
    let feeds = (0..setup).map(|_| opening.open(surface, Mode::Windowed, false)).collect();
    let (mut config, hosting) = opening.config();
    config.closing = closing;
    let harness = Harness::bare(made.add_root::<Windows>(&(), config)?.ready())?;

    Ok((harness, hosting, feeds))
}

/// Asks `mail` of the window root: what comes of it is read in the order it arrives, through [`settled`].
fn ask<K>(harness: &mut Harness<Bench>, mail: &K) -> Result<(), Unheard>
where
    K: Kind + Encode<Actors>,
    ActorRef<Windows>: Askable<K>,
{
    let windows = harness.root::<Windows>();
    let _ = harness.ask(windows, mail)?;

    Ok(())
}

/// What asks the window root for a window.
fn open() -> OpenWindow {
    OpenWindow { title: String::from("asked"), size: Logical { width: 640.0, height: 480.0 } }
}

/// Subscribes the outside to `window`, whose actor is found where the window's id says.
fn subscribe(harness: &mut Harness<Bench>, window: WindowId) -> Result<(), Unheard> {
    let place = path::<Window>(window);
    let found = harness.host().engine().table().reach(place.names(), KindId::of::<Window>());

    harness.tell(ActorRef::<Window>::found(found), &Subscribe)
}

/// What a request taken asks of a window, without the title and size of one opened.
#[derive(Debug, PartialEq)]
enum Took {
    Open(WindowId),
    Close(WindowId),
    SetMode(WindowId, Mode),
}

/// What each request taken asks of its window.
fn windows_of(requests: &[Windowing]) -> Vec<Took> {
    requests
        .iter()
        .map(|request| match request {
            Windowing::Open { window, .. } => Took::Open(*window),
            Windowing::Close { window } => Took::Close(*window),
            Windowing::SetMode { window, mode } => Took::SetMode(*window, *mode),
        })
        .collect()
}

#[test]
fn windows_asked_for_are_given_ids_after_those_set_up_in_the_order_asked_and_never_twice() -> Result<(), Unheard> {
    // Catches the root counting ids from zero, over the windows opened as the program was set up, a request given an
    // id another was, as one the host did not open being given again, and requests pushed to the host out of the
    // order the root's turns asked them, or the host's answer matched to the wrong request.
    let surface = Surface { width: 640, height: 480, scale: 1.0 };
    let (mut harness, hosting, _feeds) = windowed(2, surface, Closing::ByButton)?;
    ask(&mut harness, &open())?;
    ask(&mut harness, &open())?;
    ask(&mut harness, &open())?;
    harness.settle();

    let asked = hosting.take();
    assert_eq!(windows_of(&asked), [Took::Open(window(2)), Took::Open(window(3)), Took::Open(window(4))]);
    let failed = Unopened::Failed(String::from("no window"));
    let answered = [
        hosting.unopened(window(3), failed.clone()),
        hosting.opened(window(2), surface, true).map(drop),
        hosting.opened(window(4), surface, false).map(drop),
    ];
    assert!(answered.iter().all(Result::is_ok), "the window root holds its source");
    let opened = settled(&mut harness)?;
    ask(&mut harness, &open())?;
    harness.settle();

    assert_eq!(windows_of(&hosting.take()), [Took::Open(window(5))]);
    assert_eq!(
        opened,
        [
            Noted::Opened(Opened::Refused(failed)),
            Noted::Opened(Opened::Window { window: window(2), surface }),
            Noted::Opened(Opened::Window { window: window(4), surface }),
        ]
    );

    Ok(())
}

#[test]
fn a_window_opened_on_request_has_its_actor_before_anything_it_heard_reaches_it() -> Result<(), Unheard> {
    // Catches the root answering, or the host's events for a window being sent on, before the window's actor is made:
    // a resize pushed straight after the window opened would reach no actor and be lost, and the asker would hear the
    // window as it opened, not as it stands.
    let small = Surface { width: 640, height: 480, scale: 1.0 };
    let large = Surface { width: 1280, height: 960, scale: 2.0 };
    let (mut harness, hosting, _feeds) = windowed(1, small, Closing::ByButton)?;
    let windows = harness.root::<Windows>();
    let asked = harness.ask(windows, &open())?;
    harness.settle();

    let Ok(feed) = hosting.opened(window(1), small, false) else {
        panic!("the window root holds its source");
    };
    assert_eq!(feed.window(), window(1));
    assert_eq!(feed.push(Event::Resized(large)), Ok(()));
    let answered = harness.answer(asked)?;

    // Subscribed the moment the answer arrives, as an actor that asked would be, with nothing more stepped.
    assert_eq!(answered, Answered::Yes(Opened::Window { window: window(1), surface: small }));
    subscribe(&mut harness, window(1))?;
    let expected = [
        Noted::Heard(Event::Resized(large)),
        Noted::Heard(Event::Moded(Mode::Windowed)),
        Noted::Heard(Event::FocusLost),
    ];
    assert_eq!(settled(&mut harness)?, expected);

    Ok(())
}

#[test]
fn closing_a_window_not_open_is_refused_and_one_open_closes_once_its_host_has() -> Result<(), Unheard> {
    // Catches a close of a window never given, or still opening, pushed to the host, which would close a window no
    // actor stands for; two closes of one window pushed twice; an asker answered before the host has closed it; a
    // subscriber not sent `Closed` last, or sent what the window heard after; and a window closed once still open to a
    // close asked after.
    let surface = Surface { width: 640, height: 480, scale: 1.0 };
    let (mut harness, hosting, feeds) = windowed(1, surface, Closing::ByButton)?;
    ask(&mut harness, &open())?;
    ask(&mut harness, &CloseWindow { window: window(1) })?;
    ask(&mut harness, &CloseWindow { window: window(9) })?;
    let refused = Noted::Closed(Closed::Refused(Unclosed::NotOpen));
    assert_eq!(settled(&mut harness)?, [refused.clone(), refused.clone()]);
    assert_eq!(windows_of(&hosting.take()), [Took::Open(window(1))]);
    let Ok(feed) = hosting.opened(window(1), surface, true) else {
        panic!("the window root holds its source");
    };
    assert_eq!(settled(&mut harness)?, [Noted::Opened(Opened::Window { window: window(1), surface })]);
    subscribe(&mut harness, window(1))?;
    let standing = [
        Noted::Heard(Event::Resized(surface)),
        Noted::Heard(Event::Moded(Mode::Windowed)),
        Noted::Heard(Event::FocusGained),
    ];
    assert_eq!(settled(&mut harness)?, standing);

    ask(&mut harness, &CloseWindow { window: window(1) })?;
    ask(&mut harness, &CloseWindow { window: window(1) })?;
    assert_eq!(settled(&mut harness)?, [], "no close is answered before the host has closed the window");
    assert_eq!(windows_of(&hosting.take()), [Took::Close(window(1))]);
    let pushed = [hosting.closed(window(1)), feed.push(Event::FocusLost)];
    assert!(pushed.iter().all(Result::is_ok), "the window root holds its source");
    let (heard, answered) = parted(settled(&mut harness)?);
    assert_eq!(heard, [Noted::Heard(Event::Closed)]);
    assert_eq!(answered, [Noted::Closed(Closed::Closed), Noted::Closed(Closed::Closed)]);
    ask(&mut harness, &CloseWindow { window: window(1) })?;

    assert_eq!(settled(&mut harness)?, [refused]);
    assert!(hosting.take().is_empty(), "a window closed is asked to close no more");
    drop(feeds);

    Ok(())
}

#[test]
fn the_close_button_closes_its_window_by_button_and_only_asks_by_program() -> Result<(), Unheard> {
    // Catches `ByProgram` closing a window on its button, which a program that confirms first could not stop, and
    // `ByButton` not closing it, which would leave a program that handles nothing unclosable; and a window closed by
    // its button asked to close again as the program asks too.
    let surface = Surface { width: 640, height: 480, scale: 1.0 };
    for (closing, expected) in [(Closing::ByProgram, Vec::new()), (Closing::ByButton, vec![Took::Close(window(0))])] {
        let (mut harness, hosting, feeds) = windowed(1, surface, closing)?;
        let Some(feed) = feeds.first() else {
            panic!("one window was opened");
        };
        assert_eq!(feed.push(Event::CloseRequested), Ok(()));
        assert_eq!(feed.push(Event::CloseRequested), Ok(()));
        harness.settle();

        assert_eq!(windows_of(&hosting.take()), expected, "{closing:?}");
    }

    Ok(())
}

#[test]
fn a_mode_asked_for_is_set_once_its_host_has_and_one_the_window_has_is_answered_at_once() -> Result<(), Unheard> {
    // Catches a mode the window is shown in pushed to the host, or answered with an event, when nothing would change;
    // a mode asked of a window not open pushed to the host; a request answered before the host has said, or matched
    // to the wrong one of two the host answers in order; a second request answered at once from the mode the window
    // had while the first, which changes it, is unanswered; and the root answering from the mode and surface the
    // window opened with rather than those its host has since pushed.
    let small = Surface { width: 640, height: 480, scale: 1.0 };
    let large = Surface { width: 1920, height: 1080, scale: 1.0 };
    let (mut harness, hosting, feeds) = windowed(1, small, Closing::ByButton)?;
    let fullscreen = SetMode { window: window(0), mode: Mode::Fullscreen };
    subscribe(&mut harness, window(0))?;
    ask(&mut harness, &SetMode { window: window(0), mode: Mode::Windowed })?;
    ask(&mut harness, &fullscreen)?;
    ask(&mut harness, &SetMode { window: window(0), mode: Mode::Windowed })?;
    ask(&mut harness, &SetMode { window: window(9), mode: Mode::Fullscreen })?;
    let (standing, at_once) = parted(settled(&mut harness)?);

    let expected = [Took::SetMode(window(0), Mode::Fullscreen), Took::SetMode(window(0), Mode::Windowed)];
    assert_eq!(windows_of(&hosting.take()), expected);
    let expected = [Noted::ModeSet(ModeSet::Set { surface: small }), Noted::ModeSet(ModeSet::Refused(Unset::NotOpen))];
    assert_eq!(at_once, expected, "no request is answered before the host has said");
    let Some(feed) = feeds.first() else {
        panic!("one window was opened");
    };
    let pushed = [
        feed.push(Event::Moded(Mode::Fullscreen)),
        feed.push(Event::Resized(large)),
        hosting.set(window(0), large),
        hosting.unset(window(0), Unset::NoHost),
    ];
    assert!(pushed.iter().all(Result::is_ok), "the window root holds its source");
    let (moved, answered) = parted(settled(&mut harness)?);
    ask(&mut harness, &fullscreen)?;
    let again = settled(&mut harness)?;

    let expected = [Noted::ModeSet(ModeSet::Set { surface: large }), Noted::ModeSet(ModeSet::Refused(Unset::NoHost))];
    assert_eq!(answered, expected);
    let expected = [
        Noted::Heard(Event::Resized(small)),
        Noted::Heard(Event::Moded(Mode::Windowed)),
        Noted::Heard(Event::FocusLost),
        Noted::Heard(Event::Moded(Mode::Fullscreen)),
        Noted::Heard(Event::Resized(large)),
    ];
    assert_eq!([standing, moved].concat(), expected);
    assert_eq!(again, [Noted::ModeSet(ModeSet::Set { surface: large })]);
    assert!(hosting.take().is_empty(), "a mode the window has is asked of the host no more");

    Ok(())
}

#[test]
fn a_window_that_closes_refuses_the_modes_asked_of_it_as_not_open() -> Result<(), Unheard> {
    // Catches a mode asked of a window that is closing pushed to the host, which would show a window about to close;
    // and a request to set the mode of a window that closed before its host answered it dropped, so its asker hears
    // only that it went unanswered, or answered as set.
    let surface = Surface { width: 640, height: 480, scale: 1.0 };
    let (mut harness, hosting, _feeds) = windowed(1, surface, Closing::ByButton)?;
    let maximized = SetMode { window: window(0), mode: Mode::Maximized };
    ask(&mut harness, &maximized)?;
    ask(&mut harness, &CloseWindow { window: window(0) })?;
    assert_eq!(settled(&mut harness)?, [], "neither is answered before the host has said");
    ask(&mut harness, &maximized)?;
    let refused = || Noted::ModeSet(ModeSet::Refused(Unset::NotOpen));
    assert_eq!(settled(&mut harness)?, [refused()]);

    let expected = [Took::SetMode(window(0), Mode::Maximized), Took::Close(window(0))];
    assert_eq!(windows_of(&hosting.take()), expected);
    assert_eq!(hosting.closed(window(0)), Ok(()));

    assert_eq!(settled(&mut harness)?, [refused(), Noted::Closed(Closed::Closed)]);

    Ok(())
}
