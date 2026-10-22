//! The controls' tests: what a control element reads as, and that the controls root sends a subscriber every control
//! as it stands and then what the page pushed, in order, and nothing another actor sent it.

use super::read::value_of;
use super::*;
use crate::{Browser, Mappings};
use aethereal_actor::{Path, Tells, path};
use aethereal_substrate::{Clock, Engine, Setup};
use std::num::NonZeroUsize;
use std::sync::Mutex;

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

/// The control named `name` holding the number `value`.
fn number(name: &str, value: f64) -> Control {
    Control { name: String::from(name), value: Value::Number(value) }
}

/// An empty config.
#[derive(Plain)]
#[kind(name = "aether.test.listener_config")]
struct Nothing;

/// What the listener was sent, in order.
static HEARD: Mutex<Vec<Control>> = Mutex::new(Vec::new());

/// An actor that subscribes to the controls root, sends it a control of its own, and notes each control it is sent.
#[derive(Bound)]
#[kind(name = "aether.test.listener")]
struct Listener;

impl ChildOf for Listener {
    type Parent = Top;
}

impl Actor for Listener {
    type Config = Nothing;
    type Key = ();
    type Handles = (Control,);
    type BeginSends = (Tells<Path<Controls>, Subscribe>, Tells<Path<Controls>, Control>);
    type EndSends = ();

    fn begin(_: Nothing, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.to(path::<Controls>(())).send(&Subscribe)?.send(&number("forged", -1.0))?;

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Control> for Listener {
    type Sends = ();

    fn handle(&mut self, control: Control, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        HEARD.lock().map(|mut heard| heard.push(control)).ok();

        Ok(())
    }
}

#[test]
fn a_subscriber_is_sent_what_the_page_pushed_in_the_order_pushed_and_nothing_another_actor_sent() {
    // Catches a controls root that sends on or keeps a control any actor mails it as though the page held it, one
    // that sends a subscriber nothing or sends it twice, controls reordered among themselves, and a control the page
    // read before its listener subscribed never heard by it.
    let Ok(mut setup) = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).with_cycle::<()>() else {
        panic!("the cycle is in order");
    };
    let (source, feed) = setup.source::<Control>();
    let Ok(mut engine) = setup
        .add_root::<Controls>(&(), ControlsConfig { source })
        .and_then(|setup| setup.add_root::<Listener>(&(), Nothing))
        .map(Setup::ready)
    else {
        panic!("the controls root and its listener begin");
    };
    let read = number("speed", 0.25);
    assert_eq!(feed.push(read.clone()), Ok(()));
    let mappings = Mappings::unasked(&mut engine);
    let mut browser = Browser::new(engine, Still, mappings);
    browser.frame(16.0);

    let pushed = [number("speed", 0.5), Control { name: String::from("shown"), value: Value::Checked(true) }];
    for control in pushed.clone() {
        assert_eq!(feed.push(control), Ok(()));
    }
    browser.frame(32.0);

    let Ok(heard) = HEARD.lock().map(|heard| heard.clone()) else {
        panic!("what was heard is not poisoned");
    };
    // The control read before the listener subscribed is heard first, once, whichever the controls root handled first.
    let [moved, shown] = pushed;
    assert_eq!(heard, [read, moved, shown]);
}

/// What tells the late listener to subscribe, brought from its own source.
#[derive(Plain)]
#[kind(name = "aether.test.nudge")]
struct Nudge;

/// The source the late listener is nudged through.
#[derive(Bound)]
#[kind(name = "aether.test.late_config")]
struct LateConfig {
    nudges: Given<Source<Nudge>>,
}

/// What the late listener was sent, in order.
static HEARD_LATE: Mutex<Vec<Control>> = Mutex::new(Vec::new());

/// An actor that subscribes to the controls root twice when it is nudged, and notes each control it is sent.
#[derive(Bound)]
#[kind(name = "aether.test.late")]
struct Late {
    nudges: Handle<Source<Nudge>>,
}

impl ChildOf for Late {
    type Parent = Top;
}

impl Actor for Late {
    type Config = LateConfig;
    type Key = ();
    type Handles = (Nudge, Control);
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
    type Sends = (Tells<Path<Controls>, Subscribe>,);

    fn handle(&mut self, _: Nudge, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.to(path::<Controls>(())).send(&Subscribe)?.send(&Subscribe)?;

        Ok(())
    }
}

impl Handles<Control> for Late {
    type Sends = ();

    fn handle(&mut self, control: Control, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        HEARD_LATE.lock().map(|mut heard| heard.push(control)).ok();

        Ok(())
    }
}

#[test]
fn an_actor_that_subscribes_after_a_control_last_moved_is_sent_it_as_it_stands_once() {
    // Catches a controls root that keeps no record of its controls, or does not send it to a new subscriber, so an
    // actor never knows a slider until it moves, one that keeps the first value of a control and not its last, and
    // one that sends it again to an actor that subscribes twice.
    let Ok(mut setup) = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).with_cycle::<()>() else {
        panic!("the cycle is in order");
    };
    let (source, feed) = setup.source::<Control>();
    let (nudges, nudge) = setup.source::<Nudge>();
    let Ok(mut engine) = setup
        .add_root::<Controls>(&(), ControlsConfig { source })
        .and_then(|setup| setup.add_root::<Late>(&(), LateConfig { nudges }))
        .map(Setup::ready)
    else {
        panic!("the controls root and the late listener begin");
    };
    let mappings = Mappings::unasked(&mut engine);
    let mut browser = Browser::new(engine, Still, mappings);
    let moved = number("speed", 0.75);
    let later = number("speed", 1.0);

    let pushes = [feed.push(number("speed", 0.25)), feed.push(moved.clone())];
    assert_eq!(pushes, [Ok(()), Ok(())]);
    browser.frame(48.0);
    assert_eq!(nudge.push(Nudge), Ok(()));
    browser.frame(64.0);
    assert_eq!(feed.push(later.clone()), Ok(()));
    browser.frame(80.0);

    let Ok(heard) = HEARD_LATE.lock().map(|heard| heard.clone()) else {
        panic!("what was heard is not poisoned");
    };
    assert_eq!(heard, [moved, later]);
}
