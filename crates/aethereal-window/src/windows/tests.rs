//! The window root's tests: what a host pushes for a window reaches that window's subscribers alone, in order, once it
//! says something new, after how the window stood, and nothing another actor sent reaches them; a window asked for
//! while the program runs is given the next id, is answered once its host has said, and closes as the root is asked,
//! or as its close button says; and a mode asked for is set once its host has, or at once when the window has it.

use super::*;
use crate::event::{Button, Mode, Subscribe, Surface};
use crate::host::{Hosting, Opening, WindowFeed};
use aethereal_actor::{Answer, Answered, Asks, Path, Tells, Time, path};
use aethereal_data::{ActorPath, Kind, PathSegment};
use aethereal_substrate::harness::Turned;
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
/// events it is sent are read from the turns the engine recorded.
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

/// What a turn was handed, as a test reads it from the engine's record (ADR-0036): an answer to what its actor asked,
/// or an event of a window.
#[derive(Debug, Clone, PartialEq)]
enum Noted {
    Opened(Opened),
    Closed(Closed),
    ModeSet(ModeSet),
    Heard(Event),
}

impl Noted {
    /// What `turned` was handed: nothing for mail no test reads.
    fn of(turned: &Turned) -> Option<Self> {
        if let Ok(opened) = turned.read() {
            return Some(Self::Opened(opened));
        }
        if let Ok(closed) = turned.read() {
            return Some(Self::Closed(closed));
        }
        if let Ok(set) = turned.read() {
            return Some(Self::ModeSet(set));
        }

        turned.read().ok().map(Self::Heard)
    }
}

/// The path of the asker of `test`.
fn asker(test: u64) -> ActorPath {
    ActorPath::from_root(&PathSegment::new(Asker::NAME, &test))
}

/// What a test has read of the engine's record so far: a test reads an asker more than once, and a turn is taken once.
#[derive(Default)]
struct Notes {
    /// What each turn that read as anything was handed, beside the path of its actor, in the order they ran.
    noted: Vec<(ActorPath, Noted)>,
}

impl Notes {
    /// Runs `engine` as [`run`] does, and keeps what its turns were handed.
    fn run(&mut self, engine: &mut Engine) {
        run(engine);
        for turned in engine.take_turned() {
            let noted = Noted::of(&turned);
            self.noted.extend(noted.map(|noted| (turned.path, noted)));
        }
    }

    /// What the asker of `test` was handed, in order.
    fn of(&self, test: u64) -> Vec<Noted> {
        let asker = asker(test);

        self.noted.iter().filter(|(of, _)| *of == asker).map(|(_, noted)| noted.clone()).collect()
    }

    /// What the asker of `test` was answered to the modes it asked for, in order.
    fn modes_set(&self, test: u64) -> Vec<ModeSet> {
        self.of(test)
            .into_iter()
            .filter_map(|noted| match noted {
                Noted::ModeSet(set) => Some(set),
                Noted::Opened(_) | Noted::Closed(_) | Noted::Heard(_) => None,
            })
            .collect()
    }

    /// What the asker of `test` heard of the windows it subscribed to, in order.
    fn heard_by(&self, test: u64) -> Vec<Event> {
        self.of(test)
            .into_iter()
            .filter_map(|noted| match noted {
                Noted::Heard(event) => Some(event),
                Noted::Opened(_) | Noted::Closed(_) | Noted::ModeSet(_) => None,
            })
            .collect()
    }
}

/// What the asker of `test` does as it begins: subscribes to each window of `listens`, then asks the window root to
/// open `opens` windows, to set each mode of `sets`, then to close each of `closes`.
#[derive(Plain, Default)]
#[kind(name = "aether.test.script")]
struct Script {
    test: u64,
    listens: Vec<WindowId>,
    opens: u8,
    sets: Vec<SetMode>,
    closes: Vec<WindowId>,
}

/// An empty context.
#[derive(Plain)]
#[kind(name = "aether.test.asked")]
struct Asked;

/// An actor, keyed by the test it is of, that asks the window root what its script says, and subscribes to each window
/// it listens to or is answered with. What it asks and hears is the record's to say: it stays an actor because only an
/// actor that handles [`Event`] can subscribe to a window, and the order of its answers among its events is what these
/// tests read.
#[derive(Bound)]
#[kind(name = "aether.test.asker")]
struct Asker;

impl ChildOf for Asker {
    type Parent = Top;
}

impl Actor for Asker {
    type Config = Script;
    type Key = u64;
    type Handles = (Answer<Opened, Asked>, Answer<Closed, Asked>, Answer<ModeSet, Asked>, Event);
    type BeginSends = (
        Tells<Path<Window>, Subscribe>,
        Asks<Path<Windows>, OpenWindow, Asked>,
        Asks<Path<Windows>, SetMode, Asked>,
        Asks<Path<Windows>, CloseWindow, Asked>,
    );
    type EndSends = ();

    fn begin(script: Script, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        for window in script.listens {
            turn.to(path::<Window>(window)).send(&Subscribe)?;
        }
        let open = OpenWindow { title: String::from("asked"), size: Logical { width: 640.0, height: 480.0 } };
        for _ in 0..script.opens {
            turn.to(path::<Windows>(())).ask(&open, &Asked)?;
        }
        for set in script.sets {
            turn.to(path::<Windows>(())).ask(&set, &Asked)?;
        }
        for window in script.closes {
            turn.to(path::<Windows>(())).ask(&CloseWindow { window }, &Asked)?;
        }

        Ok(Self)
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

        Ok(())
    }
}

impl Handles<Answer<Closed, Asked>> for Asker {
    type Sends = ();

    fn handle(&mut self, (answered, _): (Answered<Closed>, Asked), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let Answered::Yes(_) = answered else {
            panic!("the window root answers every window asked to close: {answered:?}");
        };

        Ok(())
    }
}

impl Handles<Answer<ModeSet, Asked>> for Asker {
    type Sends = ();

    fn handle(&mut self, (answered, _): (Answered<ModeSet>, Asked), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let Answered::Yes(_) = answered else {
            panic!("the window root answers every mode asked for: {answered:?}");
        };

        Ok(())
    }
}

impl Handles<Event> for Asker {
    type Sends = ();

    fn handle(&mut self, _: Event, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// A ready engine that records its turns, with the window root, `setup` windows opened as it was set up, each of
/// `surface`, its close buttons closing as `closing` says, and what its host keeps.
fn windowed(setup: u64, surface: Surface, closing: Closing) -> (Engine, Hosting, Vec<WindowFeed>) {
    let mut made = self::setup();
    let mut opening = Opening::new(made.source::<Heard>(), made.controller::<Windowing>());
    let feeds = (0..setup).map(|_| opening.open(surface, Mode::Windowed, false)).collect();
    let (mut config, hosting) = opening.config();
    config.closing = closing;
    let Ok(mut engine) = made.add_root::<Windows>(&(), config).map(Setup::ready) else {
        panic!("the window root begins");
    };
    engine.record_turns();

    (engine, hosting, feeds)
}

/// Adds the asker of `test`, which asks to open `opens` windows, then to close each of `closes`.
fn ask(engine: &mut Engine, test: u64, opens: u8, closes: Vec<WindowId>) {
    script(engine, Script { test, opens, closes, ..Script::default() });
}

/// Adds the asker of the test `script` is of, which does as it says.
fn script(engine: &mut Engine, script: Script) {
    let test = script.test;
    let Ok(_) = engine.add_root::<Asker>(&test, script) else {
        panic!("the asker begins");
    };
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
fn windows_asked_for_are_given_ids_after_those_set_up_in_the_order_asked_and_never_twice() {
    // Catches the root counting ids from zero, over the windows opened as the program was set up, a request given an
    // id another was, as one the host did not open being given again, and requests pushed to the host out of the
    // order the root's turns asked them, or the host's answer matched to the wrong request.
    let surface = Surface { width: 640, height: 480, scale: 1.0 };
    let (mut engine, hosting, _feeds) = windowed(2, surface, Closing::ByButton);
    let mut notes = Notes::default();
    ask(&mut engine, 1, 3, Vec::new());
    notes.run(&mut engine);

    let asked = hosting.take();
    assert_eq!(windows_of(&asked), [Took::Open(window(2)), Took::Open(window(3)), Took::Open(window(4))]);
    let failed = Unopened::Failed(String::from("no window"));
    let answered = [
        hosting.unopened(window(3), failed.clone()),
        hosting.opened(window(2), surface, true).map(drop),
        hosting.opened(window(4), surface, false).map(drop),
    ];
    assert!(answered.iter().all(Result::is_ok), "the window root holds its source");
    notes.run(&mut engine);
    ask(&mut engine, 2, 1, Vec::new());
    notes.run(&mut engine);

    assert_eq!(windows_of(&hosting.take()), [Took::Open(window(5))]);
    let opened = notes.of(1).into_iter().filter(|noted| matches!(noted, Noted::Opened(_))).collect::<Vec<_>>();
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
    let mut notes = Notes::default();
    ask(&mut engine, 3, 1, Vec::new());
    notes.run(&mut engine);

    let Ok(feed) = hosting.opened(window(1), small, false) else {
        panic!("the window root holds its source");
    };
    assert_eq!(feed.window(), window(1));
    assert_eq!(feed.push(Event::Resized(large)), Ok(()));
    notes.run(&mut engine);

    let expected = [
        Noted::Opened(Opened::Window { window: window(1), surface: small }),
        Noted::Heard(Event::Resized(large)),
        Noted::Heard(Event::Moded(Mode::Windowed)),
        Noted::Heard(Event::FocusLost),
    ];
    assert_eq!(notes.of(3), expected);
}

#[test]
fn closing_a_window_not_open_is_refused_and_one_open_closes_once_its_host_has() {
    // Catches a close of a window never given, or still opening, pushed to the host, which would close a window no
    // actor stands for; two closes of one window pushed twice; an asker answered before the host has closed it; a
    // subscriber not sent `Closed` last, or sent what the window heard after; and a window closed once still open to a
    // close asked after.
    let surface = Surface { width: 640, height: 480, scale: 1.0 };
    let (mut engine, hosting, feeds) = windowed(1, surface, Closing::ByButton);
    let mut notes = Notes::default();
    ask(&mut engine, 4, 1, vec![window(1), window(9)]);
    notes.run(&mut engine);
    assert_eq!(windows_of(&hosting.take()), [Took::Open(window(1))]);
    let Ok(feed) = hosting.opened(window(1), surface, true) else {
        panic!("the window root holds its source");
    };
    notes.run(&mut engine);

    ask(&mut engine, 5, 0, vec![window(1), window(1)]);
    notes.run(&mut engine);
    assert_eq!(windows_of(&hosting.take()), [Took::Close(window(1))]);
    assert!(notes.of(5).is_empty(), "no close is answered before the host has closed the window");
    let pushed = [hosting.closed(window(1)), feed.push(Event::FocusLost)];
    assert!(pushed.iter().all(Result::is_ok), "the window root holds its source");
    notes.run(&mut engine);
    ask(&mut engine, 6, 0, vec![window(1)]);
    notes.run(&mut engine);

    let refused = Noted::Closed(Closed::Refused(Unclosed::NotOpen));
    let opened = Noted::Opened(Opened::Window { window: window(1), surface });
    let expected = [
        refused.clone(),
        refused.clone(),
        opened,
        Noted::Heard(Event::Resized(surface)),
        Noted::Heard(Event::Moded(Mode::Windowed)),
        Noted::Heard(Event::FocusGained),
    ];
    assert_eq!(notes.of(4)[..6], expected);
    assert_eq!(notes.of(4)[6..], [Noted::Heard(Event::Closed)]);
    assert_eq!(notes.of(5), [Noted::Closed(Closed::Closed), Noted::Closed(Closed::Closed)]);
    assert_eq!(notes.of(6), [refused]);
    assert!(hosting.take().is_empty(), "a window closed is asked to close no more");
    drop(feeds);
}

#[test]
fn the_close_button_closes_its_window_by_button_and_only_asks_by_program() {
    // Catches `ByProgram` closing a window on its button, which a program that confirms first could not stop, and
    // `ByButton` not closing it, which would leave a program that handles nothing unclosable; and a window closed by
    // its button asked to close again as the program asks too.
    let surface = Surface { width: 640, height: 480, scale: 1.0 };
    for (closing, expected) in [(Closing::ByProgram, Vec::new()), (Closing::ByButton, vec![Took::Close(window(0))])] {
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

#[test]
fn a_mode_asked_for_is_set_once_its_host_has_and_one_the_window_has_is_answered_at_once() {
    // Catches a mode the window is shown in pushed to the host, or answered with an event, when nothing would change;
    // a mode asked of a window not open pushed to the host; a request answered before the host has said, or matched
    // to the wrong one of two the host answers in order; a second request answered at once from the mode the window
    // had while the first, which changes it, is unanswered; and the root answering from the mode and surface the
    // window opened with rather than those its host has since pushed.
    let small = Surface { width: 640, height: 480, scale: 1.0 };
    let large = Surface { width: 1920, height: 1080, scale: 1.0 };
    let (mut engine, hosting, feeds) = windowed(1, small, Closing::ByButton);
    let mut notes = Notes::default();
    let fullscreen = SetMode { window: window(0), mode: Mode::Fullscreen };
    let sets = vec![
        SetMode { window: window(0), mode: Mode::Windowed },
        fullscreen,
        SetMode { window: window(0), mode: Mode::Windowed },
        SetMode { window: window(9), mode: Mode::Fullscreen },
    ];
    script(&mut engine, Script { test: 7, listens: vec![window(0)], sets, ..Script::default() });
    notes.run(&mut engine);

    let expected = [Took::SetMode(window(0), Mode::Fullscreen), Took::SetMode(window(0), Mode::Windowed)];
    assert_eq!(windows_of(&hosting.take()), expected);
    let at_once = [ModeSet::Set { surface: small }, ModeSet::Refused(Unset::NotOpen)];
    assert_eq!(notes.modes_set(7), at_once, "no request is answered before the host has said");
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
    notes.run(&mut engine);
    script(&mut engine, Script { test: 8, sets: vec![fullscreen], ..Script::default() });
    notes.run(&mut engine);

    let answered = [ModeSet::Set { surface: large }, ModeSet::Refused(Unset::NoHost)];
    assert_eq!(notes.modes_set(7), [&at_once[..], &answered[..]].concat());
    let heard = [
        Event::Resized(small),
        Event::Moded(Mode::Windowed),
        Event::FocusLost,
        Event::Moded(Mode::Fullscreen),
        Event::Resized(large),
    ];
    assert_eq!(notes.heard_by(7), heard);
    assert_eq!(notes.modes_set(8), [ModeSet::Set { surface: large }]);
    assert!(hosting.take().is_empty(), "a mode the window has is asked of the host no more");
}

#[test]
fn a_window_that_closes_refuses_the_modes_asked_of_it_as_not_open() {
    // Catches a mode asked of a window that is closing pushed to the host, which would show a window about to close;
    // and a request to set the mode of a window that closed before its host answered it dropped, so its asker hears
    // only that it went unanswered, or answered as set.
    let surface = Surface { width: 640, height: 480, scale: 1.0 };
    let (mut engine, hosting, _feeds) = windowed(1, surface, Closing::ByButton);
    let mut notes = Notes::default();
    let maximized = SetMode { window: window(0), mode: Mode::Maximized };
    script(&mut engine, Script { test: 9, sets: vec![maximized], closes: vec![window(0)], ..Script::default() });
    notes.run(&mut engine);
    script(&mut engine, Script { test: 10, sets: vec![maximized], ..Script::default() });
    notes.run(&mut engine);

    let expected = [Took::SetMode(window(0), Mode::Maximized), Took::Close(window(0))];
    assert_eq!(windows_of(&hosting.take()), expected);
    assert_eq!(notes.modes_set(10), [ModeSet::Refused(Unset::NotOpen)]);
    assert_eq!(hosting.closed(window(0)), Ok(()));
    notes.run(&mut engine);

    let expected = [Noted::ModeSet(ModeSet::Refused(Unset::NotOpen)), Noted::Closed(Closed::Closed)];
    assert_eq!(notes.of(9), expected);
}
