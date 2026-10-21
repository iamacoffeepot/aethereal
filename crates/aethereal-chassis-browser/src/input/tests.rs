//! The input's tests: what the page's events read as, and that the input root sends a subscriber what the page
//! pushed, in order, and nothing another actor sent it.

use super::dom::{Canvas, Pointing, button_of, value_of};
use super::*;
use crate::Browser;
use aethereal_actor::{Path, Tells, path};
use aethereal_substrate::{Clock, Engine, Setup};
use std::iter;
use std::num::NonZeroUsize;
use std::sync::Mutex;

#[test]
fn a_point_on_the_canvas_is_scaled_to_its_backing_store_on_each_axis() {
    // Catches a scale taken the wrong way up (shown over stored), one axis scaled by the other's ratio, and a canvas
    // shown with no size giving a point no number.
    let wide = Canvas::new(1600, 900, 800.0, 600.0);
    let hidden = Canvas::new(300, 150, 0.0, 0.0);

    assert_eq!(wide.pixel(400.0, 300.0), (800.0, 450.0));
    assert_eq!(wide.pixel(0.0, 0.0), (0.0, 0.0));
    assert_eq!(hidden.pixel(10.0, 20.0), (10.0, 20.0));
}

#[test]
fn the_pages_button_numbers_are_its_buttons_and_a_move_carries_none() {
    // Catches the middle and right buttons swapped, as the page numbers the wheel 1 and the right button 2, a number
    // past the five named taken for one of them, and a press or release that reads no button.
    let named = [0, 1, 2, 3, 4, 7].map(button_of);
    let at = (3.0, 4.0);

    assert_eq!(
        named,
        [Button::Main, Button::Middle, Button::Secondary, Button::Back, Button::Forward, Button::Other(7)]
    );
    assert_eq!(Pointing::Moved.event(at, 2), Event::PointerMoved { x: 3.0, y: 4.0 });
    assert_eq!(Pointing::Down.event(at, 2), Event::PointerDown { x: 3.0, y: 4.0, button: Button::Secondary });
    assert_eq!(Pointing::Up.event(at, 0), Event::PointerUp { x: 3.0, y: 4.0, button: Button::Main });
}

#[test]
fn a_control_holds_a_number_a_check_or_its_text_by_its_type() {
    // Catches a checkbox read by its value, which the page holds as "on" whether or not it is checked, a range read
    // as text, and a number input left empty or holding what reads as no finite number taken for one.
    let text = |value: &str| Value::Text(String::from(value));

    assert_eq!(value_of("range", String::from("0.25"), false), Value::Number(0.25));
    assert_eq!(value_of("checkbox", String::from("on"), false), Value::Checked(false));
    assert_eq!(value_of("radio", String::from("on"), true), Value::Checked(true));
    assert_eq!(value_of("number", String::new(), false), text(""));
    assert_eq!(value_of("number", String::from("inf"), false), text("inf"));
    assert_eq!(value_of("text", String::from("12"), false), text("12"));
}

/// A clock that reads the same time always: the tests' frames take no time.
struct Still;

impl Clock for Still {
    fn nanos(&self) -> u64 {
        0
    }
}

/// An empty config.
#[derive(Plain)]
struct Nothing;

impl Kind for Nothing {
    const NAME: KindName = KindName::new("aether.test.listener_config");
}

/// What the listener was sent, in order.
static HEARD: Mutex<Vec<Event>> = Mutex::new(Vec::new());

/// The move another actor sends the input root, which is not the page's.
const FORGED: Event = Event::PointerMoved { x: -1.0, y: -1.0 };

/// An actor that subscribes to the input root, sends it an event of its own, and notes each event it is sent.
#[derive(Bound)]
struct Listener;

impl Kind for Listener {
    const NAME: KindName = KindName::new("aether.test.listener");
}

impl ChildOf for Listener {
    type Parent = Top;
}

impl Actor for Listener {
    type Config = Nothing;
    type Key = ();
    type Handles = (Event,);
    type BeginSends = (Tells<Path<Input>, Subscribe>, Tells<Path<Input>, Event>);
    type EndSends = ();

    fn begin(_: Nothing, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.to(path::<Input>(())).send(&Subscribe)?.send(&FORGED)?;

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
fn a_subscriber_is_sent_what_the_page_pushed_in_the_order_pushed_and_nothing_another_actor_sent() {
    // Catches an input root that sends on an event any actor mails it as though the page heard it, one that sends a
    // subscriber nothing or sends it twice, pointer, key, and control events reordered among themselves, and a
    // control the page read before its listener subscribed never heard by it.
    let Ok(mut setup) = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).with_cycle::<()>() else {
        panic!("the cycle is in order");
    };
    let (source, feed) = setup.source::<Event>();
    let Ok(engine) = setup
        .add_root::<Input>(&(), InputConfig { source })
        .and_then(|setup| setup.add_root::<Listener>(&(), Nothing))
        .map(Setup::ready)
    else {
        panic!("the input root and its listener begin");
    };
    let read = Event::Control { name: String::from("speed"), value: Value::Number(0.25) };
    let Ok(()) = feed.push(read.clone()) else {
        panic!("the input root holds its source");
    };
    let mut browser = Browser::new(engine, Still);
    browser.frame();

    let pushed = [
        Event::KeyDown { code: String::from("KeyW"), key: String::from("w"), repeat: false },
        Event::PointerMoved { x: 1.0, y: 2.0 },
        Event::Control { name: String::from("speed"), value: Value::Number(0.5) },
        Event::PointerDown { x: 1.0, y: 2.0, button: Button::Main },
        Event::KeyUp { code: String::from("KeyW"), key: String::from("w") },
    ];
    for event in pushed.clone() {
        let Ok(()) = feed.push(event) else {
            panic!("the input root holds its source");
        };
    }
    browser.frame();

    let Ok(heard) = HEARD.lock().map(|heard| heard.clone()) else {
        panic!("what was heard is not poisoned");
    };
    // The control read before the listener subscribed is heard first, once, whichever the input root handled first.
    let expected: Vec<Event> = iter::once(read).chain(pushed).collect();
    assert_eq!(heard, expected);
}

/// What tells the late listener to subscribe, brought from its own source.
#[derive(Plain)]
struct Nudge;

impl Kind for Nudge {
    const NAME: KindName = KindName::new("aether.test.nudge");
}

/// The source the late listener is nudged through.
#[derive(Bound)]
struct LateConfig {
    nudges: Given<Source<Nudge>>,
}

impl Kind for LateConfig {
    const NAME: KindName = KindName::new("aether.test.late_config");
}

/// What the late listener was sent, in order.
static HEARD_LATE: Mutex<Vec<Event>> = Mutex::new(Vec::new());

/// An actor that subscribes to the input root twice when it is nudged, and notes each event it is sent.
#[derive(Bound)]
struct Late {
    nudges: Handle<Source<Nudge>>,
}

impl Kind for Late {
    const NAME: KindName = KindName::new("aether.test.late");
}

impl ChildOf for Late {
    type Parent = Top;
}

impl Actor for Late {
    type Config = LateConfig;
    type Key = ();
    type Handles = (Nudge, Event);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: LateConfig, turn: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        let nudges = turn.take(config.nudges)?;

        Ok(Self { nudges })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Nudge> for Late {
    type Sends = (Tells<Path<Input>, Subscribe>,);

    fn handle(&mut self, _: Nudge, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.to(path::<Input>(())).send(&Subscribe)?.send(&Subscribe)?;

        Ok(())
    }
}

impl Handles<Event> for Late {
    type Sends = ();

    fn handle(&mut self, event: Event, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        HEARD_LATE.lock().map(|mut heard| heard.push(event)).ok();

        Ok(())
    }
}

#[test]
fn an_actor_that_subscribes_after_a_control_last_moved_is_sent_it_as_it_stands_once() {
    // Catches an input root that keeps no record of its controls, or does not send it to a new subscriber, so an actor
    // never knows a slider until it moves, and one that sends it again to an actor that subscribes twice.
    let Ok(mut setup) = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).with_cycle::<()>() else {
        panic!("the cycle is in order");
    };
    let (source, feed) = setup.source::<Event>();
    let (nudges, nudge) = setup.source::<Nudge>();
    let Ok(engine) = setup
        .add_root::<Input>(&(), InputConfig { source })
        .and_then(|setup| setup.add_root::<Late>(&(), LateConfig { nudges }))
        .map(Setup::ready)
    else {
        panic!("the input root and the late listener begin");
    };
    let mut browser = Browser::new(engine, Still);
    let moved = Event::Control { name: String::from("speed"), value: Value::Number(0.75) };
    let pressed = Event::KeyDown { code: String::from("Space"), key: String::from(" "), repeat: false };

    let pushes = [
        feed.push(Event::Control { name: String::from("speed"), value: Value::Number(0.25) }),
        feed.push(moved.clone()),
    ];
    assert_eq!(pushes, [Ok(()), Ok(())]);
    browser.frame();
    assert_eq!(nudge.push(Nudge), Ok(()));
    browser.frame();
    assert_eq!(feed.push(pressed.clone()), Ok(()));
    browser.frame();

    let Ok(heard) = HEARD_LATE.lock().map(|heard| heard.clone()) else {
        panic!("what was heard is not poisoned");
    };
    assert_eq!(heard, [moved, pressed]);
}
