//! The cascade's tests: which entries each pointer event reaches through the stacks a frame drew, and when; and, in
//! an engine, whom a window's actor probes for and whose stacks it delivers.

use super::*;
use crate::event::{Mode, Surface};
use crate::host::{Hosting, Opening, WindowFeed};
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
use std::slice;

/// The one window.
const WINDOW: WindowId = WindowId::nth(0);

/// The actor numbered `number`, as a stack names it.
fn actor(number: u64) -> ProtocolRef<Hearer> {
    ProtocolRef::found(Seated { id: ActorId::new(number), seat: SeatNumber::new(number) })
}

/// The entry `entry` of `actor`'s list, drawn at the window's origin, blocking the pointer from what is beneath it,
/// declaring the default cursor.
fn blocks(actor: ProtocolRef<Hearer>, entry: u32) -> Reach {
    let hearing = Hearing::Everything { blocks: true, cursor: Cursor::Default };

    Reach { actor, entry, origin: Vec2::ZERO, hearing }
}

/// The entry `entry` of `actor`'s list, drawn at the window's origin, letting the pointer through to what is beneath
/// it, declaring the default cursor.
fn passes(actor: ProtocolRef<Hearer>, entry: u32) -> Reach {
    let hearing = Hearing::Everything { blocks: false, cursor: Cursor::Default };

    Reach { actor, entry, origin: Vec2::ZERO, hearing }
}

/// The entry `entry` of `actor`'s list, drawn at the window's origin, hearing presses alone.
fn presses(actor: ProtocolRef<Hearer>, entry: u32) -> Reach {
    Reach { actor, entry, origin: Vec2::ZERO, hearing: Hearing::Presses }
}

/// `reach`, its list drawn with its origin at `x`, `y` of the window.
fn drawn_at(reach: Reach, x: f32, y: f32) -> Reach {
    Reach { origin: Vec2::new(x, y), ..reach }
}

/// `reach`, which hears every pointer event, declaring `cursor`.
fn showing(mut reach: Reach, cursor: Cursor) -> Reach {
    if let Hearing::Everything { cursor: declared, .. } = &mut reach.hearing {
        *declared = cursor;
    }

    reach
}

/// What a frame drew under the pointer, `pointer`, of every event up to `through`, with no place pinned probed.
fn under(through: u64, pointer: &[Reach]) -> Picked {
    Picked { through, pointer: pointer.to_vec(), pinned: Vec::new() }
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
const WHEELED: Event = Event::Wheel { x: 1.0, y: 2.0, delta_x: 0.0, delta_y: 3.0 };

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
    let over = Over::Entry { actor: game, entry: 0, x: 1.0, y: 2.0 };
    let up = PointerEvent::PointerUp { x: 1.0, y: 2.0, button: Button::Main, over };
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
    let pressed = Pinned { at: 1, stack: vec![blocks(button, 0)] };
    let picked = Picked { through: 3, pointer: vec![blocks(beside, 0)], pinned: vec![pressed] };

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

#[test]
fn each_entry_hears_the_pointer_in_its_own_coordinates_from_the_origin_its_list_was_drawn_at() {
    // Catches a pointer event told in the window's pixels to an entry its parent placed elsewhere, so a child placed
    // at a panel hears a click at its own `0, 0` as one at the panel's corner and presses the wrong widget; and the
    // origin of one entry of the stack taken for every entry the event reaches (ADR-0055).
    let (list, panel) = (actor(1), actor(2));
    let mut cascade = heard(&[PRESSED]);
    let stack = [drawn_at(passes(list, 2), 0.5, -3.0), blocks(panel, 0)];

    let sent = cascade.picked(WINDOW, under(1, &stack));

    let down = |x, y| PointerEvent::PointerDown { x, y, button: Button::Main };
    let entered = PointerEvent::PointerEntered;
    let expected =
        vec![to(list, 2, entered), to(panel, 0, entered), to(list, 2, down(0.5, 5.0)), to(panel, 0, down(1.0, 2.0))];
    assert_eq!(sent, expected);
}

#[test]
fn an_entry_its_parent_moves_under_the_pointer_is_neither_left_nor_entered() {
    // Catches hover told by where an entry was drawn as well as which it is, so a list that scrolls under a resting
    // pointer tells its row it was left and entered again every frame it moves (ADR-0055).
    let row = actor(1);
    let mut cascade = Cascade::default();

    let entered = cascade.picked(WINDOW, under(0, &[drawn_at(blocks(row, 0), 0.0, 10.0)]));
    let scrolled = cascade.picked(WINDOW, under(0, &[drawn_at(blocks(row, 0), 0.0, -30.0)]));

    assert_eq!(entered, vec![to(row, 0, PointerEvent::PointerEntered)]);
    assert_eq!(scrolled, vec![]);
}

#[test]
fn a_capture_hears_each_move_from_where_its_entry_was_drawn_when_the_press_reached_it() {
    // Catches a capture told moves from the origin of whatever the pointer is over now, which once a drag leaves the
    // slider is another entry's, or none; the limit ADR-0055 states: a child moved during a drag hears positions from
    // where it was at the press.
    let (slider, game) = (actor(1), actor(2));
    let mut cascade = heard(&[PRESSED, MOVED]);
    cascade.picked(WINDOW, under(1, &[drawn_at(blocks(slider, 3), 1.0, 1.0)]));

    let sent = cascade.picked(WINDOW, under(2, &[drawn_at(blocks(game, 0), 8.0, 8.0)]));

    let expected = vec![
        to(slider, 3, PointerEvent::PointerLeft),
        to(game, 0, PointerEvent::PointerEntered),
        to(slider, 3, PointerEvent::PointerMoved { x: 0.0, y: 1.0 }),
    ];
    assert_eq!(sent, expected);
}

/// What `sent` told `actor` of a release, through any of its entries: what it was over, each time.
fn released_over(sent: &[(ProtocolRef<Hearer>, Pointed)], actor: ProtocolRef<Hearer>) -> Vec<Over> {
    sent.iter()
        .filter(|(to, _)| *to == actor)
        .filter_map(|(_, pointed)| match pointed.event {
            PointerEvent::PointerUp { over, .. } => Some(over),
            _ => None,
        })
        .collect()
}

#[test]
fn a_release_under_a_capture_is_over_the_top_of_its_stack_though_it_lets_the_pointer_through() {
    // Catches a release that says it was over what holds the capture, so a drag never learns where it was dropped; and
    // one over the first entry that blocks, or the stack's bottom, so a drop through a tooltip that lets the pointer
    // through lands on the slot beneath it, which the top of the stack says it did not (ADR-0055). An entry the pointer
    // ignores is never in a stack: the renderer leaves it out.
    let (slider, tooltip, slot) = (actor(1), actor(2), actor(3));
    let mut cascade = heard(&[PRESSED, RELEASED]);
    cascade.picked(WINDOW, under(1, &[blocks(slider, 3)]));

    let sent = cascade.picked(WINDOW, under(2, &[passes(tooltip, 1), blocks(slot, 2)]));

    let over = Over::Entry { actor: tooltip, entry: 1, x: 1.0, y: 2.0 };
    assert_eq!(released_over(&sent, slider), vec![over]);
}

#[test]
fn a_release_is_over_what_was_under_its_own_place_though_the_pointer_moved_on_within_the_frame() {
    // Catches a release over what was under where the pointer went after it, within one frame, so a drag dropped with
    // a flick lands on the slot beside the one it was dropped on (ADR-0055).
    let (slider, slot, beside) = (actor(1), actor(2), actor(3));
    let mut cascade = heard(&[PRESSED, RELEASED, MOVED]);
    let pressed = Pinned { at: 1, stack: vec![blocks(slider, 3)] };
    let released = Pinned { at: 2, stack: vec![drawn_at(blocks(slot, 0), 0.5, 0.5)] };
    let picked = Picked { through: 3, pointer: vec![blocks(beside, 0)], pinned: vec![pressed, released] };

    let sent = cascade.picked(WINDOW, picked);

    let over = Over::Entry { actor: slot, entry: 0, x: 0.5, y: 1.5 };
    assert_eq!(released_over(&sent, slider), vec![over]);
}

#[test]
fn a_release_with_no_capture_goes_through_the_stack_at_its_own_place() {
    // Catches a release no press captured sent through the stack where the pointer went after it, within one frame,
    // so the entry it says it was over is not the one that hears it, and what is beside that entry hears a release it
    // was never under.
    let (slot, beside) = (actor(1), actor(2));
    let mut cascade = heard(&[RELEASED, MOVED]);
    let released = Pinned { at: 1, stack: vec![blocks(slot, 0)] };
    let picked = Picked { through: 2, pointer: vec![blocks(beside, 0)], pinned: vec![released] };

    let sent = cascade.picked(WINDOW, picked);

    let over = Over::Entry { actor: slot, entry: 0, x: 1.0, y: 2.0 };
    assert_eq!(released_over(&sent, slot), vec![over]);
    assert_eq!(released_over(&sent, beside), vec![]);
}

#[test]
fn a_press_and_a_release_pin_their_places_and_a_move_or_a_wheel_turn_does_not() {
    // Catches a release probed only as the pointer, so no stack at its own place comes back and what it was over is
    // taken from where the pointer went after it; and a move or a wheel turn pinned, spending the frame's few probes on
    // places nothing reads, so presses and releases after them go unprobed.
    let wheel = Event::Wheel { x: 1.0, y: 2.0, delta_x: 0.0, delta_y: 40.0 };
    let mut cascade = Cascade::default();

    let pinned: Vec<bool> = [PRESSED, MOVED, wheel, RELEASED]
        .iter()
        .filter_map(|event| cascade.heard(WINDOW, event))
        .map(|probed| probed.pinned)
        .collect();

    assert_eq!(pinned, vec![true, false, false, true]);
}

#[test]
fn a_release_tells_where_it_was_over_in_the_coordinates_of_the_entry_it_was_over() {
    // Catches the place a release was over given in the window's pixels, or from the origin of the capture's entry, as
    // the release itself is: a drop on a slot its parent placed at a panel would then be taken as one at another slot
    // of that panel (ADR-0055).
    let (slider, slot) = (actor(1), actor(2));
    let mut cascade = heard(&[PRESSED, RELEASED]);
    cascade.picked(WINDOW, under(1, &[drawn_at(blocks(slider, 3), 0.5, 0.5)]));

    let sent = cascade.picked(WINDOW, under(2, &[drawn_at(blocks(slot, 0), -4.0, 1.5)]));

    let over = Over::Entry { actor: slot, entry: 0, x: 5.0, y: 0.5 };
    let up = PointerEvent::PointerUp { x: 0.5, y: 1.5, button: Button::Main, over };
    assert_eq!(sent.last(), Some(&to(slider, 3, up)));
}

#[test]
fn a_release_where_the_pointer_reaches_nothing_says_it_was_over_nothing() {
    // Catches a release over empty space that says it was over what the press reached, or what the pointer last
    // reached, so a drag dropped outside every screen is taken as dropped on the slot it began on.
    let slider = actor(1);
    let mut cascade = heard(&[PRESSED, RELEASED]);
    cascade.picked(WINDOW, under(1, &[blocks(slider, 3)]));

    let sent = cascade.picked(WINDOW, under(2, &[]));

    assert_eq!(released_over(&sent, slider), vec![Over::Nothing]);
}

#[test]
fn a_release_over_the_entry_that_holds_the_capture_says_so() {
    // Catches a release that passes over the actor holding the capture to what lies beneath it, so a drag dropped
    // back on its own screen is taken as dropped on the game; the source decides what a drop on itself means.
    let (slider, game) = (actor(1), actor(2));
    let mut cascade = heard(&[PRESSED, RELEASED]);
    cascade.picked(WINDOW, under(1, &[passes(slider, 3), blocks(game, 0)]));

    let sent = cascade.picked(WINDOW, under(2, &[passes(slider, 3), blocks(game, 0)]));

    let over = Over::Entry { actor: slider, entry: 3, x: 1.0, y: 2.0 };
    assert_eq!(released_over(&sent, slider), vec![over]);
    assert_eq!(released_over(&sent, game), vec![over], "the capture is every entry the press reached");
}

/// What a test tells the prober to do.
#[derive(Debug, Clone, PartialEq, Bound)]
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
/// with the window's feed, and the host's end of the root's requests.
fn probed_window() -> Result<(Harness<Bench>, WindowFeed, Hosting), Unheard> {
    let mut setup = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).with_cycle::<()>();
    let mut opening = Opening::new(setup.source::<Heard>(), setup.controller::<Windowing>());
    let surface = Surface { width: 640, height: 480, scale: 1.0 };
    let feed = opening.open("window", surface, Mode::Windowed, true);
    let (config, hosting) = opening.config();
    let engine =
        setup.add_root::<Windows>(&(), config)?.add_root::<Prober>(&(), Prober)?.add_root::<Widget>(&(), Widget)?;
    let mut harness = Harness::bare(engine.ready())?;
    harness.host().engine().record_turns();

    Ok((harness, feed, hosting))
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
    // not say its place is pinned, so the press is tested where the pointer went after it; and stacks from any actor
    // delivered, so any actor could send the pointer where it chose.
    let (mut harness, feed, _hosting) = probed_window()?;
    assert_eq!(feed.push(MOVED), Ok(()), "the window root holds its source");
    assert_eq!(sent(&mut harness), (vec![], vec![]), "nobody probes yet");

    let prober = ActorRef::<Prober>::found(found::<Prober>(&mut harness, path::<Prober>(()).names()));
    harness.tell(prober, &ProbeOrder::Subscribe)?;
    harness.settle_turns();
    assert_eq!(feed.push(PRESSED), Ok(()), "the window root holds its source");
    let pressed = Probed { window: WINDOW, at: 1, x: 1.0, y: 2.0, pinned: true };
    assert_eq!(sent(&mut harness), (vec![pressed], vec![]));

    let widget = ProtocolRef::found(found::<Widget>(&mut harness, path::<Widget>(()).names()));
    let picked = Picked { through: 1, pointer: vec![blocks(widget, 0)], pinned: Vec::new() };
    let window = ActorRef::<Window>::found(found::<Window>(&mut harness, path::<Window>(WINDOW).names()));
    harness.tell(window, &picked)?;
    assert_eq!(sent(&mut harness), (vec![], vec![]), "stacks the harness sent are not the prober's");

    harness.tell(prober, &ProbeOrder::Pick(picked))?;
    let down = PointerEvent::PointerDown { x: 1.0, y: 2.0, button: Button::Main };
    let reached = |event| Pointed { window: WINDOW, entry: 0, event };
    assert_eq!(sent(&mut harness), (vec![], vec![reached(PointerEvent::PointerEntered), reached(down)]));

    Ok(())
}

#[test]
fn the_cursor_is_the_topmost_entry_s_the_pointer_reaches_not_the_bottom_s_nor_the_first_that_blocks() {
    // Catches a cursor taken from the bottom of the stack, so the game's crosshair shows over the console; and from the
    // first entry that blocks, so a tooltip that lets the pointer through shows the hand of the button beneath it.
    let (tooltip, button, game) = (actor(1), actor(2), actor(3));
    let mut cascade = Cascade::default();
    let button = showing(blocks(button, 4), Cursor::Hand);
    let game = showing(blocks(game, 0), Cursor::Grab);

    cascade.picked(WINDOW, under(0, &[showing(passes(tooltip, 0), Cursor::Text), button.clone(), game.clone()]));
    assert_eq!(cascade.cursor(), &Cursor::Text);

    cascade.picked(WINDOW, under(0, &[button, game]));
    assert_eq!(cascade.cursor(), &Cursor::Hand);
}

#[test]
fn over_nothing_the_pointer_reaches_the_default_cursor_shows() {
    // Catches a window that keeps the last cursor it showed once the pointer leaves everything that declares one, so
    // the text field's I-beam stays over the empty window.
    let mut cascade = Cascade::default();
    assert_eq!(cascade.cursor(), &Cursor::Default, "nothing has been drawn yet");

    cascade.picked(WINDOW, under(0, &[showing(blocks(actor(1), 0), Cursor::Text)]));
    cascade.picked(WINDOW, under(0, &[]));

    assert_eq!(cascade.cursor(), &Cursor::Default);
}

#[test]
fn while_a_button_is_held_the_pressed_entry_s_cursor_shows_over_another_actor_and_the_pointer_s_once_released() {
    // Catches a cursor that follows the pointer during a drag, so the slider's grip turns into the I-beam of the text
    // field it crosses; one taken from the top of the press's stack before the press is delivered, so a press that
    // reaches nothing shows what was under the pointer; and a capture's cursor kept after the release.
    let (slider, field) = (actor(1), actor(2));
    let slider = showing(blocks(slider, 3), Cursor::Grabbing);
    let field = showing(blocks(field, 0), Cursor::Text);
    let mut cascade = heard(&[PRESSED, MOVED]);

    cascade.picked(WINDOW, under(1, &[slider]));
    assert_eq!(cascade.cursor(), &Cursor::Grabbing, "the press reached the slider");

    cascade.picked(WINDOW, under(2, slice::from_ref(&field)));
    assert_eq!(cascade.cursor(), &Cursor::Grabbing, "the pointer is over the field, the button still held");

    cascade.heard(WINDOW, &RELEASED);
    cascade.picked(WINDOW, under(3, &[field]));
    assert_eq!(cascade.cursor(), &Cursor::Text, "released over the field");
}

#[test]
fn a_pressed_entry_that_declares_another_cursor_once_its_drag_begins_shows_it_wherever_the_pointer_goes_after() {
    // Catches a capture's cursor fixed at the press, so a source that declares the image it drags once the pointer has
    // moved a few pixels, as a drag begins, never shows it; and one followed only while the pointer is over the
    // source, so the image turns back into the grip as it is dragged away.
    let (item, slot) = (actor(1), actor(2));
    let mut cascade = heard(&[PRESSED, MOVED, MOVED]);
    cascade.picked(WINDOW, under(1, &[showing(blocks(item, 0), Cursor::Grab)]));

    cascade.picked(WINDOW, under(2, &[showing(blocks(item, 0), Cursor::Grabbing)]));
    assert_eq!(cascade.cursor(), &Cursor::Grabbing, "the item's drag began with the pointer still over it");

    cascade.picked(WINDOW, under(3, &[showing(blocks(slot, 5), Cursor::NotAllowed)]));
    assert_eq!(cascade.cursor(), &Cursor::Grabbing, "dragged over a slot that declares another cursor");
}

#[test]
fn a_window_asks_its_host_for_a_cursor_only_as_the_one_it_shows_changes() -> Result<(), Unheard> {
    // Catches a cursor that never reaches the host, so the window shows the arrow whatever is declared; one asked for
    // again with every frame's stacks, so the host is asked to set the same cursor sixty times a second; and the
    // default never asked for back, so the I-beam stays once the pointer leaves the field.
    let (mut harness, feed, hosting) = probed_window()?;
    let prober = ActorRef::<Prober>::found(found::<Prober>(&mut harness, path::<Prober>(()).names()));
    harness.tell(prober, &ProbeOrder::Subscribe)?;
    harness.settle_turns();
    let widget = ProtocolRef::found(found::<Widget>(&mut harness, path::<Widget>(()).names()));
    let field = showing(blocks(widget, 0), Cursor::Text);
    let shown = |harness: &mut Harness<Bench>, stack: Vec<Reach>, at: u64| -> Result<Vec<Windowing>, Unheard> {
        assert_eq!(feed.push(MOVED), Ok(()), "the window root holds its source");
        harness.settle_turns();
        harness.tell(prober, &ProbeOrder::Pick(Picked { through: at, pointer: stack, pinned: Vec::new() }))?;
        harness.settle_turns();

        Ok(hosting.take())
    };

    let set = |cursor| vec![Windowing::SetCursor { window: WINDOW, cursor }];
    assert_eq!(shown(&mut harness, vec![field.clone()], 1)?, set(Cursor::Text));
    assert_eq!(shown(&mut harness, vec![field], 2)?, vec![], "the same cursor, shown already");
    assert_eq!(shown(&mut harness, vec![], 3)?, set(Cursor::Default));

    Ok(())
}

#[test]
fn a_press_reaches_an_entry_that_hears_presses_alone_and_goes_on_beneath_it_to_the_topmost_that_blocks() {
    // Catches an entry that hears presses alone missing the press, so a parent's focused field keeps its caret after
    // a click into its child; one that stops the press, so the child it is listed over never hears its own click; and
    // one beneath an entry that blocks still sent the press, so a click on the console blurs the game's field.
    let (parent, child, beneath) = (actor(1), actor(2), actor(3));
    let mut cascade = heard(&[PRESSED]);
    let stack = [presses(parent, 7), blocks(child, 2), presses(beneath, 0)];

    let sent = cascade.picked(WINDOW, under(1, &stack));

    let down = PointerEvent::PointerDown { x: 1.0, y: 2.0, button: Button::Main };
    let expected = vec![to(child, 2, PointerEvent::PointerEntered), to(parent, 7, down), to(child, 2, down)];
    assert_eq!(sent, expected);
}

#[test]
fn an_entry_that_hears_presses_alone_hears_no_move_hover_wheel_or_release_and_is_never_captured() {
    // Catches an entry that hears presses alone treated as one that passes: hovered, so the parent lights a widget the
    // child covers; sent the moves and the wheel under the pointer; captured by its press, so a drag in the child also
    // drags in the parent and the release reaches it; and named as what the release was over, so a drop into the child
    // lands on the parent.
    let (parent, child) = (actor(1), actor(2));
    let mut cascade = heard(&[MOVED, PRESSED, MOVED, RELEASED, WHEELED]);
    let stack = [presses(parent, 7), blocks(child, 2)];

    let sent = cascade.picked(WINDOW, under(5, &stack));

    let over = Over::Entry { actor: child, entry: 2, x: 1.0, y: 2.0 };
    let moved = PointerEvent::PointerMoved { x: 1.0, y: 2.0 };
    let down = PointerEvent::PointerDown { x: 1.0, y: 2.0, button: Button::Main };
    let up = PointerEvent::PointerUp { x: 1.0, y: 2.0, button: Button::Main, over };
    let wheel = PointerEvent::Wheel { x: 1.0, y: 2.0, delta_x: 0.0, delta_y: 3.0 };
    let expected = vec![
        to(child, 2, PointerEvent::PointerEntered),
        to(child, 2, moved),
        to(parent, 7, down),
        to(child, 2, down),
        to(child, 2, moved),
        to(child, 2, up),
        to(child, 2, wheel),
    ];
    assert_eq!(sent, expected);
}

#[test]
fn an_entry_that_hears_presses_alone_never_chooses_the_cursor_and_a_drag_beneath_it_keeps_the_dragged_one() {
    // Catches the cursor taken from an entry that hears presses alone, so a parent's hit over its child replaces the
    // child's I-beam with the default; and that entry captured first by the press, so the child's drag never shows the
    // grip it declares once its drag begins.
    let (parent, child) = (actor(1), actor(2));
    let mut resting = Cascade::default();
    resting.picked(WINDOW, under(0, &[presses(parent, 7), showing(blocks(child, 2), Cursor::Text)]));
    let mut dragging = heard(&[PRESSED, MOVED]);
    dragging.picked(WINDOW, under(1, &[presses(parent, 7), showing(blocks(child, 2), Cursor::Grab)]));
    let pressed = dragging.cursor().clone();

    dragging.picked(WINDOW, under(2, &[presses(parent, 7), showing(blocks(child, 2), Cursor::Grabbing)]));
    let mut alone = Cascade::default();
    alone.picked(WINDOW, under(0, &[presses(parent, 7)]));

    assert_eq!(resting.cursor(), &Cursor::Text);
    assert_eq!(pressed, Cursor::Grab);
    assert_eq!(dragging.cursor(), &Cursor::Grabbing, "the child's drag began beneath the parent's hit");
    assert_eq!(alone.cursor(), &Cursor::Default);
}
