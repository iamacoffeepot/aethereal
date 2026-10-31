//! The controls' tests: what a control element reads as, and that the controls root sends a subscriber every control
//! as it stands and then what the page pushed, in order, and nothing another actor sent it. The outside is that
//! subscriber, and that other actor.

use super::read::value_of;
use super::*;
use crate::{Browser, Hosted, Serving};
use aethereal_actor::{Actors, Refused, Seated, Tie};
use aethereal_data::{Encode, Kind};
use aethereal_substrate::{Clock, Engine, Feed};
use std::num::NonZeroUsize;

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

/// A browser over a page's controls, and what a test reaches them through.
struct Page {
    browser: Browser<Still>,
    outside: Seated,
    controls: Seated,
    feed: Feed<Control>,
}

impl Page {
    /// A browser with the controls root, and the outside seated.
    fn new() -> Result<Self, Refused> {
        let mut setup = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).with_cycle::<()>();
        let (source, feed) = setup.source::<Control>();
        let mut engine = setup.ready();
        let id = engine.add_root::<Controls>(&(), ControlsConfig { source })?;
        let Some(seat) = engine.table().seat(id) else {
            panic!("the controls root is seated");
        };
        let (outside, _arrivals) = engine.seat_outside()?;
        let browser = Browser::new(engine, Still, Serving::Nothing, Hosted::Nothing);

        Ok(Self { browser, outside, controls: Seated { id, seat }, feed })
    }

    /// Sends `mail` to the controls root from the outside: mail from an actor that is not the root itself.
    fn tell<K: Kind + Encode<Actors>>(&mut self, mail: &K) {
        let Ok(_) = self.browser.engine.send_outside(self.outside, self.controls, mail, |_| Tie::TELL) else {
            panic!("what the test tells encodes");
        };
    }

    /// The controls the outside was handed in the turns the engine recorded (ADR-0036), in order.
    fn heard(&mut self) -> Vec<Control> {
        let turned = self.browser.engine.take_turned();

        turned.iter().filter(|turn| turn.actor == self.outside).filter_map(|turn| turn.read().ok()).collect()
    }
}

#[test]
fn a_subscriber_is_sent_what_the_page_pushed_in_the_order_pushed_and_nothing_another_actor_sent() -> Result<(), Refused>
{
    // Catches a controls root that sends on or keeps a control any actor mails it as though the page held it, one
    // that sends a subscriber nothing or sends it twice, controls reordered among themselves, and a control the page
    // read before its listener subscribed never heard by it.
    let mut page = Page::new()?;
    let forged = number("forged", -1.0);
    page.tell(&forged);
    page.tell(&Subscribe);
    page.tell(&forged);
    let read = number("speed", 0.25);
    assert_eq!(page.feed.push(read.clone()), Ok(()));
    page.browser.frame(16.0);

    let pushed = [number("speed", 0.5), Control { name: String::from("shown"), value: Value::Checked(true) }];
    for control in pushed.clone() {
        assert_eq!(page.feed.push(control), Ok(()));
    }
    page.browser.frame(32.0);

    // The control read before the listener subscribed is heard first, once, whichever the controls root handled first.
    let [moved, shown] = pushed;
    assert_eq!(page.heard(), [read, moved, shown]);

    Ok(())
}

#[test]
fn an_actor_that_subscribes_after_a_control_last_moved_is_sent_it_as_it_stands_once() -> Result<(), Refused> {
    // Catches a controls root that keeps no record of its controls, or does not send it to a new subscriber, so an
    // actor never knows a slider until it moves, one that keeps the first value of a control and not its last, and
    // one that sends it again to an actor that subscribes twice.
    let mut page = Page::new()?;
    let moved = number("speed", 0.75);
    let later = number("speed", 1.0);

    let pushes = [page.feed.push(number("speed", 0.25)), page.feed.push(moved.clone())];
    assert_eq!(pushes, [Ok(()), Ok(())]);
    page.browser.frame(48.0);
    page.tell(&Subscribe);
    page.tell(&Subscribe);
    page.browser.frame(64.0);
    assert_eq!(page.feed.push(later.clone()), Ok(()));
    page.browser.frame(80.0);

    assert_eq!(page.heard(), [moved, later]);

    Ok(())
}
