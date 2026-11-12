//! The cascade's tests: which entries each pointer event reaches through the stacks a frame drew, and when; and, in
//! an engine, whom a window's actor probes for and whose stacks it delivers.

use super::*;
use crate::event::{Mode, Surface};
use crate::host::{Opening, WindowFeed};
use crate::keys::KeysListed;
use crate::window::Window;
use crate::windows::{Heard, Windowing, Windows};
use aethereal_actor::{
    Actor, ActorRef, Beginning, ChildOf, Ending, Fault, Handles, Path, SeatNumber, Seated, Table, Tells, Top, Turn,
    path,
};
use aethereal_data::{ActorId, ActorPath, Kind, KindId, PathSegment};
use aethereal_substrate::Engine;
use aethereal_substrate::harness::{Bench, Harness, Steps, Unheard};
use std::num::NonZeroUsize;

/// The one window.
const WINDOW: WindowId = WindowId::nth(0);

/// The actor numbered `number`, as a stack names it.
fn actor(number: u64) -> ProtocolRef<Hearer> {
    ProtocolRef::found(Seated { id: ActorId::new(number), seat: SeatNumber::new(number) })
}

/// The entry `entry` of `actor`'s list, blocking the pointer from what is beneath it.
fn blocks(actor: ProtocolRef<Hearer>, entry: u32) -> Reach {
    Reach { actor, entry, blocks: true }
}

/// The entry `entry` of `actor`'s list, letting the pointer through to what is beneath it.
fn passes(actor: ProtocolRef<Hearer>, entry: u32) -> Reach {
    Reach { actor, entry, blocks: false }
}

/// What a frame drew under the pointer, `pointer`, of every event up to `through`, with no press probed.
fn under(through: u64, pointer: &[Reach]) -> Picked {
    Picked { through, pointer: pointer.to_vec(), presses: Vec::new() }
}

/// `event` sent to the entry `entry` of `actor`'s list.
fn to(actor: ProtocolRef<Hearer>, entry: u32, event: PointerEvent) -> (ProtocolRef<Hearer>, Pointed) {
    (actor, Pointed { window: WINDOW, entry, event })
}

/// A cascade that heard `events`, in order.
fn heard(events: &[Event]) -> Cascade {
    let mut cascade = Cascade::default();
    for event in events {
        cascade.heard(WINDOW, event);
    }

    cascade
}

const MOVED: Event = Event::PointerMoved { x: 1.0, y: 2.0 };
const PRESSED: Event = Event::PointerDown { x: 1.0, y: 2.0, button: Button::Main };
const RELEASED: Event = Event::PointerUp { x: 1.0, y: 2.0, button: Button::Main };

#[test]
fn a_press_reaches_down_to_the_topmost_entry_that_blocks_and_no_lower() {
    // Catches a press delivered only to the top of the stack, so a tooltip that lets the pointer through hides the
    // button beneath it; and one delivered past an entry that blocks, so a click on the console also clicks the game.
    let (tooltip, button, game) = (actor(1), actor(2), actor(3));
    let mut cascade = heard(&[PRESSED]);
    let stack = [passes(tooltip, 0), blocks(button, 4), blocks(game, 0)];

    let sent = cascade.picked(WINDOW, under(1, &stack));

    let down = PointerEvent::PointerDown { x: 1.0, y: 2.0, button: Button::Main };
    let entered = PointerEvent::PointerEntered;
    let expected = vec![to(tooltip, 0, entered), to(button, 4, entered), to(tooltip, 0, down), to(button, 4, down)];
    assert_eq!(sent, expected);
}

#[test]
fn an_event_waits_for_the_stacks_of_a_frame_drawn_after_it() {
    // Catches an event delivered through a stack drawn before it was heard, which tests the pointer against what was
    // not yet on screen; and one dropped once a frame's stacks come back without it.
    let game = actor(1);
    let mut cascade = heard(&[MOVED, MOVED]);
    let stack = [blocks(game, 0)];

    let first = cascade.picked(WINDOW, under(1, &stack));
    let second = cascade.picked(WINDOW, under(2, &stack));

    let moved = PointerEvent::PointerMoved { x: 1.0, y: 2.0 };
    assert_eq!(first, vec![to(game, 0, PointerEvent::PointerEntered), to(game, 0, moved)]);
    assert_eq!(second, vec![to(game, 0, moved)], "the pointer still reaches the game, and nothing enters again");
}

#[test]
fn the_pointer_crossing_from_one_entry_to_another_leaves_one_and_enters_the_other() {
    // Catches hover told only as the pointer enters, so a highlighted button stays lit once the pointer leaves it; and
    // an entry told it was entered again on every frame the pointer rests on it.
    let (left, right) = (actor(1), actor(2));
    let mut cascade = Cascade::default();

    let entered = cascade.picked(WINDOW, under(0, &[blocks(left, 0)]));
    let rested = cascade.picked(WINDOW, under(0, &[blocks(left, 0)]));
    let crossed = cascade.picked(WINDOW, under(0, &[blocks(right, 1)]));

    assert_eq!(entered, vec![to(left, 0, PointerEvent::PointerEntered)]);
    assert_eq!(rested, vec![]);
    assert_eq!(crossed, vec![to(left, 0, PointerEvent::PointerLeft), to(right, 1, PointerEvent::PointerEntered)]);
}

#[test]
fn a_press_captures_every_move_and_its_release_wherever_the_pointer_goes() {
    // Catches a drag that drops the slider it began on as the pointer leaves it, and a release sent to whatever is
    // under the pointer, so a button pressed and released elsewhere clicks; and a capture kept after the release, so
    // the next move still goes to the slider.
    let (slider, game) = (actor(1), actor(2));
    let mut cascade = heard(&[PRESSED, MOVED, RELEASED, MOVED]);
    cascade.picked(WINDOW, under(1, &[blocks(slider, 3)]));

    let sent = cascade.picked(WINDOW, under(4, &[blocks(game, 0)]));

    let moved = PointerEvent::PointerMoved { x: 1.0, y: 2.0 };
    let up = PointerEvent::PointerUp { x: 1.0, y: 2.0, button: Button::Main };
    let expected = vec![
        to(slider, 3, PointerEvent::PointerLeft),
        to(game, 0, PointerEvent::PointerEntered),
        to(slider, 3, moved),
        to(slider, 3, up),
        to(game, 0, moved),
    ];
    assert_eq!(sent, expected);
}

#[test]
fn a_press_goes_through_the_stack_at_its_own_place_and_the_wheel_through_the_pointer_s() {
    // Catches a press delivered through the stack at where the pointer went after it, within one frame, so a quick
    // click and move presses whatever is beside the button; and a wheel turn sent to a capture, so a list being
    // dragged in scrolls whatever is under the pointer.
    let (button, beside) = (actor(1), actor(2));
    let wheel = Event::Wheel { x: 1.0, y: 2.0, delta_x: 0.0, delta_y: 40.0 };
    let mut cascade = heard(&[PRESSED, MOVED, wheel]);
    let pressed = Pressed { at: 1, stack: vec![blocks(button, 0)] };
    let picked = Picked { through: 3, pointer: vec![blocks(beside, 0)], presses: vec![pressed] };

    let sent = cascade.picked(WINDOW, picked);

    let down = PointerEvent::PointerDown { x: 1.0, y: 2.0, button: Button::Main };
    let moved = PointerEvent::PointerMoved { x: 1.0, y: 2.0 };
    let turned = PointerEvent::Wheel { x: 1.0, y: 2.0, delta_x: 0.0, delta_y: 40.0 };
    let expected = vec![
        to(beside, 0, PointerEvent::PointerEntered),
        to(button, 0, down),
        to(button, 0, moved),
        to(beside, 0, turned),
    ];
    assert_eq!(sent, expected);
}

/// What a test tells the prober to do.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.test.probe_order")]
pub enum ProbeOrder {
    /// Subscribe to the window's probes.
    Subscribe,
    /// Send the window these stacks.
    Pick(Picked),
    /// Send the window these `Keys` entries.
    List(KeysListed),
}

/// An actor that stands where the renderer would: it subscribes to the window's probes, and sends it stacks and the
/// `Keys` entries listed, as it is told; what it is sent is read from the turns the engine recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.prober")]
pub struct Prober;

impl ChildOf for Prober {
    type Parent = Top;
}

impl Actor for Prober {
    type Config = Self;
    type Key = ();
    type Handles = (ProbeOrder, Probed);
    type BeginSends = ();
    type EndSends = ();

    fn begin(prober: Self, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(prober)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<ProbeOrder> for Prober {
    type Sends = (Tells<Path<Window>, SubscribeProbes>, Tells<Path<Window>, Picked>, Tells<Path<Window>, KeysListed>);

    fn handle(&mut self, order: ProbeOrder, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let window = path::<Window>(WINDOW);
        match order {
            ProbeOrder::Subscribe => {
                turn.to(window).send(&SubscribeProbes)?;
            }
            ProbeOrder::Pick(picked) => {
                turn.to(window).send(&picked)?;
            }
            ProbeOrder::List(listed) => {
                turn.to(window).send(&listed)?;
            }
        }

        Ok(())
    }
}

impl Handles<Probed> for Prober {
    type Sends = ();

    fn handle(&mut self, _: Probed, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// An actor whose draw list holds what the pointer reaches: what it is sent is read from the turns recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.widget")]
struct Widget;

impl ChildOf for Widget {
    type Parent = Top;
}

impl Actor for Widget {
    type Config = Self;
    type Key = ();
    type Handles = (Pointed,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(widget: Self, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(widget)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Pointed> for Widget {
    type Sends = ();

    fn handle(&mut self, _: Pointed, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// The actor at `place`, of the type `T`, as the engine's table has it.
pub fn found<T: Kind>(harness: &mut Harness<Bench>, place: &ActorPath) -> Seated {
    harness.host().engine().table().reach(place, KindId::of::<T>())
}

/// A harness over an engine with the window root, one window opened as it was set up, the prober, and the widget,
/// with the window's feed.
fn probed_window() -> Result<(Harness<Bench>, WindowFeed), Unheard> {
    let mut setup = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).with_cycle::<()>();
    let mut opening = Opening::new(setup.source::<Heard>(), setup.controller::<Windowing>());
    let surface = Surface { width: 640, height: 480, scale: 1.0 };
    let feed = opening.open("window", surface, Mode::Windowed, true);
    let (config, _hosting) = opening.config();
    let engine =
        setup.add_root::<Windows>(&(), config)?.add_root::<Prober>(&(), Prober)?.add_root::<Widget>(&(), Widget)?;
    let mut harness = Harness::bare(engine.ready())?;
    harness.host().engine().record_turns();

    Ok((harness, feed))
}

/// What the prober and the widget were sent, settled: each in the order it was sent.
fn sent(harness: &mut Harness<Bench>) -> (Vec<Probed>, Vec<Pointed>) {
    let turns = harness.settle_turns();
    let prober = ActorPath::from_root(&PathSegment::new(Prober::NAME, &()));
    let widget = ActorPath::from_root(&PathSegment::new(Widget::NAME, &()));
    let heard = turns.iter().filter(|turned| turned.path == prober).filter_map(|turned| turned.read().ok()).collect();
    let pointed = turns.iter().filter(|turned| turned.path == widget).filter_map(|turned| turned.read().ok()).collect();

    (heard, pointed)
}

#[test]
fn a_window_probes_its_pointer_for_its_prober_and_delivers_the_stacks_only_it_sends() -> Result<(), Unheard> {
    // Catches a window that probes before anyone asked, keeping events no stack will come back for; a probe that does
    // not say it was a press, so the press is tested where the pointer went after it; and stacks from any actor
    // delivered, so any actor could send the pointer where it chose.
    let (mut harness, feed) = probed_window()?;
    assert_eq!(feed.push(MOVED), Ok(()), "the window root holds its source");
    assert_eq!(sent(&mut harness), (vec![], vec![]), "nobody probes yet");

    let prober = ActorRef::<Prober>::found(found::<Prober>(&mut harness, path::<Prober>(()).names()));
    harness.tell(prober, &ProbeOrder::Subscribe)?;
    harness.settle_turns();
    assert_eq!(feed.push(PRESSED), Ok(()), "the window root holds its source");
    let pressed = Probed { window: WINDOW, at: 1, x: 1.0, y: 2.0, press: true };
    assert_eq!(sent(&mut harness), (vec![pressed], vec![]));

    let widget = ProtocolRef::found(found::<Widget>(&mut harness, path::<Widget>(()).names()));
    let picked = Picked { through: 1, pointer: vec![blocks(widget, 0)], presses: Vec::new() };
    let window = ActorRef::<Window>::found(found::<Window>(&mut harness, path::<Window>(WINDOW).names()));
    harness.tell(window, &picked)?;
    assert_eq!(sent(&mut harness), (vec![], vec![]), "stacks the harness sent are not the prober's");

    harness.tell(prober, &ProbeOrder::Pick(picked))?;
    let down = PointerEvent::PointerDown { x: 1.0, y: 2.0, button: Button::Main };
    let reached = |event| Pointed { window: WINDOW, entry: 0, event };
    assert_eq!(sent(&mut harness), (vec![], vec![reached(PointerEvent::PointerEntered), reached(down)]));

    Ok(())
}
