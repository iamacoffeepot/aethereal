//! The window root's tests: what a host pushes for a window reaches that window's subscribers alone, in order, once it
//! says something new, after how the window stood, and nothing another actor sent reaches them; and a window asked
//! for while the program runs is given the next id, is answered once its host has said, and closes as the root is
//! asked, or as its close button says.

use super::*;
use crate::event::{Button, Subscribe, Surface};
use crate::host::{Hosting, Opening, WindowFeed};
use aethereal_actor::{Answer, Answered, Asks, Path, Tells, Time, path};
use aethereal_substrate::{Clock, Engine, Setup};
use std::num::NonZeroUsize;
use std::sync::Mutex;

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

/// What the listener was sent, in order.
static HEARD: Mutex<Vec<Event>> = Mutex::new(Vec::new());

/// What the listener listens to: the window it subscribes to.
#[derive(Plain)]
#[kind(name = "aether.test.listens")]
struct Listens {
    window: WindowId,
}

/// What another actor sends the window root and the window, which is not the host's.
const FORGED: Event = Event::PointerMoved { x: -1.0, y: -1.0 };

/// An actor that subscribes to one window twice, sends the window root and the window an event of its own, and notes
/// each event it is sent.
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

    fn handle(&mut self, event: Event, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        HEARD.lock().map(|mut heard| heard.push(event)).ok();

        Ok(())
    }
}

/// An engine of one worker, set up with no cycle.
fn setup() -> Setup<()> {
    let Ok(setup) = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).with_cycle::<()>() else {
        panic!("the cycle is in order");
    };

    setup
}

/// The window opened `nth`.
const fn window(nth: u64) -> WindowId {
    WindowId::nth(nth)
}

#[test]
fn a_subscriber_hears_its_window_as_it_stood_then_what_its_host_pushed_that_says_something_new() {
    // Catches the window root sending an event to the wrong window's actor or to every window's, an event another actor
    // sent the root or the window sent on as though the host heard it, a subscriber not sent how the window stood or
    // sent it twice for subscribing twice, a resize to the size the window has or a focus it has sent on, a turn of the
    // wheel dropped as though it said nothing new, and a window's actor not made before a root made after the window
    // root begins, so a subscription in its `begin` is lost.
    let mut setup = setup();
    let small = Surface { width: 640, height: 480, scale: 1.0 };
    let large = Surface { width: 1280, height: 960, scale: 2.0 };
    let mut opening = Opening::new(setup.source::<Heard>(), setup.controller::<Windowing>());
    let first = opening.open(small, true);
    let second = opening.open(small, false);
    let (mut config, _hosting) = opening.config();
    config.closing = Closing::ByProgram;
    let Ok(mut engine) = setup
        .add_root::<Windows>(&(), config)
        .and_then(|setup| setup.add_root::<Listener>(&(), Listens { window: second.window() }))
        .map(Setup::ready)
    else {
        panic!("the window root and its listener begin");
    };

    let pushed = [
        first.push(Event::KeyDown { code: String::from("KeyW"), key: String::from("w"), repeat: false }),
        second.push(Event::PointerMoved { x: 1.0, y: 2.0 }),
        second.push(Event::Resized(small)),
        second.push(Event::Resized(large)),
        second.push(Event::Resized(large)),
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

    let Ok(heard) = HEARD.lock().map(|heard| heard.clone()) else {
        panic!("what was heard is not poisoned");
    };
    let expected = [
        Event::Resized(small),
        Event::FocusLost,
        Event::PointerMoved { x: 1.0, y: 2.0 },
        Event::Resized(large),
        Event::FocusGained,
        Event::FocusLost,
        Event::PointerDown { x: 1.0, y: 2.0, button: Button::Main },
        Event::Wheel { x: 1.0, y: 2.0, delta_x: 0.0, delta_y: 80.0 },
        Event::CloseRequested,
    ];
    assert_eq!(heard, expected);
}

/// What an asker noted, beside the test it is of.
static NOTED: Mutex<Vec<(u64, Noted)>> = Mutex::new(Vec::new());

/// What an asker noted: an answer to what it asked, or an event of a window it was answered with.
#[derive(Debug, Clone, PartialEq)]
enum Noted {
    Opened(Opened),
    Closed(Closed),
    Heard(Event),
}

/// What the asker of a test noted, in order.
fn noted(test: u64) -> Vec<Noted> {
    let Ok(noted) = NOTED.lock() else {
        panic!("what was noted is not poisoned");
    };

    noted.iter().filter(|(of, _)| *of == test).map(|(_, noted)| noted.clone()).collect()
}

/// What the asker of `test` asks the window root as it begins: to open `opens` windows, then to close each of
/// `closes`.
#[derive(Plain)]
#[kind(name = "aether.test.script")]
struct Script {
    test: u64,
    opens: u8,
    closes: Vec<WindowId>,
}

/// An empty context.
#[derive(Plain)]
#[kind(name = "aether.test.asked")]
struct Asked;

/// An actor, keyed by the test it is of, that asks the window root what its script says, notes each answer, and
/// subscribes to each window it is answered with, noting what it hears.
#[derive(Bound)]
#[kind(name = "aether.test.asker")]
struct Asker {
    test: u64,
}

impl ChildOf for Asker {
    type Parent = Top;
}

impl Asker {
    /// Notes `noted`.
    fn note(&self, noted: Noted) {
        NOTED.lock().map(|mut all| all.push((self.test, noted))).ok();
    }
}

impl Actor for Asker {
    type Config = Script;
    type Key = u64;
    type Handles = (Answer<Opened, Asked>, Answer<Closed, Asked>, Event);
    type BeginSends = (Asks<Path<Windows>, OpenWindow, Asked>, Asks<Path<Windows>, CloseWindow, Asked>);
    type EndSends = ();

    fn begin(script: Script, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let open = OpenWindow { title: String::from("asked"), size: Logical { width: 640.0, height: 480.0 } };
        for _ in 0..script.opens {
            turn.to(path::<Windows>(())).ask(&open, &Asked)?;
        }
        for window in script.closes {
            turn.to(path::<Windows>(())).ask(&CloseWindow { window }, &Asked)?;
        }

        Ok(Self { test: script.test })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Answer<Opened, Asked>> for Asker {
    type Sends = (Tells<Path<Window>, Subscribe>,);

    fn handle(
        &mut self,
        (answered, _): (Answered<Opened>, Asked),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(opened) = answered else {
            panic!("the window root answers every window asked for: {answered:?}");
        };
        if let Opened::Window { window, .. } = opened {
            turn.to(path::<Window>(window)).send(&Subscribe)?;
        }
        self.note(Noted::Opened(opened));

        Ok(())
    }
}

impl Handles<Answer<Closed, Asked>> for Asker {
    type Sends = ();

    fn handle(&mut self, (answered, _): (Answered<Closed>, Asked), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let Answered::Yes(closed) = answered else {
            panic!("the window root answers every window asked to close: {answered:?}");
        };
        self.note(Noted::Closed(closed));

        Ok(())
    }
}

impl Handles<Event> for Asker {
    type Sends = ();

    fn handle(&mut self, event: Event, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.note(Noted::Heard(event));

        Ok(())
    }
}

/// A ready engine with the window root, `setup` windows opened as it was set up, each of `surface`, its close buttons
/// closing as `closing` says, and what its host keeps.
fn windowed(setup: u64, surface: Surface, closing: Closing) -> (Engine, Hosting, Vec<WindowFeed>) {
    let mut made = self::setup();
    let mut opening = Opening::new(made.source::<Heard>(), made.controller::<Windowing>());
    let feeds = (0..setup).map(|_| opening.open(surface, false)).collect();
    let (mut config, hosting) = opening.config();
    config.closing = closing;
    let Ok(engine) = made.add_root::<Windows>(&(), config).map(Setup::ready) else {
        panic!("the window root begins");
    };

    (engine, hosting, feeds)
}

/// Adds the asker of `test`, which asks to open `opens` windows, then to close each of `closes`.
fn ask(engine: &mut Engine, test: u64, opens: u8, closes: Vec<WindowId>) {
    let Ok(_) = engine.add_root::<Asker>(&test, Script { test, opens, closes }) else {
        panic!("the asker begins");
    };
}

/// The window each request taken opens, or closes.
fn windows_of(requests: &[Windowing]) -> Vec<(bool, WindowId)> {
    requests
        .iter()
        .map(|request| match request {
            Windowing::Open { window, .. } => (true, *window),
            Windowing::Close { window } => (false, *window),
        })
        .collect()
}

#[test]
fn windows_asked_for_are_given_ids_after_those_set_up_in_the_order_asked_and_never_twice() {
    // Catches the root counting ids from zero, over the windows opened as the program was set up, a request given an
    // id another was, as one the host did not open being given again, and requests pushed to the host out of the
    // order the root's turns asked them, or the host's answer matched to the wrong request.
    let surface = Surface { width: 640, height: 480, scale: 1.0 };
    let (mut engine, hosting, _feeds) = windowed(2, surface, Closing::ByButton);
    ask(&mut engine, 1, 3, Vec::new());
    run(&mut engine);

    let asked = hosting.take();
    assert_eq!(windows_of(&asked), [(true, window(2)), (true, window(3)), (true, window(4))]);
    let failed = Unopened::Failed(String::from("no window"));
    let answered = [
        hosting.unopened(window(3), failed.clone()),
        hosting.opened(window(2), surface, true).map(drop),
        hosting.opened(window(4), surface, false).map(drop),
    ];
    assert!(answered.iter().all(Result::is_ok), "the window root holds its source");
    run(&mut engine);
    ask(&mut engine, 2, 1, Vec::new());
    run(&mut engine);

    assert_eq!(windows_of(&hosting.take()), [(true, window(5))]);
    let opened = noted(1).into_iter().filter(|noted| matches!(noted, Noted::Opened(_))).collect::<Vec<_>>();
    assert_eq!(
        opened,
        [
            Noted::Opened(Opened::Refused(failed)),
            Noted::Opened(Opened::Window { window: window(2), surface }),
            Noted::Opened(Opened::Window { window: window(4), surface }),
        ]
    );
}

#[test]
fn a_window_opened_on_request_has_its_actor_before_anything_it_heard_reaches_it() {
    // Catches the root answering, or the host's events for a window being sent on, before the window's actor is made:
    // a resize pushed straight after the window opened would reach no actor and be lost, and the asker would hear the
    // window as it opened, not as it stands.
    let small = Surface { width: 640, height: 480, scale: 1.0 };
    let large = Surface { width: 1280, height: 960, scale: 2.0 };
    let (mut engine, hosting, _feeds) = windowed(1, small, Closing::ByButton);
    ask(&mut engine, 3, 1, Vec::new());
    run(&mut engine);

    let Ok(feed) = hosting.opened(window(1), small, false) else {
        panic!("the window root holds its source");
    };
    assert_eq!(feed.window(), window(1));
    assert_eq!(feed.push(Event::Resized(large)), Ok(()));
    run(&mut engine);

    let expected = [
        Noted::Opened(Opened::Window { window: window(1), surface: small }),
        Noted::Heard(Event::Resized(large)),
        Noted::Heard(Event::FocusLost),
    ];
    assert_eq!(noted(3), expected);
}

#[test]
fn closing_a_window_not_open_is_refused_and_one_open_closes_once_its_host_has() {
    // Catches a close of a window never given, or still opening, pushed to the host, which would close a window no
    // actor stands for; two closes of one window pushed twice; an asker answered before the host has closed it; a
    // subscriber not sent `Closed` last, or sent what the window heard after; and a window closed once still open to a
    // close asked after.
    let surface = Surface { width: 640, height: 480, scale: 1.0 };
    let (mut engine, hosting, feeds) = windowed(1, surface, Closing::ByButton);
    ask(&mut engine, 4, 1, vec![window(1), window(9)]);
    run(&mut engine);
    assert_eq!(windows_of(&hosting.take()), [(true, window(1))]);
    let Ok(feed) = hosting.opened(window(1), surface, true) else {
        panic!("the window root holds its source");
    };
    run(&mut engine);

    ask(&mut engine, 5, 0, vec![window(1), window(1)]);
    run(&mut engine);
    assert_eq!(windows_of(&hosting.take()), [(false, window(1))]);
    assert!(noted(5).is_empty(), "no close is answered before the host has closed the window");
    let pushed = [hosting.closed(window(1)), feed.push(Event::FocusLost)];
    assert!(pushed.iter().all(Result::is_ok), "the window root holds its source");
    run(&mut engine);
    ask(&mut engine, 6, 0, vec![window(1)]);
    run(&mut engine);

    let refused = Noted::Closed(Closed::Refused(Unclosed::NotOpen));
    let opened = Noted::Opened(Opened::Window { window: window(1), surface });
    let expected = [
        refused.clone(),
        refused.clone(),
        opened,
        Noted::Heard(Event::Resized(surface)),
        Noted::Heard(Event::FocusGained),
    ];
    assert_eq!(noted(4)[..5], expected);
    assert_eq!(noted(4)[5..], [Noted::Heard(Event::Closed)]);
    assert_eq!(noted(5), [Noted::Closed(Closed::Closed), Noted::Closed(Closed::Closed)]);
    assert_eq!(noted(6), [refused]);
    assert!(hosting.take().is_empty(), "a window closed is asked to close no more");
    drop(feeds);
}

#[test]
fn the_close_button_closes_its_window_by_button_and_only_asks_by_program() {
    // Catches `ByProgram` closing a window on its button, which a program that confirms first could not stop, and
    // `ByButton` not closing it, which would leave a program that handles nothing unclosable; and a window closed by
    // its button asked to close again as the program asks too.
    let surface = Surface { width: 640, height: 480, scale: 1.0 };
    for (closing, expected) in [(Closing::ByProgram, Vec::new()), (Closing::ByButton, vec![(false, window(0))])] {
        let (mut engine, hosting, feeds) = windowed(1, surface, closing);
        let Some(feed) = feeds.first() else {
            panic!("one window was opened");
        };
        assert_eq!(feed.push(Event::CloseRequested), Ok(()));
        assert_eq!(feed.push(Event::CloseRequested), Ok(()));
        run(&mut engine);

        assert_eq!(windows_of(&hosting.take()), expected, "{closing:?}");
    }
}
