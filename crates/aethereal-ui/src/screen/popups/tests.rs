//! A screen's popups and tooltips: where their content is placed and listed, what opens and shuts them, what a modal
//! one holds, and a tooltip's wait, read from the lists each `Prepare` sends, where its layout put each widget, the
//! sleeps it runs, and the states its table keeps.

use super::super::input::tests::{
    Ev, child, column, hit, key_down, keys, laid, over, point, press, raised, release, sized,
};
use super::super::keys::Role;
use super::super::orders::Order;
use super::super::tests::screen;
use super::super::{Listed, Screen};
use super::{Tipping, place};
use crate::input::Raised;
use crate::style::{Length, Style};
use crate::table::Focus;
use crate::tree::{KeyPath, Node, Segment, Tree};
use crate::widget::{Arrange, Draws, Inside, Laid, Layer, Opens, Side, Tip, Widget};
use crate::widgets::{Button, Column, Opened, Popup, Scroll, Stack, Tooltip};
use aethereal_actor::{Answered, Slept};
use aethereal_data::Plain;
use aethereal_math::{Rect2, Vec2};
use aethereal_render::draw_list::{Entry, Pointer};
use aethereal_render::text::Colour;
use aethereal_window::{Cursor, PointerEvent};

/// A box `width` by `height` logical pixels, filled, so it draws.
fn filled(width: f32, height: f32) -> Style {
    let background = Colour::opaque(0.2, 0.4, 0.6);

    Style { width: Length::Pixels(width), height: Length::Pixels(height), background, ..Style::default() }
}

/// A filled box at `place` among its siblings, `width` by `height`.
fn block(place: u32, width: f32, height: f32) -> Node {
    Node::new(Segment::Place(place), Stack, filled(width, height), vec![])
}

/// A button at `place` among its siblings, 40 by 20, raising `Click(click)`.
fn button(place: u32, click: u32) -> Node {
    Node::new(Segment::Place(place), Button::new(Ev::Click(click)), sized(40.0, 20.0), vec![])
}

/// `popup` at `place` among its siblings, over `anchor` and `content`.
fn popup(place: u32, popup: Popup, anchor: Node, content: Vec<Node>) -> Node {
    let mut children = vec![anchor];
    children.extend(content);

    Node::new(Segment::Place(place), popup, Style::default(), children)
}

/// The path of `place`, then each of `deeper`, below the root.
fn path(places: &[u32]) -> KeyPath {
    let mut segments = vec![Segment::Place(0)];
    segments.extend(places.iter().map(|place| Segment::Place(*place)));

    KeyPath::of(&segments)
}

/// Where the screen's last layout put the widget at `path`.
fn placed(screen: &Screen, path: &KeyPath) -> Laid {
    let found = screen.unkept.placed.iter().find(|(placed, _)| placed == path).map(|(_, laid)| *laid);
    let Some(laid) = found else {
        panic!("{path:?} is laid out");
    };

    laid
}

/// Whether the popup at `path` is open, as the screen's table keeps it.
fn opened(screen: &Screen, path: &KeyPath) -> Opened {
    screen.table().state::<Popup>(path).map_or(Opened::Shut, |popped| popped.open)
}

/// Clicks the hit over the widget at `path`: a press and a release of the main button over it, and what it raised.
fn click(screen: &mut Screen, path: &KeyPath) -> Vec<Raised<Ev>> {
    let entry = hit(screen, path);
    press(screen, entry, 2.0, 2.0);

    raised(release(screen, entry, 2.0, 2.0, over(entry, 2.0, 2.0)))
}

/// The places in the screen's list of the draws of the widget at `path`.
fn draws(screen: &Screen, path: &KeyPath) -> Vec<usize> {
    let drawn = |listed: &Listed| matches!(listed, Listed::Draw(key) if key.path == *path);

    screen.listing.iter().enumerate().filter(|(_, listed)| drawn(listed)).map(|(at, _)| at).collect()
}

/// The sleeps among `orders`: each tooltip's wait, its delay and what it ends for.
fn sleeps(orders: &[Order]) -> Vec<Tipping> {
    orders
        .iter()
        .filter_map(|order| match order {
            Order::Sleep { context, .. } => Some(context.clone()),
            _ => None,
        })
        .collect()
}

/// The state of the tooltip at `path`, as the screen's table keeps it.
fn tip(screen: &Screen, path: &KeyPath) -> Tip {
    screen.table().state::<Tooltip>(path).unwrap_or_default()
}

/// Ends the sleep `tipping` names, as the job's answer does.
fn wake(screen: &mut Screen, tipping: Tipping) {
    screen.slept(Answered::Yes(Slept), tipping);
}

#[test]
fn a_popup_in_a_scroll_draws_after_the_rest_is_not_cut_by_the_scroll_and_follows_its_anchor() {
    // Catches content drawn in tree order, so a sibling after the popup covers it; content cut to the scroll its anchor
    // stands in, so a menu opened near a list's foot shows only its top; and content placed where the anchor was laid
    // out rather than where the scroll slid it, so it stays put while its anchor scrolls away.
    let mut screen = screen();
    let tree = || {
        let open = popup(1, Popup::new(), block(0, 40.0, 10.0), vec![block(1, 60.0, 30.0)]);
        let style = Style { width: Length::Pixels(200.0), height: Length::Pixels(50.0), ..Style::default() };
        let scroll = Node::new(
            Segment::Place(0),
            Scroll::down(),
            style,
            vec![block(0, 200.0, 40.0), open, block(2, 200.0, 100.0)],
        );

        column(vec![scroll, block(1, 200.0, 20.0)])
    };
    let (popup_at, content, after) = (path(&[0, 1]), path(&[0, 1, 1]), path(&[1]));
    laid(&mut screen, tree(), &[]);
    click(&mut screen, &popup_at);
    laid(&mut screen, tree(), &[]);

    let shown = placed(&screen, &content);
    let wheel = PointerEvent::Wheel { x: 10.0, y: 10.0, delta_x: 0.0, delta_y: 20.0 };
    let scroll = hit(&screen, &path(&[0]));
    point(&mut screen, scroll, wheel);
    laid(&mut screen, tree(), &[]);
    let scrolled = placed(&screen, &content);

    let whole = Rect2 { min: Vec2::ZERO, max: Vec2::new(200.0, 100.0) };
    assert_eq!(shown.rect, Rect2 { min: Vec2::new(0.0, 50.0), max: Vec2::new(60.0, 80.0) });
    assert_eq!(shown.clip, whole, "the content is cut by nothing around it");
    let first_content = draws(&screen, &content).into_iter().min();
    let last_after = draws(&screen, &after).into_iter().max();
    assert!(first_content > last_after, "the content draws after the sibling after it: {:?}", screen.listing);
    assert_eq!(scrolled.rect.min, Vec2::new(0.0, 30.0), "the content moved with its anchor as the scroll slid");
}

#[test]
fn content_that_does_not_fit_flips_to_the_roomier_side_and_shifts_inside_the_screen() {
    // Catches content placed on its side however little room there is, so a menu opened at the foot of the screen runs
    // off it; content that flips when it fits, or flips into less room; content shifted past the screen's far edge;
    // and content longer than the screen pushed past its start rather than starting there.
    let bounds = Rect2 { min: Vec2::ZERO, max: Vec2::new(200.0, 100.0) };
    let rect =
        |x: f32, y: f32, width: f32, height: f32| Rect2::from_origin_size(Vec2::new(x, y), Vec2::new(width, height));

    let fits = place(rect(10.0, 10.0, 40.0, 10.0), Vec2::new(60.0, 30.0), Side::Below, bounds);
    let flipped = place(rect(150.0, 80.0, 40.0, 10.0), Vec2::new(60.0, 30.0), Side::Below, bounds);
    let left = place(rect(170.0, 10.0, 20.0, 10.0), Vec2::new(40.0, 20.0), Side::Right, bounds);
    let squeezed = place(rect(0.0, 40.0, 20.0, 20.0), Vec2::new(30.0, 90.0), Side::Below, bounds);
    let towering = place(rect(0.0, 40.0, 20.0, 20.0), Vec2::new(30.0, 150.0), Side::Above, bounds);

    assert_eq!(fits, Vec2::new(10.0, 20.0));
    assert_eq!(flipped, Vec2::new(140.0, 50.0), "flipped above, and shifted left to stay inside");
    assert_eq!(left, Vec2::new(130.0, 10.0), "flipped to the left of its anchor");
    assert_eq!(squeezed, Vec2::new(0.0, 10.0), "neither side has room: kept below, shifted up inside");
    assert_eq!(towering, Vec2::new(0.0, 0.0), "longer than the screen: from its top");
}

#[test]
fn a_click_on_the_anchor_toggles_a_popup_drawn_and_reached_over_its_anchor() {
    // Catches a popup whose hit lies beneath its anchor's, so an anchor that is itself reached, a button, takes every
    // click and the popup never opens; and a popup that opens and never shuts from its anchor.
    let mut screen = screen();
    let tree = || column(vec![popup(0, Popup::new(), button(0, 1), vec![block(1, 60.0, 30.0)])]);
    let (popup_at, anchor) = (child(0), path(&[0, 0]));
    laid(&mut screen, tree(), &[]);

    let over_anchor = hit(&screen, &popup_at) > hit(&screen, &anchor);
    let opening = click(&mut screen, &popup_at);
    let first = opened(&screen, &popup_at);
    click(&mut screen, &popup_at);
    let second = opened(&screen, &popup_at);

    assert!(over_anchor, "the popup's hit is over its anchor's: {:?}", screen.listing);
    assert_eq!(opening, vec![], "the anchor's own click is not raised");
    assert_eq!(first, Opened::Open);
    assert_eq!(second, Opened::Shut);
}

#[test]
fn a_press_outside_escape_and_the_anchor_going_each_shut_a_popup() {
    // Catches a popup left open by a press elsewhere on its screen, by Escape, or after what it was opened from has
    // gone, which leaves content standing against nothing; and a popup that shuts on a press within its own content.
    let mut screen = screen();
    let tree = |anchored: bool| {
        let content = vec![button(1, 2)];
        let held = if anchored {
            popup(0, Popup::new(), block(0, 40.0, 10.0), content)
        } else {
            Node::new(Segment::Place(0), Popup::new(), Style::default(), content)
        };

        column(vec![held, button(1, 1)])
    };
    let (popup_at, inside, outside) = (child(0), path(&[0, 1]), child(1));
    laid(&mut screen, tree(true), &[]);

    click(&mut screen, &popup_at);
    laid(&mut screen, tree(true), &[]);
    let within = hit(&screen, &inside);
    press(&mut screen, within, 2.0, 2.0);
    let pressed_within = opened(&screen, &popup_at);
    let elsewhere = hit(&screen, &outside);
    press(&mut screen, elsewhere, 2.0, 2.0);
    let pressed_outside = opened(&screen, &popup_at);

    laid(&mut screen, tree(true), &[]);
    click(&mut screen, &popup_at);
    laid(&mut screen, tree(true), &[]);
    let declared = keys(&screen, Role::Screen);
    key_down(&mut screen, declared, "Escape", "Escape");
    let escaped = opened(&screen, &popup_at);

    laid(&mut screen, tree(true), &[]);
    click(&mut screen, &popup_at);
    laid(&mut screen, tree(false), &[]);
    let unanchored = opened(&screen, &popup_at);
    laid(&mut screen, tree(true), &[]);
    let anchored_again = opened(&screen, &popup_at);

    assert_eq!(pressed_within, Opened::Open);
    assert_eq!(pressed_outside, Opened::Shut);
    assert_eq!(escaped, Opened::Shut);
    assert_eq!(unanchored, Opened::Shut);
    assert_eq!(anchored_again, Opened::Shut, "it does not open again with its anchor");
}

#[test]
fn a_popup_that_closes_on_a_choice_shuts_once_the_choice_is_raised_and_one_that_stays_does_not() {
    // Catches a menu that stays open once a choice in it is made, which is what a dropdown needs from it; one that
    // shuts before the choice's own click is raised, losing it; one shut by a click on its empty space, which chooses
    // nothing; and a popup of settings shut by the first click in it.
    let mut screen = screen();
    let tree = |kind: Popup| {
        let content = Node::new(Segment::Place(1), Column, Style::default(), vec![button(0, 5), block(1, 40.0, 10.0)]);
        column(vec![popup(0, kind, block(0, 40.0, 10.0), vec![content])])
    };
    let (popup_at, choice, blank) = (child(0), path(&[0, 1, 0]), path(&[0, 1, 1]));

    laid(&mut screen, tree(Popup::new().closing()), &[]);
    click(&mut screen, &popup_at);
    laid(&mut screen, tree(Popup::new().closing()), &[]);
    click(&mut screen, &blank);
    let unchosen = opened(&screen, &popup_at);
    let chosen = click(&mut screen, &choice);
    let closing = opened(&screen, &popup_at);

    laid(&mut screen, tree(Popup::new()), &[]);
    click(&mut screen, &popup_at);
    laid(&mut screen, tree(Popup::new()), &[]);
    click(&mut screen, &choice);
    let staying = opened(&screen, &popup_at);

    assert_eq!(unchosen, Opened::Open);
    assert_eq!(chosen, vec![Raised::Event(Ev::Click(5))]);
    assert_eq!(closing, Opened::Shut);
    assert_eq!(staying, Opened::Open);
}

#[test]
fn a_modal_popup_s_backdrop_blocks_the_screen_beneath_and_tab_stays_within_it() {
    // Catches a modal popup whose rest of the screen still hears clicks; a backdrop listed over its own content, so
    // nothing in it can be pressed; Tab walking out of a modal onto what it blocks; and a backdrop press that leaves it
    // open, with no way out by the pointer.
    let mut screen = screen();
    let content = || Node::new(Segment::Place(1), Column, Style::default(), vec![button(0, 2), button(1, 3)]);
    let tree = || column(vec![button(0, 1), popup(1, Popup::new().modal(), block(0, 40.0, 10.0), vec![content()])]);
    let (popup_at, first, second) = (child(1), path(&[1, 1, 0]), path(&[1, 1, 1]));
    laid(&mut screen, tree(), &[]);
    click(&mut screen, &popup_at);
    laid(&mut screen, tree(), &[]);

    let backdrop = screen.listing.iter().position(|listed| *listed == Listed::Backdrop(popup_at.clone()));
    let Some(backdrop) = backdrop else {
        panic!("a modal popup lists its backdrop: {:?}", screen.listing);
    };
    let beneath = hit(&screen, &child(0));
    let within = hit(&screen, &first);
    let Some(Entry::Hit { rect, pointer, .. }) = screen.listed.get(backdrop).cloned() else {
        panic!("the backdrop is a hit");
    };
    let tab = |screen: &mut Screen| {
        let declared = keys(screen, Role::Screen);
        key_down(screen, declared, "Tab", "Tab");
        screen.table().focus().clone()
    };
    let tabbed = [tab(&mut screen), tab(&mut screen), tab(&mut screen)];
    let entry = u32::try_from(backdrop).unwrap_or(u32::MAX);
    press(&mut screen, entry, 150.0, 90.0);
    let pressed_behind = opened(&screen, &popup_at);

    assert!(beneath < entry, "the backdrop is over the rest of the screen");
    assert!(within > entry, "the content is over the backdrop");
    assert_eq!(rect, Rect2 { min: Vec2::ZERO, max: Vec2::new(200.0, 100.0) });
    assert_eq!(pointer, Pointer::Blocks(Cursor::Default));
    assert_eq!(tabbed, [Focus::On(first.clone()), Focus::On(second), Focus::On(first)]);
    assert_eq!(pressed_behind, Opened::Shut);
}

#[test]
fn open_state_survives_a_rebuild_by_key_and_a_popup_at_another_key_starts_shut() {
    // Catches open state kept by the popup's place among what is shown, or reset by a rebuild that changed what the
    // popup holds, so a menu shuts whenever the screen's data changes; and state carried over to another key.
    let mut screen = screen();
    let tree = |place: u32, wide: f32| {
        let held = popup(place, Popup::new(), block(0, 40.0, 10.0), vec![block(1, wide, 30.0)]);
        let mut children = Vec::new();
        if place > 0 {
            children.push(block(0, 40.0, 10.0));
        }
        children.push(held);

        column(children)
    };
    laid(&mut screen, tree(0, 60.0), &[]);
    click(&mut screen, &child(0));

    laid(&mut screen, tree(0, 80.0), &[]);
    let rebuilt = opened(&screen, &child(0));
    let content = placed(&screen, &path(&[0, 1])).rect.width();
    laid(&mut screen, tree(1, 80.0), &[]);
    let elsewhere = opened(&screen, &child(1));

    assert_eq!(rebuilt, Opened::Open);
    assert!((content - 80.0).abs() < f32::EPSILON, "the rebuilt content is laid out open: {content}");
    assert_eq!(elsewhere, Opened::Shut);
}

/// Whether a disclosure's content shows: open from the first, until its screen shuts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Plain)]
#[kind(name = "aether.ui.test.disclosed")]
enum Disclosed {
    #[default]
    Open,
    Shut,
}

/// A program's own primitive that claims a layer of its screen as a `Popup` does, through `Widget`'s methods alone: its
/// first child its anchor, and its content open below it until its screen shuts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Disclosure;

impl Widget for Disclosure {
    type State = Disclosed;

    fn arrange(&self) -> Arrange {
        Arrange::Popup(Layer { side: Side::Below, opens: Opens::Pressed { modal: false, inside: Inside::Stays } })
    }

    fn draw(&self, _: &Disclosed, _: &Style, _: &Laid, _: &mut Draws<'_, '_>) {}

    fn open(&self, disclosed: &Disclosed) -> bool {
        *disclosed == Disclosed::Open
    }

    fn shut(&self, disclosed: &mut Disclosed) {
        *disclosed = Disclosed::Shut;
    }
}

#[test]
fn a_program_s_own_primitive_opens_a_layer_and_is_shut_through_its_widget_methods() {
    // Catches a top layer only the stock widgets reach, as when the library knew a popup by its type: a program's own
    // primitive that arranges its children as a popup would have its content laid out in its box and listed in tree
    // order, beneath what follows it, and a press outside it would leave it open.
    let mut screen = screen();
    let tree = || {
        let content = vec![block(0, 40.0, 10.0), block(1, 60.0, 30.0)];
        column(vec![Node::new(Segment::Place(0), Disclosure, Style::default(), content), button(1, 1)])
    };
    let (disclosure, content, outside) = (child(0), path(&[0, 1]), child(1));
    laid(&mut screen, tree(), &[]);

    let shown = placed(&screen, &content);
    let over_outside = hit(&screen, &content) > hit(&screen, &outside);
    let elsewhere = hit(&screen, &outside);
    press(&mut screen, elsewhere, 2.0, 2.0);
    let pressed_outside = screen.table().state::<Disclosure>(&disclosure);
    laid(&mut screen, tree(), &[]);
    let shut = draws(&screen, &content);

    assert_eq!(shown.rect, Rect2 { min: Vec2::new(0.0, 10.0), max: Vec2::new(60.0, 40.0) }, "placed below its anchor");
    assert!(over_outside, "its content is listed over what follows it: {:?}", screen.listing);
    assert_eq!(pressed_outside, Some(Disclosed::Shut));
    assert_eq!(shut, vec![], "its content draws no more once it is shut");
}

/// A tree of a tooltip over a button raising `Click(1)`, its content a column holding a button raising `Click(2)`.
fn tipped() -> Tree {
    let content = Node::new(Segment::Place(1), Column, filled(60.0, 20.0), vec![button(0, 2)]);
    let tooltip = Node::new(Segment::Place(0), Tooltip::new(), Style::default(), vec![button(0, 1), content]);

    column(vec![tooltip])
}

/// Brings the pointer onto, or off, the hit over the widget at `path`, as the window does.
fn hover(screen: &mut Screen, path: &KeyPath, onto: bool) {
    let event = if onto {
        PointerEvent::PointerEntered
    } else {
        PointerEvent::PointerLeft
    };
    let entry = hit(screen, path);
    point(screen, entry, event);
}

#[test]
fn a_tooltip_opens_only_once_the_sleep_of_the_hover_now_ends() {
    // Catches a tooltip that opens as the pointer comes, with no wait; one opened by the sleep of a hover that ended
    // before it did; and one opened by a stale sleep, a hover's that ended, while a later hover still waits.
    let mut screen = screen();
    let (tooltip, anchor) = (child(0), path(&[0, 0]));
    laid(&mut screen, tipped(), &[]);

    hover(&mut screen, &anchor, true);
    let (_, came) = laid(&mut screen, tipped(), &[]);
    let waiting = tip(&screen, &tooltip);
    let (_, still) = laid(&mut screen, tipped(), &[]);
    hover(&mut screen, &anchor, false);
    laid(&mut screen, tipped(), &[]);
    let first = sleeps(&came);
    for tipping in first.clone() {
        wake(&mut screen, tipping);
    }
    let ended_first = tip(&screen, &tooltip);

    hover(&mut screen, &anchor, true);
    let (_, again) = laid(&mut screen, tipped(), &[]);
    let second = sleeps(&again);
    for tipping in first.clone() {
        wake(&mut screen, tipping);
    }
    let stale = tip(&screen, &tooltip);
    for tipping in second.clone() {
        wake(&mut screen, tipping);
    }
    let woke = tip(&screen, &tooltip);
    laid(&mut screen, tipped(), &[]);
    let shown = draws(&screen, &tooltip.child(Segment::Place(1)));

    assert_eq!(first, vec![Tipping { path: tooltip.clone(), hover: 1 }]);
    assert_eq!(waiting, Tip::Waiting { hover: 1 });
    assert_eq!(sleeps(&still), vec![], "a hover waits on one sleep");
    assert_eq!(ended_first, Tip::Away, "the hover ended before its sleep did");
    assert_eq!(second, vec![Tipping { path: tooltip, hover: 2 }]);
    assert_eq!(stale, Tip::Waiting { hover: 2 }, "the first hover's sleep opens nothing for the second");
    assert_eq!(woke, Tip::Open);
    assert!(!shown.is_empty(), "its content draws once it is open: {:?}", screen.listing);
}

#[test]
fn a_tooltip_never_takes_focus_or_blocks_a_click_and_a_press_shuts_it() {
    // Catches a tooltip that blocks the pointer from its anchor or from the game beneath it; one whose content takes
    // presses or Tab; a press through its own hit that moves focus; and a tooltip left open over what was pressed.
    let mut screen = screen();
    let (tooltip, anchor, inner) = (child(0), path(&[0, 0]), path(&[0, 1, 0]));
    let tab = ["Tab"];
    laid(&mut screen, tipped(), &tab);
    hover(&mut screen, &anchor, true);
    let (_, came) = laid(&mut screen, tipped(), &tab);
    for tipping in sleeps(&came) {
        wake(&mut screen, tipping);
    }
    laid(&mut screen, tipped(), &tab);

    let own = screen.listed.get(usize::try_from(hit(&screen, &tooltip)).unwrap_or(usize::MAX)).cloned();
    let inner_listed = screen.listing.contains(&Listed::Hit(inner));
    let declared = keys(&screen, Role::Screen);
    key_down(&mut screen, declared, "Tab", "Tab");
    key_down(&mut screen, declared, "Tab", "Tab");
    let tabbed = screen.table().focus().clone();
    let open = tip(&screen, &tooltip);
    let clicked = click(&mut screen, &anchor);
    let dismissed = tip(&screen, &tooltip);
    let passing = hit(&screen, &tooltip);
    press(&mut screen, passing, 1.0, 1.0);
    let after_own = screen.table().focus().clone();

    assert!(matches!(own, Some(Entry::Hit { pointer: Pointer::Passes(_), .. })), "{own:?}");
    assert!(!inner_listed, "its content lists no hit");
    assert_eq!(tabbed, Focus::On(anchor.clone()), "Tab passes its content's button");
    assert_eq!(open, Tip::Open);
    assert_eq!(clicked, vec![Raised::Event(Ev::Click(1))]);
    assert_eq!(dismissed, Tip::Dismissed);
    assert_eq!(after_own, Focus::On(anchor), "a press through its hit leaves focus where it was");
}
