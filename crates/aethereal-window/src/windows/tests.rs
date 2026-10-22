//! The window root's tests: what a host pushes for a window reaches that window's subscribers alone, in order, once it
//! says something new, after how the window stood, and nothing another actor sent reaches them.

use super::*;
use crate::event::{Button, Subscribe, Surface};
use crate::host::Opening;
use aethereal_actor::{Path, Tells, Time, path};
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
        turn.to(path::<Windows>(())).send(&Heard { window, event: FORGED })?;

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

#[test]
fn a_subscriber_hears_its_window_as_it_stood_then_what_its_host_pushed_that_says_something_new() {
    // Catches the window root sending an event to the wrong window's actor or to every window's, an event another actor
    // sent the root or the window sent on as though the host heard it, a subscriber not sent how the window stood or
    // sent it twice for subscribing twice, a resize to the size the window has or a focus it has sent on, a turn of the
    // wheel dropped as though it said nothing new, and a window's actor not made before a root made after the window
    // root begins, so a subscription in its `begin` is lost.
    let Ok(mut setup) = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).with_cycle::<()>() else {
        panic!("the cycle is in order");
    };
    let small = Surface { width: 640, height: 480, scale: 1.0 };
    let large = Surface { width: 1280, height: 960, scale: 2.0 };
    let mut opening = Opening::new(setup.source::<Heard>());
    let first = opening.open(small, true);
    let second = opening.open(small, false);
    let Ok(mut engine) = setup
        .add_root::<Windows>(&(), opening.config())
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
