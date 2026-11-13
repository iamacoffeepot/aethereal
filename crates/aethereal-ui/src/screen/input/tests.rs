//! A screen's input: where the pointer and the keys its list reaches go, what its widgets raise, how its focus moves
//! and what keys it declares for it, read from what each turn of input hands back and the lists its `Prepare`s send.

use super::super::keys::Role;
use super::super::orders::Order;
use super::super::tests::{answered, id, last_list, mono, screen};
use super::super::{Listed, Outcome, Screen};
use super::Heard;
use crate::input::{Input, Pointing, Raised};
use crate::style::{Length, Style};
use crate::table::Focus;
use crate::tree::{ItemKey, KeyPath, Node, Segment, Tree};
use crate::widget::{Arrange, Draws, Laid, Stateless, Widget};
use crate::widgets::{Button, Column, Press, Slider, Sliding, Tab, Tabs, Text, TextInput, Toggle};
use aethereal_actor::{ProtocolRef, SeatNumber, Seated};
use aethereal_data::ActorId;
use aethereal_render::draw_list::{Entry, Pointer};
use aethereal_render::text::{Colour, Font};
use aethereal_window::WindowId;
use aethereal_window::{AllowIme, Button as Pressed, Cursor, KeyEvent, KeySet, Keyed, Over, Pointed, PointerEvent};

/// The window the screen draws in.
pub(in crate::screen) fn window() -> WindowId {
    id(0)
}

/// The events the tests' widgets raise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::screen) enum Ev {
    Click(u32),
    Changed,
    Submitted,
    Flipped,
    Slid,
    Opened,
    Placed(u32),
    Discarded,
}

/// The screen's actor, as the window names it.
pub(in crate::screen) fn me() -> ActorId {
    ActorId::new(1)
}

/// What a release over an entry of the screen's own list names it as.
pub(in crate::screen) fn mine() -> ProtocolRef<aethereal_window::Hearer> {
    ProtocolRef::found(Seated { id: me(), seat: SeatNumber::new(1) })
}

/// A box `width` by `height` logical pixels, its text in the renderer's monospace font.
pub(in crate::screen) fn sized(width: f32, height: f32) -> Style {
    Style { width: Length::Pixels(width), height: Length::Pixels(height), font: Font::Mono, ..Style::default() }
}

/// A tree of a column over `children`, at the screen's top left.
pub(in crate::screen) fn column(children: Vec<Node>) -> Tree {
    Tree::new(Node::new(Segment::Place(0), Column, Style::default(), children))
}

/// The path of the column's child at `place`.
pub(in crate::screen) fn child(place: u32) -> KeyPath {
    KeyPath::of(&[Segment::Place(0), Segment::Place(place)])
}

/// Lays `tree` out on `screen` with `hotkeys`, every draw answered made: what the `Prepare` did, and every order sent
/// on the way.
pub(in crate::screen) fn laid(screen: &mut Screen, tree: Tree, hotkeys: &[&str]) -> (Outcome, Vec<Order>) {
    let prepared = screen.prepare(tree, hotkeys);

    (prepared.outcome, answered(screen, &prepared.orders, false))
}

/// The number of the entry of the screen's list that is the hit over the widget at `path`.
pub(in crate::screen) fn hit(screen: &Screen, path: &KeyPath) -> u32 {
    let listed = Listed::Hit(path.clone());
    let Some(at) = screen.listing.iter().position(|entry| *entry == listed) else {
        panic!("the screen lists a hit over {path:?}: {:?}", screen.listing);
    };

    u32::try_from(at).unwrap_or(u32::MAX)
}

/// The number of the entry of the screen's list that declares the keys of `role`.
pub(in crate::screen) fn keys(screen: &Screen, role: Role) -> u32 {
    let listed = Listed::Keys(role);
    let Some(at) = screen.listing.iter().position(|entry| *entry == listed) else {
        panic!("the screen declares keys of {role:?}: {:?}", screen.listing);
    };

    u32::try_from(at).unwrap_or(u32::MAX)
}

/// `event` through the entry `entry` of the screen's list.
pub(in crate::screen) fn point(screen: &mut Screen, entry: u32, event: PointerEvent) -> Heard {
    screen.pointed(Pointed { window: window(), entry, event }, me())
}

/// A press of the main button at `x`, `y` through the entry `entry`.
pub(in crate::screen) fn press(screen: &mut Screen, entry: u32, x: f32, y: f32) -> Heard {
    point(screen, entry, PointerEvent::PointerDown { x, y, button: Pressed::Main })
}

/// A release of the main button at `x`, `y` through the entry `entry`, over `over`.
pub(in crate::screen) fn release(screen: &mut Screen, entry: u32, x: f32, y: f32, over: Over) -> Heard {
    point(screen, entry, PointerEvent::PointerUp { x, y, button: Pressed::Main, over })
}

/// The release's place over the entry `entry` of the screen's own list.
pub(in crate::screen) fn over(entry: u32, x: f32, y: f32) -> Over {
    Over::Entry { actor: mine(), entry, x, y }
}

/// The key at `code`, meaning `key`, pressed through the entry `entry`.
pub(in crate::screen) fn key_down(screen: &mut Screen, entry: u32, code: &str, key: &str) -> Heard {
    let event = KeyEvent::KeyDown { code: String::from(code), key: String::from(key), repeat: false };

    screen.keyed(Keyed { window: window(), entry, event }, &[])
}

/// `text` typed through the entry `entry`.
pub(in crate::screen) fn typed(screen: &mut Screen, entry: u32, text: &str) -> Heard {
    screen.keyed(Keyed { window: window(), entry, event: KeyEvent::Text { text: String::from(text) } }, &[])
}

/// What `heard` raised, as the tests' events.
pub(in crate::screen) fn raised(heard: Heard) -> Vec<Raised<Ev>> {
    heard
        .raised
        .into_iter()
        .map(|raised| match raised.typed::<Ev>() {
            Ok(raised) => raised,
            Err(other) => panic!("the tests' widgets raise the tests' events: {other:?}"),
        })
        .collect()
}

/// The codes of the keys `heard` left for the screen.
pub(in crate::screen) fn left_over(heard: &Heard) -> Vec<&str> {
    heard.keys.iter().map(|key| key.code.as_str()).collect()
}

/// A text input of `text`, raising `Changed`, and `Submitted` with Enter, 100 by 20 logical pixels.
pub(in crate::screen) fn text_input(text: &str) -> Node {
    Node::new(Segment::Place(0), TextInput::new(text, Ev::Changed).submit(Ev::Submitted), sized(100.0, 20.0), vec![])
}

/// A screen whose monospace font's metrics have come, so its texts lay out.
pub(in crate::screen) fn screen_with_fonts() -> Screen {
    let mut screen = screen();
    screen.metrics_read(Font::Mono, mono(), Vec::new());

    screen
}

#[test]
fn a_click_is_a_press_and_a_release_over_the_same_button_and_not_one_released_elsewhere() {
    // Catches a click raised on the press, so a press dragged off to cancel still acts; one raised on any release, so a
    // drag from the button dropped on another acts; and the arguments of the event lost or swapped on the way.
    let mut screen = screen();
    let button = Node::new(Segment::Place(0), Button::new(Ev::Click(7)), sized(40.0, 20.0), vec![]);
    laid(&mut screen, column(vec![button]), &[]);
    let entry = hit(&screen, &child(0));

    let pressed = raised(press(&mut screen, entry, 10.0, 10.0));
    let clicked = raised(release(&mut screen, entry, 12.0, 10.0, over(entry, 12.0, 10.0)));
    press(&mut screen, entry, 10.0, 10.0);
    let elsewhere = raised(release(&mut screen, entry, 90.0, 90.0, Over::Nothing));

    assert_eq!(pressed, vec![]);
    assert_eq!(clicked, vec![Raised::Event(Ev::Click(7))]);
    assert_eq!(elsewhere, vec![]);
}

#[test]
fn a_click_after_the_view_rebuilt_raises_the_arguments_the_new_view_copied() {
    // Catches input routed through the tree built at the last `Prepare` after a method changed what the view copies,
    // so a second click before the next frame raises the first's stale arguments: two quick clicks that each should
    // take the next item both take the same one.
    let mut screen = screen();
    let tree =
        |click| column(vec![Node::new(Segment::Place(0), Button::new(Ev::Click(click)), sized(40.0, 20.0), vec![])]);
    laid(&mut screen, tree(1), &[]);
    let entry = hit(&screen, &child(0));
    let click = |screen: &mut Screen| {
        press(screen, entry, 5.0, 5.0);
        raised(release(screen, entry, 5.0, 5.0, over(entry, 5.0, 5.0)))
    };

    let first = click(&mut screen);
    screen.rebuilt(tree(2));
    let second = click(&mut screen);

    assert_eq!(first, vec![Raised::Event(Ev::Click(1))]);
    assert_eq!(second, vec![Raised::Event(Ev::Click(2))]);
}

#[test]
fn a_screen_whose_interaction_state_changed_draws_again_though_its_tree_is_the_same() {
    // Catches a hover that changes the button's state but leaves the screen skipping its identical tree, so the button
    // never lights up until something else changes.
    let mut screen = screen();
    let tree = || column(vec![Node::new(Segment::Place(0), Button::new(Ev::Click(1)), sized(40.0, 20.0), vec![])]);
    laid(&mut screen, tree(), &[]);
    let entry = hit(&screen, &child(0));

    point(&mut screen, entry, PointerEvent::PointerEntered);
    let (hovered, orders) = laid(&mut screen, tree(), &[]);
    let (again, _) = laid(&mut screen, tree(), &[]);

    assert_eq!(hovered, Outcome::Laid);
    let tinted = orders.iter().any(|order| matches!(order, Order::Instances { .. }));
    assert!(tinted, "the hovered button draws its tint: {orders:?}");
    assert_eq!(again, Outcome::Unchanged);
}

#[test]
fn hover_and_press_survive_a_rebuild_of_a_different_tree() {
    // Catches a button's state made anew whenever the view rebuilds a changed tree, so a press held while a label
    // ticks over is forgotten, and its release clicks nothing.
    let mut screen = screen();
    let button = |click| Node::new(Segment::Place(0), Button::new(Ev::Click(click)), sized(40.0, 20.0), vec![]);
    laid(&mut screen, column(vec![button(1)]), &[]);
    let entry = hit(&screen, &child(0));
    point(&mut screen, entry, PointerEvent::PointerEntered);
    press(&mut screen, entry, 10.0, 10.0);

    let label = Node::new(Segment::Place(1), Text::new("Gold: 12"), sized(40.0, 20.0), vec![]);
    laid(&mut screen, column(vec![button(2), label]), &[]);

    let kept = screen.table().state::<Button<Ev>>(&child(0));
    assert_eq!(kept, Some(Press { hovered: true, pressed: true }));
}

#[test]
fn tab_moves_focus_through_focusable_widgets_in_tree_order_and_shift_tab_back() {
    // Catches Tab that follows the order widgets were last drawn or hit rather than the tree's; one that stops at the
    // end rather than going round; a widget nested in another container passed over; and Shift-Tab that goes forward.
    let mut screen = screen();
    let nested = Node::new(Segment::Place(0), Toggle::new(false, Ev::Flipped), sized(20.0, 10.0), vec![]);
    let tree = column(vec![
        Node::new(Segment::Place(0), Button::new(Ev::Click(1)), sized(40.0, 20.0), vec![]),
        Node::new(Segment::Place(1), Column, Style::default(), vec![nested]),
        Node::new(Segment::Place(2), Slider::new(0.0, 0.0, 1.0, 0.0, Ev::Slid), sized(40.0, 10.0), vec![]),
    ]);
    laid(&mut screen, tree, &["Tab"]);
    let tabs = keys(&screen, Role::Screen);
    let modifiers = keys(&screen, Role::Modifiers);
    let focus = |screen: &Screen| screen.table().focus().clone();

    let mut forward = Vec::new();
    for _ in 0..4 {
        key_down(&mut screen, tabs, "Tab", "Tab");
        forward.push(focus(&screen));
    }
    key_down(&mut screen, modifiers, "ShiftLeft", "Shift");
    key_down(&mut screen, tabs, "Tab", "Tab");
    let back = focus(&screen);

    let toggle = KeyPath::of(&[Segment::Place(0), Segment::Place(1), Segment::Place(0)]);
    let on = Focus::On;
    assert_eq!(forward, vec![on(child(0)), on(toggle), on(child(2)), on(child(0))]);
    assert_eq!(back, on(child(2)));
}

#[test]
fn a_focused_widget_whose_key_goes_clears_focus_and_stops_the_input_method() {
    // Catches focus left on a key no widget has, so the screen goes on declaring every key for a text input that is
    // gone and the game hears no key again; and the window's input method left on, taking the game's keys into
    // compositions.
    let mut screen = screen_with_fonts();
    laid(&mut screen, column(vec![text_input("gold")]), &[]);
    let entry = hit(&screen, &child(0));

    let focused = press(&mut screen, entry, 5.0, 5.0);
    let (_, orders) = laid(&mut screen, column(vec![]), &[]);

    assert_eq!(focused.imes, vec![AllowIme { window: window(), allowed: true }]);
    assert_eq!(screen.table().focus(), &Focus::Nowhere);
    let stopped = orders.contains(&Order::AllowIme(AllowIme { window: window(), allowed: false }));
    assert!(stopped, "the input method is stopped: {orders:?}");
}

#[test]
fn a_focused_text_input_declares_every_key_blocking_and_gives_them_up_on_blur() {
    // Catches a text input that takes focus but leaves the keys declared as before, so a letter typed into it moves
    // the game beneath; and one that keeps every key after Escape, so the game never hears a key again.
    let mut screen = screen_with_fonts();
    let tree = || column(vec![text_input("gold")]);
    laid(&mut screen, tree(), &[]);
    let entry = hit(&screen, &child(0));
    press(&mut screen, entry, 5.0, 5.0);
    let (_, focused) = laid(&mut screen, tree(), &[]);
    let declared = keys(&screen, Role::Screen);

    let blurred = key_down(&mut screen, declared, "Escape", "Escape");
    let (_, after) = laid(&mut screen, tree(), &[]);

    let every = Entry::Keys { frame: screen.target.frame, keys: KeySet::All, blocks: true };
    let first = last_list(&focused).and_then(|list| list.first().cloned());
    assert_eq!(first, Some(every));
    assert_eq!(blurred.imes, vec![AllowIme { window: window(), allowed: false }]);
    let keyless = last_list(&after).is_some_and(|list| !list.iter().any(|entry| matches!(entry, Entry::Keys { .. })));
    assert!(keyless, "no keys are declared without focus or hotkeys: {after:?}");
}

#[test]
fn a_key_no_widget_uses_reaches_the_screen_and_a_typed_letter_does_not() {
    // Catches a key the focused text input passed being dropped, so a screen's F2 does nothing while it is typed in; a
    // letter typed into it reaching the screen's hotkeys too; and a hotkey that never reaches the screen without focus.
    let mut screen = screen_with_fonts();
    let tree = || column(vec![text_input("gold")]);
    laid(&mut screen, tree(), &["KeyI"]);
    let declared = keys(&screen, Role::Screen);
    let hotkey = key_down(&mut screen, declared, "KeyI", "i");

    let entry = hit(&screen, &child(0));
    press(&mut screen, entry, 5.0, 5.0);
    laid(&mut screen, tree(), &["KeyI"]);
    let declared = keys(&screen, Role::Screen);
    let passed = key_down(&mut screen, declared, "F2", "F2");
    let letter = key_down(&mut screen, declared, "KeyI", "i");

    assert_eq!(left_over(&hotkey), vec!["KeyI"]);
    assert_eq!(left_over(&passed), vec!["F2"]);
    assert_eq!(left_over(&letter), Vec::<&str>::new());
}

#[test]
fn typed_text_goes_to_the_focused_input_and_replaces_its_selection() {
    // Catches typed text that goes nowhere, or to a widget without focus; and a select-all chord that is lost, so
    // typing after it appends rather than replaces.
    let mut screen = screen_with_fonts();
    let tree = || column(vec![text_input("gold")]);
    laid(&mut screen, tree(), &[]);
    let entry = hit(&screen, &child(0));
    press(&mut screen, entry, 99.0, 5.0);
    laid(&mut screen, tree(), &[]);
    let declared = keys(&screen, Role::Screen);

    let appended = raised(typed(&mut screen, declared, "en"));
    key_down(&mut screen, declared, "ControlLeft", "Control");
    key_down(&mut screen, declared, "KeyA", "a");
    let replaced = raised(typed(&mut screen, declared, "x"));
    let submitted = raised(key_down(&mut screen, declared, "Enter", "Enter"));

    assert_eq!(appended, vec![Raised::Text(Ev::Changed, String::from("golden"))]);
    assert_eq!(replaced, vec![Raised::Text(Ev::Changed, String::from("x"))]);
    assert_eq!(submitted, vec![Raised::Text(Ev::Submitted, String::from("gold"))]);
}

#[test]
fn a_key_the_screen_takes_first_reaches_it_past_a_focused_input_and_types_nothing() {
    // Catches a console's toggle typed into its own line, as a focused text input uses every key that types, so the
    // console never closes; the text of a key the screen took typed into the input the screen focused as it took it,
    // so the console opens with the toggle in its line; and an input the screen focuses of its own accord left without
    // focus, so the keys typed at the open console reach the game.
    let mut screen = screen_with_fonts();
    let tree = || column(vec![text_input("")]);
    let first = ["Backquote"];
    let down = |screen: &mut Screen, entry: u32, code: &str, key: &str| {
        let event = KeyEvent::KeyDown { code: String::from(code), key: String::from(key), repeat: false };
        screen.keyed(Keyed { window: window(), entry, event }, &first)
    };
    laid(&mut screen, tree(), &first);
    let declared = keys(&screen, Role::Screen);

    let opened = down(&mut screen, declared, "Backquote", "`");
    screen.focus(&child(0));
    let toggle_typed = raised(typed(&mut screen, declared, "`"));
    laid(&mut screen, tree(), &first);
    let every = keys(&screen, Role::Screen);
    let letter = down(&mut screen, every, "KeyM", "m");
    let letter_typed = raised(typed(&mut screen, every, "m"));
    let closed = down(&mut screen, every, "Backquote", "`");
    let closing_typed = raised(typed(&mut screen, every, "`"));

    assert_eq!(left_over(&opened), vec!["Backquote"]);
    assert_eq!(toggle_typed, vec![]);
    assert_eq!(screen.table().focus(), &Focus::On(child(0)));
    assert_eq!(left_over(&letter), Vec::<&str>::new());
    assert_eq!(letter_typed, vec![Raised::Text(Ev::Changed, String::from("m"))]);
    assert_eq!(left_over(&closed), vec!["Backquote"]);
    assert_eq!(closing_typed, vec![]);
}

#[test]
fn a_text_its_screen_sets_puts_the_input_s_caret_at_its_end() {
    // Catches a line recalled from a history, or completed, whose caret stays where it stood in the line before, so
    // what is typed next lands in its middle or at its start.
    let mut screen = screen_with_fonts();
    laid(&mut screen, column(vec![text_input("")]), &[]);
    screen.focus(&child(0));
    laid(&mut screen, column(vec![text_input("")]), &[]);
    let declared = keys(&screen, Role::Screen);

    screen.rebuilt(column(vec![text_input("memory")]));
    let appended = raised(typed(&mut screen, declared, "!"));

    assert_eq!(appended, vec![Raised::Text(Ev::Changed, String::from("memory!"))]);
}

#[test]
fn a_slider_dragged_outside_its_box_still_sets_its_value() {
    // Catches a slider that sets its value only while the pointer is over its box, so a drag that overshoots the end
    // stops short of it; and one that goes on following the pointer after the release.
    let mut screen = screen();
    let slider = Node::new(Segment::Place(0), Slider::new(0.0, 0.0, 10.0, 1.0, Ev::Slid), sized(100.0, 10.0), vec![]);
    laid(&mut screen, column(vec![slider]), &[]);
    let entry = hit(&screen, &child(0));

    let pressed = raised(press(&mut screen, entry, 50.0, 5.0));
    let overshot = raised(point(&mut screen, entry, PointerEvent::PointerMoved { x: 400.0, y: 80.0 }));
    release(&mut screen, entry, 400.0, 80.0, Over::Nothing);
    let after = raised(point(&mut screen, entry, PointerEvent::PointerMoved { x: 70.0, y: 5.0 }));

    assert_eq!(pressed, vec![Raised::Number(Ev::Slid, 5.0)]);
    assert_eq!(overshot, vec![Raised::Number(Ev::Slid, 10.0)]);
    assert_eq!(after, vec![]);
}

#[test]
fn a_slider_held_while_a_widget_before_it_starts_drawing_still_hears_its_moves_and_release() {
    // Catches what the capture sends read against the newest list, so a slider dragged while the button before it
    // starts drawing its hover tint, which renumbers every entry after it, hears its moves no more and never its
    // release, which reaches whatever now has its entry's number, and it is left dragging.
    let mut screen = screen();
    let tree = || {
        column(vec![
            Node::new(Segment::Place(0), Button::new(Ev::Click(1)), sized(40.0, 20.0), vec![]),
            Node::new(Segment::Place(1), Slider::new(0.0, 0.0, 10.0, 1.0, Ev::Slid), sized(100.0, 10.0), vec![]),
        ])
    };
    laid(&mut screen, tree(), &[]);
    let button = hit(&screen, &child(0));
    let slider = hit(&screen, &child(1));

    press(&mut screen, slider, 50.0, 25.0);
    point(&mut screen, button, PointerEvent::PointerEntered);
    laid(&mut screen, tree(), &[]);
    let renumbered = hit(&screen, &child(1));
    let moved = raised(point(&mut screen, slider, PointerEvent::PointerMoved { x: 100.0, y: 25.0 }));
    release(&mut screen, slider, 100.0, 25.0, Over::Nothing);
    let sliding = screen.table().state::<Slider<Ev>>(&child(1));

    assert_ne!(renumbered, slider, "the button's tint renumbers the slider's hit");
    assert_eq!(moved, vec![Raised::Number(Ev::Slid, 10.0)]);
    assert_eq!(sliding, Some(Sliding { hovered: false, dragging: false }));
}

/// A widget the pointer reaches that raises `Click(id)` for each move and release it hears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Heeds {
    id: u32,
}

impl Widget for Heeds {
    type State = Stateless;

    fn arrange(&self) -> Arrange {
        Arrange::Leaf
    }

    fn draw(&self, _: &Stateless, _: &Style, _: &Laid, _: &mut Draws<'_, '_>) {}

    fn pointer(&self, _: &Stateless) -> Pointer {
        Pointer::Blocks(Cursor::Default)
    }

    fn pointed(&self, _: &mut Stateless, pointing: Pointing, input: &mut Input<'_, '_>) {
        let follows = matches!(pointing, Pointing::Moved { .. } | Pointing::Up { .. });
        if follows {
            input.raise(Raised::Event(Ev::Click(self.id)));
        }
    }
}

#[test]
fn a_held_widget_whose_key_leaves_hears_nothing_more_and_neither_does_what_took_its_number() {
    // Catches what the capture sends to a widget gone from the tree handed to the widget that now has its entry's
    // number, so a row removed under the pointer passes its drag and its release to the row after it; and a hold
    // never let go, so the entry's number goes on naming the gone widget after every button is up.
    let mut screen = screen();
    let heeding =
        |id: u32| Node::new(Segment::Item(ItemKey::new(u64::from(id))), Heeds { id }, sized(40.0, 20.0), vec![]);
    laid(&mut screen, column(vec![heeding(1), heeding(2)]), &[]);
    let first = hit(&screen, &item(1));

    press(&mut screen, first, 10.0, 10.0);
    laid(&mut screen, column(vec![heeding(2)]), &[]);
    let taken = hit(&screen, &item(2));
    let moved = raised(point(&mut screen, first, PointerEvent::PointerMoved { x: 10.0, y: 15.0 }));
    let released = raised(release(&mut screen, first, 10.0, 10.0, over(taken, 10.0, 10.0)));
    let after = raised(point(&mut screen, taken, PointerEvent::PointerMoved { x: 10.0, y: 10.0 }));

    assert_eq!(taken, first, "the second widget now has the first's entry number");
    assert_eq!(moved, vec![]);
    assert_eq!(released, vec![]);
    assert_eq!(after, vec![Raised::Event(Ev::Click(2))], "with every button up, the number is read in the newest list");
}

#[test]
fn a_filled_widget_blocks_the_pointer_unless_it_is_within_one_the_pointer_reaches() {
    // Catches a panel's background the pointer passes through, so a click on the inventory's empty space also walks
    // the character there; and a button's filled label that takes the press from the button it labels.
    let mut screen = screen();
    let filled = Style { background: Colour::opaque(0.2, 0.2, 0.2), ..sized(40.0, 20.0) };
    let label = Node::new(Segment::Place(0), Column, filled.clone(), vec![]);
    let button = Node::new(Segment::Place(1), Button::new(Ev::Click(1)), sized(40.0, 20.0), vec![label]);
    let panel = Node::new(Segment::Place(0), Column, filled, vec![]);
    let (_, orders) = laid(&mut screen, column(vec![panel, button]), &[]);

    let hits: Vec<Pointer> = last_list(&orders)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|entry| match entry {
            Entry::Hit { pointer, .. } => Some(pointer),
            _ => None,
        })
        .collect();
    assert_eq!(hits, vec![Pointer::Blocks(Cursor::Default), Pointer::Blocks(Cursor::Hand)]);
}

/// A column of a button for each of `keys`, each keyed by its item, as `items` builds it.
fn keyed(keys: &[u64]) -> Tree {
    let rows = keys
        .iter()
        .map(|key| Node::new(Segment::Item(ItemKey::new(*key)), Button::new(Ev::Click(1)), sized(40.0, 10.0), vec![]))
        .collect();

    column(rows)
}

/// The path of the column's item keyed `key`.
fn item(key: u64) -> KeyPath {
    KeyPath::of(&[Segment::Place(0), Segment::Item(ItemKey::new(key))])
}

#[test]
fn an_item_s_state_and_focus_follow_its_key_as_the_list_reorders_and_go_when_it_leaves() {
    // Catches widget state kept by an item's place in its list, so when the list reorders the hover and the focus of
    // one item light up another; and an item's state or the focus kept after the item leaves the list, so an item
    // added later with its place comes up hovered and focused.
    let mut screen = screen();
    laid(&mut screen, keyed(&[1, 2, 3]), &[]);
    let entry = hit(&screen, &item(2));
    point(&mut screen, entry, PointerEvent::PointerEntered);
    press(&mut screen, entry, 5.0, 5.0);
    release(&mut screen, entry, 5.0, 5.0, over(entry, 5.0, 5.0));

    laid(&mut screen, keyed(&[3, 2, 1]), &[]);
    let followed = screen.table().state::<Button<Ev>>(&item(2));
    let other = screen.table().state::<Button<Ev>>(&item(3));
    let focus = screen.table().focus().clone();
    laid(&mut screen, keyed(&[3, 1]), &[]);

    assert_eq!(followed, Some(Press { hovered: true, pressed: false }));
    assert_eq!(other, Some(Press::default()));
    assert_eq!(focus, Focus::On(item(2)));
    assert_eq!(screen.table().state::<Button<Ev>>(&item(2)), None);
    assert_eq!(screen.table().focus(), &Focus::Nowhere);
}

/// Tabs titled "Items", "Skills", and "Quests", the one at `selected` open, over its panel, a column 30 pixels tall.
fn tabs(selected: u32) -> Tree {
    let panel = Node::new(Segment::Place(0), Column, sized(40.0, 30.0), vec![]);
    let tab = Node::new(Segment::Place(selected), Tab { title: String::from("Skills") }, Style::default(), vec![panel]);
    let style = Style { font: Font::Mono, ..Style::default() };
    let strip =
        Node::new(Segment::Place(0), Tabs::new(&["Items", "Skills", "Quests"], selected, Ev::Opened), style, vec![tab]);

    column(vec![strip])
}

#[test]
fn tabs_raise_change_with_the_tab_chosen_and_never_open_one_themselves() {
    // Catches tabs that open the tab chosen themselves, so the screen's open tab and the one shown part ways; a click
    // on a header that raises the wrong tab, or none; arrows that step from the last tab they raised rather than from
    // the open one the screen gave them; and a panel laid out under the headers rather than below them.
    let mut screen = screen_with_fonts();
    laid(&mut screen, tabs(0), &[]);
    let entry = hit(&screen, &child(0));
    let placed = |screen: &Screen, path: &KeyPath| {
        let found = screen.unkept.placed.iter().find(|(placed, _)| placed == path).map(|(_, laid)| *laid);
        let Some(laid) = found else {
            panic!("{path:?} is laid out");
        };
        laid
    };
    let strip = placed(&screen, &child(0));
    let shown = placed(&screen, &KeyPath::of(&[Segment::Place(0), Segment::Place(0), Segment::Place(0)])).rect;
    let below = strip.content.min.y;

    // In the monospace font at 16 pixels, "Items" and its insets take 60 pixels, so 80 is over "Skills".
    point(&mut screen, entry, PointerEvent::PointerMoved { x: 80.0, y: 5.0 });
    press(&mut screen, entry, 80.0, 5.0);
    let clicked = raised(release(&mut screen, entry, 80.0, 5.0, over(entry, 80.0, 5.0)));
    laid(&mut screen, tabs(0), &[]);
    let declared = keys(&screen, Role::Screen);
    let first = raised(key_down(&mut screen, declared, "ArrowRight", "ArrowRight"));
    let again = raised(key_down(&mut screen, declared, "ArrowRight", "ArrowRight"));
    let back = raised(key_down(&mut screen, declared, "ArrowLeft", "ArrowLeft"));

    assert!(below > strip.rect.min.y, "the headers keep room above the panel: {strip:?}");
    assert_eq!(shown.min.y, below, "the panel stands below the headers");
    assert_eq!(clicked, vec![Raised::Index(Ev::Opened, 1)]);
    assert_eq!(first, vec![Raised::Index(Ev::Opened, 1)]);
    assert_eq!(again, vec![Raised::Index(Ev::Opened, 1)], "the open tab is still the screen's first");
    assert_eq!(back, vec![Raised::Index(Ev::Opened, 2)], "left from the first goes round to the last");
}
