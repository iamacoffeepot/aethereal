//! A screen's drags: when a press becomes a drag rather than a click, the cursor its source declares, where its drop
//! lands, in the screen, on another actor, or nowhere, what an actor dropped on does with it, the payload it carries,
//! and the target it lights; read from what each turn of input hands back and the lists its `Prepare`s send.

use super::super::input::tests::{Ev, child, column, hit, laid, over, point, press, raised, release, sized};
use super::super::orders::Order;
use super::super::tests::{last_list, screen};
use super::super::{Heard, Outcome, Screen};
use super::{Grip, Landing};
use crate::drag::tests::{Item, Spell};
use crate::drag::{DragSource, DropTarget, Payload};
use crate::input::Raised;
use crate::style::Style;
use crate::tree::{KeyPath, Node, Segment, Tree};
use crate::widget::{Arrange, Draws, Laid, Quad, Sample, Stateless, Widget};
use crate::widgets::{Button, Column, Stack};
use aethereal_actor::{ProtocolRef, SeatNumber, Seated};
use aethereal_data::ActorId;
use aethereal_math::Vec2;
use aethereal_render::draw_list::{Entry, Pointer};
use aethereal_render::text::Colour;
use aethereal_window::{Cursor, Hearer, Over, PointerEvent};

/// A method a drop of an item calls: the slot, then the item.
fn place_item(_: u32, _: Item) {}

/// A method a drop of a spell calls: the slot, then the spell.
fn place_spell(_: u32, _: Spell) {}

/// A button at `place` among its siblings, 40 by 20, raising `Click(click)`.
fn button(place: u32, click: u32) -> Node {
    Node::new(Segment::Place(place), Button::new(Ev::Click(click)), sized(40.0, 20.0), vec![])
}

/// The column the tests drag in: a button that drags the item numbered `item`, raising `Discarded` when nothing takes
/// it; a button that takes items, raising `Placed(3)`; one that takes no drops; and one that takes spells, raising
/// `Placed(4)`. Each is 20 high, the first at the top.
fn shelf(item: u32) -> Tree {
    column(vec![
        button(0, 1).with_drag(DragSource::new(Item { id: item }).outside(Ev::Discarded)),
        button(1, 2).with_drop(DropTarget::new(Ev::Placed(3), place_item)),
        button(2, 3),
        button(3, 4).with_drop(DropTarget::new(Ev::Placed(4), place_spell)),
    ])
}

/// The item numbered `item` as a drag carries it.
fn item(item: u32) -> Payload {
    Payload::of(Item { id: item })
}

/// A screen with the shelf of the item numbered 7 laid out, and the number of the entry of its source's hit.
fn shelved() -> (Screen, u32) {
    let mut screen = screen();
    laid(&mut screen, shelf(7), &[]);
    let source = hit(&screen, &child(0));

    (screen, source)
}

/// Presses the source through `source` at 10, 10 and moves the pointer `by` pixels across.
fn drag(screen: &mut Screen, source: u32, by: f32) {
    press(screen, source, 10.0, 10.0);
    point(screen, source, PointerEvent::PointerMoved { x: 10.0 + by, y: 10.0 });
}

/// Another actor, which a release may be over.
fn other() -> ProtocolRef<Hearer> {
    ProtocolRef::found(Seated { id: ActorId::new(2), seat: SeatNumber::new(2) })
}

/// The pointer of the hit over the widget at `path` in the list among `orders`.
fn pointer_of(screen: &Screen, orders: &[Order], path: &KeyPath) -> Pointer {
    let entry = hit(screen, path);
    let list = last_list(orders).unwrap_or_else(|| screen.listed.clone());
    let at = usize::try_from(entry).unwrap_or(usize::MAX);
    let Some(Entry::Hit { pointer, .. }) = list.get(at) else {
        panic!("the list holds the hit over {path:?}: {list:?}");
    };

    pointer.clone()
}

#[test]
fn a_press_released_within_the_threshold_is_a_click_and_begins_no_drag() {
    // Catches a drag begun on the press or on any move at all, so a click on a draggable slot by a hand that shakes a
    // pixel or two picks the item up and never selects it.
    let (mut screen, source) = shelved();

    drag(&mut screen, source, 3.0);
    let gripped = matches!(screen.grip, Grip::Pressed { .. });
    let clicked = raised(release(&mut screen, source, 13.0, 10.0, over(source, 13.0, 10.0)));

    assert!(gripped, "a move of 3 pixels at a scale of 1 holds the grip: {:?}", screen.grip);
    assert_eq!(clicked, vec![Raised::Event(Ev::Click(1))]);
    assert_eq!(screen.grip, Grip::Loose);
}

#[test]
fn past_the_threshold_a_drag_begins_and_its_release_raises_no_click() {
    // Catches a drag whose release over its own source still clicks it, so dragging an item off its slot and back
    // selects it as well; and a threshold that is never passed, so nothing is ever dragged.
    let (mut screen, source) = shelved();

    drag(&mut screen, source, 5.0);
    let dragging = matches!(screen.grip, Grip::Dragging { .. });
    let released = raised(release(&mut screen, source, 15.0, 10.0, over(source, 15.0, 10.0)));

    assert!(dragging, "a move of 5 pixels begins the drag: {:?}", screen.grip);
    assert_eq!(released, vec![Raised::Dropped(Ev::Discarded, item(7))], "its own source takes no drops");
}

#[test]
fn the_pressed_hit_declares_the_drag_s_cursor_once_the_drag_begins_and_its_own_after() {
    // Catches the drag's cursor declared on the press, so a click flashes a closed hand; never declared, so the window
    // shows the button's hand over the whole drag; and kept after the release, so the hand stays closed over the slot.
    let (mut screen, source) = shelved();

    drag(&mut screen, source, 3.0);
    let (_, pressed) = laid(&mut screen, shelf(7), &[]);
    let before = pointer_of(&screen, &pressed, &child(0));
    let source = hit(&screen, &child(0));
    point(&mut screen, source, PointerEvent::PointerMoved { x: 20.0, y: 10.0 });
    let (_, begun) = laid(&mut screen, shelf(7), &[]);
    let dragging = pointer_of(&screen, &begun, &child(0));
    let source = hit(&screen, &child(0));
    release(&mut screen, source, 20.0, 10.0, Over::Nothing);
    let (_, ended) = laid(&mut screen, shelf(7), &[]);
    let after = pointer_of(&screen, &ended, &child(0));

    assert_eq!(before, Pointer::Blocks(Cursor::Hand));
    assert_eq!(dragging, Pointer::Blocks(Cursor::Grabbing));
    assert_eq!(after, Pointer::Blocks(Cursor::Hand));
}

#[test]
fn a_drop_on_a_target_in_the_screen_raises_its_drop_with_the_payload_after_its_arguments() {
    // Catches a drop raised by the source rather than the target, the target's arguments lost, or the payload left
    // off, so `place(slot, item)` is called with nothing to place.
    let (mut screen, source) = shelved();
    let target = hit(&screen, &child(1));

    drag(&mut screen, source, 10.0);
    let dropped = raised(release(&mut screen, source, 10.0, 30.0, over(target, 10.0, 30.0)));

    assert_eq!(dropped, vec![Raised::Dropped(Ev::Placed(3), item(7))]);
}

#[test]
fn a_drop_within_a_target_reaches_the_target_around_the_widget_under_it() {
    // Catches a drop that looks only at the widget whose hit it was over, so a panel that takes items refuses one
    // dropped on a button inside it.
    let mut screen = screen();
    let panel = Node::new(Segment::Place(1), Column, Style::default(), vec![button(0, 2)])
        .with_drop(DropTarget::new(Ev::Placed(5), place_item));
    let source = button(0, 1).with_drag(DragSource::new(Item { id: 7 }));
    laid(&mut screen, column(vec![source, panel]), &[]);
    let source = hit(&screen, &child(0));
    let inner = hit(&screen, &KeyPath::of(&[Segment::Place(0), Segment::Place(1), Segment::Place(0)]));

    drag(&mut screen, source, 10.0);
    let dropped = raised(release(&mut screen, source, 10.0, 30.0, over(inner, 10.0, 30.0)));

    assert_eq!(dropped, vec![Raised::Dropped(Ev::Placed(5), item(7))]);
}

#[test]
fn a_drop_on_nothing_raises_the_source_s_drop_outside() {
    // Catches a drop that nothing takes forgotten, so an item dragged off every window stays held by nothing, never
    // dropped on the ground nor put back.
    let (mut screen, source) = shelved();

    drag(&mut screen, source, 10.0);
    let dropped = raised(release(&mut screen, source, 300.0, 300.0, Over::Nothing));

    assert_eq!(dropped, vec![Raised::Dropped(Ev::Discarded, item(7))]);
}

#[test]
fn a_drop_on_what_takes_no_drops_or_another_kind_raises_the_source_s_drop_outside() {
    // Catches a drop on a widget without `drop` taken as landed, and one on a target of another kind raised there, so
    // an item dropped on a spell slot is cast as a spell.
    let (mut screen, source) = shelved();
    let plain = hit(&screen, &child(2));
    let spells = hit(&screen, &child(3));

    drag(&mut screen, source, 10.0);
    let on_plain = raised(release(&mut screen, source, 10.0, 50.0, over(plain, 10.0, 50.0)));
    drag(&mut screen, source, 10.0);
    let on_spells = raised(release(&mut screen, source, 10.0, 70.0, over(spells, 10.0, 70.0)));

    assert_eq!(on_plain, vec![Raised::Dropped(Ev::Discarded, item(7))]);
    assert_eq!(on_spells, vec![Raised::Dropped(Ev::Discarded, item(7))]);
}

#[test]
fn a_drop_on_another_actor_is_left_to_mail_with_its_payload_entry_and_place() {
    // Catches a drop on another actor raised as one on nothing, so dragging an item onto a child panel drops it on the
    // ground; and the mail missing the entry or place the target finds its widget by, or the payload.
    let (mut screen, source) = shelved();

    drag(&mut screen, source, 10.0);
    let heard = release(&mut screen, source, 10.0, 10.0, Over::Entry { actor: other(), entry: 5, x: 3.0, y: 4.0 });
    let Heard { raised, mut away, .. } = heard;
    assert_eq!(away.len(), 1, "one drop is left to mail: {away:?}");
    let away = away.remove(0);

    assert_eq!(away.to, other());
    assert_eq!((away.payload, away.entry, away.at), (item(7), 5, Vec2::new(3.0, 4.0)));
    assert_eq!(away.source, child(0));
    let outside = away.outside.map(Raised::typed::<Ev>);
    assert!(matches!(outside, Some(Ok(Raised::Dropped(Ev::Discarded, _)))), "{outside:?}");
    assert!(raised.is_empty(), "nothing is raised before the actor answers: {raised:?}");
}

#[test]
fn a_drop_mailed_to_a_screen_raises_its_target_s_drop_or_is_refused() {
    // Catches the target's library finding its widget by something other than the entry the release was over, raising
    // a drop its target does not take, or taking a drop where nothing takes it, so the source never learns to put the
    // item back.
    let (mut screen, _) = shelved();
    let target = hit(&screen, &child(1));
    let plain = hit(&screen, &child(2));

    let taken = screen.dropped(target, &item(9));
    let on_plain = screen.dropped(plain, &item(9));
    let wrong_kind = screen.dropped(target, &Payload::of(Spell { id: 9 }));

    let Landing::Taken(heard) = taken else {
        panic!("the item's target takes it");
    };
    assert_eq!(raised(heard), vec![Raised::Dropped(Ev::Placed(3), item(9))]);
    assert!(matches!(on_plain, Landing::Refused));
    assert!(matches!(wrong_kind, Landing::Refused));
}

#[test]
fn a_refused_drop_raises_its_source_s_drop_outside() {
    // Catches a refusal that the source ignores, so an item dropped on an actor that took nothing is lost.
    let (mut screen, _) = shelved();

    let refused = raised(screen.refused(&child(0), item(7)));

    assert_eq!(refused, vec![Raised::Dropped(Ev::Discarded, item(7))]);
}

#[test]
fn a_drag_carries_the_payload_its_view_copied_when_the_press_reached_it() {
    // Catches a payload read again from the tree the release reaches, so an item dragged while the list it came from
    // was rebuilt is dropped as whatever item now stands at its place.
    let (mut screen, source) = shelved();
    let target = hit(&screen, &child(1));

    press(&mut screen, source, 10.0, 10.0);
    screen.rebuilt(shelf(8));
    point(&mut screen, source, PointerEvent::PointerMoved { x: 20.0, y: 10.0 });
    let dropped = raised(release(&mut screen, source, 10.0, 30.0, over(target, 10.0, 30.0)));

    assert_eq!(dropped, vec![Raised::Dropped(Ev::Placed(3), item(7))]);
}

/// A well that draws itself only while a drag it takes is over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Well;

impl Widget for Well {
    type State = Stateless;

    fn arrange(&self) -> Arrange {
        Arrange::Leaf
    }

    fn draw(&self, _: &Stateless, _: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        if !draws.drop_over() {
            return;
        }
        draws.quad(Quad {
            rect: laid.rect,
            clip: laid.clip,
            colour: Colour::opaque(0.2, 0.8, 0.2),
            border_width: 0.0,
            border_colour: Colour::CLEAR,
            radius: 0.0,
            sample: Sample::Solid,
        });
    }
}

#[test]
fn a_target_in_the_screen_draws_lit_while_a_drag_it_takes_is_over_it() {
    // Catches a target never told a drag is over it, so nothing shows where a drop would land; one told before the drag
    // begins, so a click lights it; and one lit by a drag it does not take, so a spell slot invites an item.
    let wells = |source: Node| {
        column(vec![
            source,
            Node::new(Segment::Place(1), Well, sized(40.0, 20.0), vec![])
                .with_drop(DropTarget::new(Ev::Placed(3), place_item)),
            Node::new(Segment::Place(2), Well, sized(40.0, 20.0), vec![])
                .with_drop(DropTarget::new(Ev::Placed(4), place_spell)),
        ])
    };
    let tree = || {
        wells(Node::new(Segment::Place(0), Stack, sized(40.0, 20.0), vec![]).with_drag(DragSource::new(Item { id: 7 })))
    };
    let mut screen = screen();
    laid(&mut screen, tree(), &[]);
    let source = hit(&screen, &child(0));
    let items = hit(&screen, &child(1));
    let draws = |orders: &[Order]| orders.iter().any(|order| matches!(order, Order::Instances { .. }));

    drag(&mut screen, source, 3.0);
    point(&mut screen, items, PointerEvent::PointerEntered);
    let (_, gripped) = laid(&mut screen, tree(), &[]);
    point(&mut screen, source, PointerEvent::PointerMoved { x: 10.0, y: 30.0 });
    let (lit, over_items) = laid(&mut screen, tree(), &[]);
    let (items, spells) = (hit(&screen, &child(1)), hit(&screen, &child(2)));
    point(&mut screen, items, PointerEvent::PointerLeft);
    point(&mut screen, spells, PointerEvent::PointerEntered);
    let (_, over_spells) = laid(&mut screen, tree(), &[]);

    assert!(!draws(&gripped), "no drag has begun: {gripped:?}");
    assert_eq!(lit, Outcome::Laid);
    assert!(draws(&over_items), "the item's well lights: {over_items:?}");
    assert!(!draws(&over_spells), "the spell's well takes no items: {over_spells:?}");
}

#[test]
fn a_drag_released_after_a_target_before_its_source_lit_up_still_drops() {
    // Catches the capture's release read against the newest list, in which the target before the source, drawing as
    // it lit, renumbered the source's hit, so the release reaches the target rather than the source and the drop is
    // lost, the drag left under way.
    let tree = || {
        column(vec![
            Node::new(Segment::Place(0), Well, sized(40.0, 20.0), vec![])
                .with_drop(DropTarget::new(Ev::Placed(3), place_item)),
            Node::new(Segment::Place(1), Stack, sized(40.0, 20.0), vec![]).with_drag(DragSource::new(Item { id: 7 })),
        ])
    };
    let mut screen = screen();
    laid(&mut screen, tree(), &[]);
    let source = hit(&screen, &child(1));
    let well = hit(&screen, &child(0));

    press(&mut screen, source, 10.0, 30.0);
    point(&mut screen, source, PointerEvent::PointerMoved { x: 10.0, y: 10.0 });
    point(&mut screen, well, PointerEvent::PointerEntered);
    laid(&mut screen, tree(), &[]);
    let renumbered = hit(&screen, &child(1));
    let lit = hit(&screen, &child(0));
    let dropped = raised(release(&mut screen, source, 10.0, 10.0, over(lit, 10.0, 10.0)));

    assert_ne!(renumbered, source, "the lit well's draw renumbers the source's hit");
    assert_eq!(dropped, vec![Raised::Dropped(Ev::Placed(3), item(7))]);
    assert_eq!(screen.grip, Grip::Loose);
}
