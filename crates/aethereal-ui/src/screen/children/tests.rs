//! The children a screen places: where each is called in its list, and what size each is told, read from the lists its
//! `Prepare`s send, the orders they return, and where its layouts put each widget.

use super::super::input::tests::{
    Ev, column, hit, laid, over, point, press, raised, release, screen_with_fonts, sized, text_input, window,
};
use super::super::orders::Order;
use super::super::tests::{last_list, screen, sent};
use super::super::{Listed, Outcome, Screen, Viewport};
use crate::child::{CallArea, Called, Resize, Resizes};
use crate::input::Raised;
use crate::style::{Edges, Length, Space, Style};
use crate::table::Focus;
use crate::tree::{KeyPath, Node, Segment, Tree};
use crate::widgets::{Button, Opened, Popup, Scroll, Stack};
use aethereal_actor::{
    Actor, ActorRef, Anywhere, Beginning, ChildOf, Ending, Fault, Handles, ProtocolRef, SeatNumber, Seated, Turn, child,
};
use aethereal_data::{ActorId, Plain};
use aethereal_math::{Rect2, Vec2};
use aethereal_render::draw_list::{Area, Callee, Clip, Entry, Pointer};
use aethereal_render::text::Colour;
use aethereal_window::{AllowIme, Button as Pressed, PointerEvent};

/// A child the tests' screens place, keyed by a number: it takes its size, and does nothing with it.
#[derive(Debug, Plain)]
#[kind(name = "aether.test.ui.minimap")]
struct Minimap;

impl ChildOf for Minimap {
    type Parent = Anywhere;
}

impl Actor for Minimap {
    type Config = Self;
    type Key = u64;
    type Handles = (Resize,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(minimap: Self, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(minimap)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Resize> for Minimap {
    type Sends = ();

    fn handle(&mut self, _: Resize, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// The minimap spawned under `key`, as the engine refers to it: its id and seat are its key's.
fn spawned(key: u64) -> ActorRef<Minimap> {
    let number = key + 100;

    ActorRef::found(Seated { id: ActorId::new(number), seat: SeatNumber::new(number) })
}

/// The minimap under `key`, as a screen that spawned it holds it.
fn minimap(key: u64) -> Called {
    Called::new(child::<Minimap>(key), spawned(key))
}

/// What a call of the minimap under `key` names.
fn callee(key: u64) -> Callee {
    Callee::of(child::<Minimap>(key))
}

/// What the minimap under `key` is told its size through.
fn told(key: u64) -> ProtocolRef<Resizes> {
    spawned(key).protocol()
}

/// A slot at `place` among its siblings, `width` by `height`, placing the minimap under `key` in `area`.
fn slot(place: u32, key: u64, area: CallArea, width: f32, height: f32) -> Node {
    Node::call(Segment::Place(place), minimap(key), area, sized(width, height))
}

/// The path of the root column's child at `place`.
fn at(place: u32) -> KeyPath {
    KeyPath::of(&[Segment::Place(0), Segment::Place(place)])
}

/// A filled box at `place` among its siblings, `width` by `height`, so it draws.
fn block(place: u32, width: f32, height: f32) -> Node {
    let background = Colour::opaque(0.2, 0.4, 0.6);
    let style = Style { width: Length::Pixels(width), height: Length::Pixels(height), background, ..Style::default() };

    Node::new(Segment::Place(place), Stack, style, vec![])
}

/// The rect from `min` to `max`.
fn rect(min: [f32; 2], max: [f32; 2]) -> Rect2 {
    Rect2 { min: Vec2::new(min[0], min[1]), max: Vec2::new(max[0], max[1]) }
}

/// What the screen's last list among `orders` draws and calls, in order: a draw as nothing more, a call as its child
/// and its area.
fn drawn_and_called(orders: &[Order]) -> Vec<Option<(Callee, Area)>> {
    last_list(orders)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|entry| match entry {
            Entry::Draw { .. } => Some(None),
            Entry::Call { child, area } => Some(Some((child, area))),
            Entry::Run(_) | Entry::Hit { .. } | Entry::Keys { .. } => None,
        })
        .collect()
}

/// The area of the call of the minimap under `key` in the screen's last list among `orders`.
fn area_of(orders: &[Order], key: u64) -> Area {
    let found = drawn_and_called(orders).into_iter().flatten().find(|(called, _)| *called == callee(key));
    let Some((_, area)) = found else {
        panic!("the minimap under {key} is called");
    };

    area
}

/// Each size `orders` tell a child, in order.
fn resizes(orders: &[Order]) -> Vec<(ProtocolRef<Resizes>, Viewport)> {
    orders
        .iter()
        .filter_map(|order| match order {
            Order::Resize { child, resize } => Some((*child, resize.viewport)),
            _ => None,
        })
        .collect()
}

/// The viewport of a child `width` by `height` pixels in the tests' screen, 200 by 100 at a UI scale of `scale`.
fn viewport(width: f32, height: f32, scale: f32) -> Viewport {
    Viewport { size: Vec2::new(width, height), ..Viewport::window(200, 100, scale) }
}

/// The number of the entry of the screen's list that is the hit over the child placed in the slot at `path`.
fn into_child(screen: &Screen, path: &KeyPath) -> u32 {
    let Some(at) = listed_at(screen, |listed| *listed == Listed::Child(path.clone())) else {
        panic!("the screen lists a hit over the child at {path:?}: {:?}", screen.listing);
    };

    u32::try_from(at).unwrap_or(u32::MAX)
}

/// Whether the popup at `path` is open, as the screen's table keeps it.
fn opened(screen: &Screen, path: &KeyPath) -> Opened {
    screen.table().state::<Popup>(path).map_or(Opened::Shut, |popped| popped.open)
}

/// The place in the screen's list of the first entry `listed` matches.
fn listed_at(screen: &Screen, listed: impl Fn(&Listed) -> bool) -> Option<usize> {
    screen.listing.iter().position(listed)
}

#[test]
fn a_child_is_called_at_its_slot_s_content_box_in_draw_order_in_the_area_it_names() {
    // Catches a call listed after everything else, so the box drawn after the slot hides beneath the child; a child
    // placed at the slot's border box, so it draws over the slot's padding; and the markup's area dropped or swapped,
    // so a child that draws in the window's pixels is moved again by the slot's corner.
    let mut screen = screen();
    let padded = Style { padding: Edges::all(Space::Pixels(5.0)), ..sized(80.0, 40.0) };
    let inheriting = Node::call(Segment::Place(1), minimap(1), CallArea::Inherit, padded);
    let tree = column(vec![
        block(0, 200.0, 10.0),
        inheriting,
        block(2, 200.0, 10.0),
        slot(3, 2, CallArea::Absolute, 50.0, 30.0),
    ]);

    let (_, orders) = laid(&mut screen, tree, &[]);

    let inherit = Area::Inherit { origin: Vec2::new(5.0, 15.0), clip: Clip::To(rect([5.0, 15.0], [75.0, 45.0])) };
    let absolute = Area::Absolute { clip: Clip::To(rect([0.0, 60.0], [50.0, 90.0])) };
    assert_eq!(drawn_and_called(&orders), vec![None, Some((callee(1), inherit)), None, Some((callee(2), absolute))]);
}

#[test]
fn a_child_is_told_its_size_when_its_slot_or_the_scale_changes_and_never_when_it_moves() {
    // Catches a child told nothing, so it lays out to the size it began with; one told on every layout, a mail each
    // frame anything in its parent changes; one told when its slot only moved, which costs it a layout for nothing; and
    // one not told when the parent's UI scale changed, so it draws at the old scale inside a slot of the new size.
    let mut screen = screen();
    let tree = |above: f32, tall: f32| column(vec![block(0, 200.0, above), slot(1, 1, CallArea::Inherit, 80.0, tall)]);

    let (_, first) = laid(&mut screen, tree(5.0, 20.0), &[]);
    let (_, moved) = laid(&mut screen, tree(10.0, 20.0), &[]);
    let (_, resized) = laid(&mut screen, tree(10.0, 30.0), &[]);
    let (_, again) = laid(&mut screen, tree(15.0, 30.0), &[]);
    let rescaled_frame = screen.resize(Viewport::window(200, 100, 2.0));
    let (_, rescaled) = laid(&mut screen, tree(15.0, 30.0), &[]);

    assert_eq!(resizes(&first), vec![(told(1), viewport(80.0, 20.0, 1.0))]);
    assert_eq!(resizes(&moved), vec![]);
    assert_eq!(resizes(&resized), vec![(told(1), viewport(80.0, 30.0, 1.0))]);
    assert_eq!(resizes(&again), vec![]);
    assert_eq!(resizes(&sent(&rescaled_frame)), vec![], "a size is told once the slot is laid out anew");
    assert_eq!(resizes(&rescaled), vec![(told(1), viewport(160.0, 60.0, 2.0))]);
}

#[test]
fn a_child_in_a_scroll_slides_with_it_and_is_cut_to_it() {
    // Catches a child placed where its slot was laid out rather than where the scroll slid it, so it stays put while the
    // list around it moves; one cut only to its own slot, so it draws past the scroll's edge; and a scroll that tells
    // the child its size as it slides, though only its place changed.
    let mut screen = screen();
    let scroll_style = Style { width: Length::Pixels(200.0), height: Length::Pixels(50.0), ..Style::default() };
    let tree = || {
        let held = vec![block(0, 150.0, 30.0), slot(1, 1, CallArea::Inherit, 80.0, 40.0), block(2, 150.0, 100.0)];
        column(vec![Node::new(Segment::Place(0), Scroll::down(), scroll_style.clone(), held)])
    };
    let scroll = at(0);
    let (_, first) = laid(&mut screen, tree(), &[]);

    let entry = hit(&screen, &scroll);
    point(&mut screen, entry, PointerEvent::Wheel { x: 10.0, y: 10.0, delta_x: 0.0, delta_y: 20.0 });
    let (_, slid) = laid(&mut screen, tree(), &[]);

    let offset = screen.table().state::<Scroll>(&scroll).unwrap_or_default().offset;
    assert!(offset > 0.0, "the wheel slid the scroll");
    let unslid = Area::Inherit { origin: Vec2::new(0.0, 30.0), clip: Clip::To(rect([0.0, 30.0], [80.0, 50.0])) };
    let top = 30.0 - offset;
    let slid_area = Area::Inherit { origin: Vec2::new(0.0, top), clip: Clip::To(rect([0.0, top], [80.0, 50.0])) };
    assert_eq!(area_of(&first, 1), unslid);
    assert_eq!(area_of(&slid, 1), slid_area);
    assert_eq!(resizes(&slid), vec![]);
}

#[test]
fn a_child_in_a_popup_s_content_is_called_in_the_top_layer_only_while_it_is_open() {
    // Catches a child in a shut popup called, or told a size, though nothing of the popup's content shows; one in an
    // open popup called at its place in the tree, so the box after the popup covers it; and the hit over it that hears
    // presses listed while the popup is shut, so a press where it would be blurs the screen, or listed at the child's
    // place in the tree, beneath the box after the popup, so a press into the child never reaches it.
    let mut screen = screen();
    let tree = || {
        let held = vec![block(0, 40.0, 10.0), slot(1, 1, CallArea::Inherit, 60.0, 30.0)];
        let popup = Node::new(Segment::Place(0), Popup::new(), Style::default(), held);

        column(vec![popup, block(1, 200.0, 40.0)])
    };
    let (popup, inside, after) = (at(0), at(0).child(Segment::Place(1)), at(1));

    let (_, shut) = laid(&mut screen, tree(), &[]);
    let called_shut = listed_at(&screen, |listed| *listed == Listed::Call(inside.clone()));
    let pressed_shut = listed_at(&screen, |listed| *listed == Listed::Child(inside.clone()));

    let entry = hit(&screen, &popup);
    press(&mut screen, entry, 2.0, 2.0);
    release(&mut screen, entry, 2.0, 2.0, over(entry, 2.0, 2.0));
    let (_, open) = laid(&mut screen, tree(), &[]);

    assert_eq!(called_shut, None);
    assert_eq!(pressed_shut, None);
    assert_eq!(resizes(&shut), vec![]);
    let called = listed_at(&screen, |listed| *listed == Listed::Call(inside.clone()));
    let pressed = listed_at(&screen, |listed| *listed == Listed::Child(inside.clone()));
    let covering = listed_at(&screen, |listed| matches!(listed, Listed::Draw(key) if key.path == after));
    let (Some(called), Some(pressed), Some(covering)) = (called, pressed, covering) else {
        panic!("the child, its hit, and the box after the popup are all listed: {:?}", screen.listing);
    };
    assert!(called > covering, "the child is listed over the box after its popup: {:?}", screen.listing);
    assert_eq!(pressed, called + 1, "the hit over the child is listed right over it: {:?}", screen.listing);
    assert_eq!(resizes(&open), vec![(told(1), viewport(60.0, 30.0, 1.0))]);
}

#[test]
fn a_hidden_child_is_left_out_without_a_word_to_it_and_told_its_size_again_when_shown() {
    // Catches a hidden child still called, so it still draws; one mailed as it is hidden, an empty size, say, which a
    // child that is a screen would lay its panel out to and lose; and one shown again and not told its size, which may
    // have changed while it was hidden.
    let mut screen = screen();
    let tree = |show: bool| {
        let mut children = vec![block(0, 200.0, 10.0)];
        if show {
            children.push(slot(1, 1, CallArea::Inherit, 80.0, 40.0));
        }
        column(children)
    };

    let (_, shown) = laid(&mut screen, tree(true), &[]);
    let (_, hidden) = laid(&mut screen, tree(false), &[]);
    let called_hidden = drawn_and_called(&hidden);
    let (_, again) = laid(&mut screen, tree(true), &[]);

    assert_eq!(resizes(&shown), vec![(told(1), viewport(80.0, 40.0, 1.0))]);
    assert_eq!(called_hidden, vec![None], "only the box is listed");
    let mailed = hidden.iter().any(|order| matches!(order, Order::Resize { .. }));
    assert!(!mailed, "{hidden:?}");
    assert_eq!(resizes(&again), vec![(told(1), viewport(80.0, 40.0, 1.0))]);
}

#[test]
fn an_unchanged_tree_with_a_child_is_skipped() {
    // Catches a slot that compares unequal to itself rebuilt, by the reference or the call it holds, so a screen with a
    // child lays out and tells the child its size every frame.
    let mut screen = screen();
    let tree = || column(vec![slot(0, 1, CallArea::Absolute, 80.0, 40.0)]);
    laid(&mut screen, tree(), &[]);

    let again = screen.prepare(tree(), &[]);

    assert_eq!(again.outcome, Outcome::Unchanged);
    assert!(again.orders.is_empty(), "{:?}", again.orders);
}

#[test]
fn a_child_screen_lays_out_to_the_size_it_is_told_within_its_parent_s_frame() {
    // Catches a child screen that lays out to its frame, the window, rather than the size it was told, so its panel
    // overflows the slot and is cut; and one that rewrites its program's frame size though only its own size changed.
    let mut screen = screen();
    let whole = Style { width: Length::Share(1.0), height: Length::Share(1.0), ..Style::default() };
    let tree = || Tree::new(Node::new(Segment::Place(0), Stack, whole.clone(), vec![]));
    laid(&mut screen, tree(), &[]);

    let resize = Resize { viewport: viewport(80.0, 40.0, 1.0) };
    let rewritten = screen.resize(resize.viewport);
    let (outcome, _) = laid(&mut screen, tree(), &[]);

    let root = KeyPath::of(&[Segment::Place(0)]);
    let found = screen.unkept.placed.iter().find(|(placed, _)| *placed == root).map(|(_, laid)| laid.rect);
    assert_eq!(outcome, Outcome::Laid);
    assert_eq!(found, Some(rect([0.0, 0.0], [80.0, 40.0])));
    assert!(rewritten.is_empty(), "{rewritten:?}");
}

#[test]
fn a_widget_listed_after_a_child_is_reached_through_its_own_entry() {
    // Catches what the screen keeps of its list falling out of step with the entries it sent at a call, so the pointer
    // through the entry of a widget after a child reaches the widget before it, or nothing.
    let mut screen = screen();
    let button = Node::new(Segment::Place(1), Button::new(Ev::Click(4)), sized(40.0, 20.0), vec![]);
    let (_, orders) = laid(&mut screen, column(vec![slot(0, 1, CallArea::Inherit, 80.0, 40.0), button]), &[]);
    let sent = last_list(&orders).unwrap_or_default();
    let button_box = rect([0.0, 40.0], [40.0, 60.0]);
    let found = sent.iter().position(|entry| matches!(entry, Entry::Hit { rect, .. } if *rect == button_box));
    let Some(entry) = found.and_then(|place| u32::try_from(place).ok()) else {
        panic!("the button's hit is sent: {sent:?}");
    };

    press(&mut screen, entry, 2.0, 2.0);
    let clicked = raised(release(&mut screen, entry, 2.0, 2.0, over(entry, 2.0, 2.0)));

    assert_eq!(clicked, vec![Raised::Event(Ev::Click(4))]);
}

#[test]
fn the_hit_over_a_child_is_listed_over_its_call_at_what_it_is_cut_to_and_hears_presses_alone() {
    // Catches the hit listed before the call, so the child's own entries stand over it and a press the child blocks
    // never reaches the screen; one over the slot's border box, or not cut to the scroll around it, so a press on the
    // slot's padding or past the scroll's edge counts as a press into the child; and one that blocks or passes, so it
    // takes the child's cursor, its hover, and its drags.
    let mut screen = screen();
    let scroll_style = Style { width: Length::Pixels(200.0), height: Length::Pixels(30.0), ..Style::default() };
    let padded = Style { padding: Edges::all(Space::Pixels(5.0)), ..sized(80.0, 40.0) };
    let held = vec![Node::call(Segment::Place(0), minimap(1), CallArea::Inherit, padded)];
    let tree = column(vec![Node::new(Segment::Place(0), Scroll::down(), scroll_style, held)]);

    let (_, orders) = laid(&mut screen, tree, &[]);

    let sent = last_list(&orders).unwrap_or_default();
    let called = sent.iter().position(|entry| matches!(entry, Entry::Call { child, .. } if *child == callee(1)));
    let Some(called) = called else {
        panic!("the child is called: {sent:?}");
    };
    let over = sent.get(called + 1).cloned();
    let cut = rect([5.0, 5.0], [75.0, 30.0]);
    let expected =
        Entry::Hit { frame: screen.target.frame, pass: screen.target.pass, rect: cut, pointer: Pointer::Presses };
    assert_eq!(over, Some(expected));
}

#[test]
fn a_press_into_a_child_clears_the_screen_s_focus_and_stops_its_input_method_and_raises_nothing() {
    // Catches a screen whose focused text input keeps its focus and caret after a click into its child, and goes on
    // taking the keys the child passes; its input method left composing; and a press into the child that raises an
    // event of the screen's own, or focuses something.
    let mut screen = screen_with_fonts();
    let tree = || column(vec![text_input("gold"), slot(1, 1, CallArea::Inherit, 80.0, 40.0)]);
    laid(&mut screen, tree(), &[]);
    let field = hit(&screen, &at(0));
    let focused = press(&mut screen, field, 5.0, 5.0);
    release(&mut screen, field, 5.0, 5.0, over(field, 5.0, 5.0));
    laid(&mut screen, tree(), &[]);
    let child_hit = into_child(&screen, &at(1));

    let blurred = press(&mut screen, child_hit, 10.0, 30.0);

    assert_eq!(focused.imes, vec![AllowIme { window: window(), allowed: true }]);
    assert_eq!(screen.table().focus(), &Focus::Nowhere);
    assert_eq!(blurred.imes, vec![AllowIme { window: window(), allowed: false }]);
    assert_eq!(raised(blurred), vec![]);
}

#[test]
fn a_press_into_a_child_shuts_an_open_popup_it_is_outside_of_and_not_one_it_is_within() {
    // Catches a screen's open menu left open after a click into a child beside it; and a press into a child within the
    // menu's own content that shuts the menu, so the child can never be used.
    let mut screen = screen();
    let tree = || {
        let held = vec![block(0, 40.0, 10.0), slot(1, 1, CallArea::Inherit, 60.0, 30.0)];
        let popup = Node::new(Segment::Place(0), Popup::new(), Style::default(), held);

        column(vec![popup, slot(1, 2, CallArea::Inherit, 80.0, 40.0)])
    };
    let (popup, inside, beside) = (at(0), at(0).child(Segment::Place(1)), at(1));
    laid(&mut screen, tree(), &[]);
    let anchor = hit(&screen, &popup);
    press(&mut screen, anchor, 2.0, 2.0);
    release(&mut screen, anchor, 2.0, 2.0, over(anchor, 2.0, 2.0));
    laid(&mut screen, tree(), &[]);

    let within = into_child(&screen, &inside);
    press(&mut screen, within, 2.0, 2.0);
    let after_within = opened(&screen, &popup);
    let outside = into_child(&screen, &beside);
    let secondary = PointerEvent::PointerDown { x: 2.0, y: 2.0, button: Pressed::Secondary };
    point(&mut screen, outside, secondary);

    assert_eq!(after_within, Opened::Open);
    assert_eq!(opened(&screen, &popup), Opened::Shut, "any button's press into the child beside it shuts it");
}

#[test]
fn the_hit_over_a_child_raises_nothing_and_takes_no_hover_move_release_or_wheel() {
    // Catches the screen taking the pointer through the hit over a child as it takes a widget's: hovered, so a tooltip
    // whose anchor is beside the child waits as if pointed at; laid out again for each move over the child; or a
    // release over the child taken as a click of the screen's.
    let mut screen = screen();
    let tree = || column(vec![slot(0, 1, CallArea::Inherit, 80.0, 40.0)]);
    laid(&mut screen, tree(), &[]);
    let child_hit = into_child(&screen, &at(0));

    let events = [
        PointerEvent::PointerEntered,
        PointerEvent::PointerMoved { x: 3.0, y: 4.0 },
        PointerEvent::PointerUp { x: 3.0, y: 4.0, button: Pressed::Main, over: over(child_hit, 3.0, 4.0) },
        PointerEvent::Wheel { x: 3.0, y: 4.0, delta_x: 0.0, delta_y: 20.0 },
        PointerEvent::PointerLeft,
    ];
    let heard: Vec<Vec<Raised<Ev>>> =
        events.into_iter().map(|event| raised(point(&mut screen, child_hit, event))).collect();
    let again = screen.prepare(tree(), &[]);

    assert!(heard.iter().all(Vec::is_empty), "{heard:?}");
    assert!(screen.reached.is_empty(), "{:?}", screen.reached);
    assert_eq!(again.outcome, Outcome::Unchanged);
}
