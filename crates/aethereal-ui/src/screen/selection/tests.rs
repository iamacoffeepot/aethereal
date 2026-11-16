//! A screen's selection across the rows of a scroll: what a press and a drag over several rows select, what each row is
//! drawn holding, what a copy and Select All write, rows a windowed scroll did not build among them, a drag past the
//! view sliding it on, and a press on a link still following it, read from the screen's table, the parts its layout
//! draws, and what its turns of input ask of the clipboard.

use super::super::input::tests::{
    Ev, hit, key_down, keys, laid, over, point, press, raised, release, screen_with_fonts,
};
use super::super::keys::Role;
use super::super::tests::mono;
use super::super::{Clipping, Heard, Last, Screen};
use super::parts;
use crate::fonts::{Fonts, Texts};
use crate::input::{Raised, Raises};
use crate::style::{Length, Style};
use crate::table::{Mark, Selection};
use crate::tree::{ItemKey, KeyPath, Node, Segment, Shape, Tree};
use crate::widget::{Selected, Selects};
use crate::widgets::{Column, Flow, Link, Rich, Scroll, Selectable};
use aethereal_math::Vec2;
use aethereal_render::text::{Colour, Font, Run, Wrap};
use aethereal_window::PointerEvent;

/// How tall each row is, in pixels.
const ROW: f32 = 20.0;

/// How tall the scroll is, in pixels: four rows of the screen's hundred.
const VIEW: f32 = 80.0;

/// The colour of the scroll's style, which its selection is drawn in.
const INK: Colour = Colour { red: 0.2, green: 0.4, blue: 0.8, alpha: 1.0 };

/// The path of the scroll, the first child of the screen's root column.
fn list() -> KeyPath {
    KeyPath::of(&[Segment::Place(0), Segment::Place(0)])
}

/// The path of the row at `place`, keyed by its place.
fn row(place: u64) -> KeyPath {
    list().child(Segment::Item(ItemKey::new(place)))
}

/// A style in the monospace font at its default size, `width` by `height`.
fn mono_box(width: f32, height: f32) -> Style {
    Style { width: Length::Pixels(width), height: Length::Pixels(height), font: Font::Mono, ..Style::default() }
}

/// The view of a screen holding a scroll down, [`VIEW`] tall, of a `Selectable` row for each of `lines`, keyed by its
/// place, built as markup's `items` builds it in a scroll: only the rows the screen says to build.
fn listed(screen: &Screen, lines: &[String]) -> Tree {
    let scroll = Scroll::down();
    let mut rows = screen.rows(&list(), &scroll, lines.len(), |at| ItemKey::new(u64::try_from(at).unwrap_or(0)));
    for at in rows.places() {
        let key = ItemKey::new(u64::try_from(at).unwrap_or(0));
        let line = Selectable { runs: vec![Run::new(&lines[at], Colour::WHITE)] };
        rows.push(at, Node::new(Segment::Item(key), line, mono_box(200.0, ROW), vec![]));
    }
    let (lines, built) = rows.finish();
    let style = Style { height: Length::Pixels(VIEW), colour: INK, ..Style::default() };
    let scroll = Node::new(Segment::Place(0), scroll.list(lines), style, built);

    Tree::new(Node::new(Segment::Place(0), Column, Style::default(), vec![scroll]))
}

/// Lays the view of `lines` out twice, as the first frames do: the first measures the row the scroll stands at, and the
/// second builds the window its layout bounds.
fn shown(screen: &mut Screen, lines: &[String]) {
    for _ in 0..2 {
        let tree = listed(screen, lines);
        laid(screen, tree, &[]);
    }
}

/// `count` lines, `line 00` on.
fn numbered(count: usize) -> Vec<String> {
    (0..count).map(|at| format!("line {at:02}")).collect()
}

/// How wide one character of the monospace font is at the default size.
fn cell() -> f32 {
    let mut fonts = Fonts::default();
    fonts.read(Font::Mono, mono(), Vec::new());
    let mut texts = Texts::default();
    let mut measuring = fonts.measuring(&mut texts, 1.0);
    let Ok(carets) = measuring.carets(Font::Mono, Style::default().font_size, "m", Wrap::None) else {
        panic!("the monospace font's metrics are read");
    };

    carets.lines[0].stops[1].x
}

/// The place, in the screen's pixels, of the boundary `offset` characters into the row at `place`, halfway down it, a
/// little past the boundary so it is the nearest.
fn spot(screen: &Screen, place: u64, offset: f32) -> Vec2 {
    let Some(laid) = screen.laid(&row(place)) else {
        panic!("the row at {place} is laid out");
    };

    laid.content.min + Vec2::new(cell().mul_add(offset, cell() * 0.2), ROW / 2.0)
}

/// The number of the hit over the widget at `path`, and how far it is drawn from the screen's own coordinates, which
/// the window's places through it are told less.
fn entry(screen: &Screen, path: &KeyPath) -> (u32, Vec2) {
    let entry = hit(screen, path);

    (entry, screen.moved[usize::try_from(entry).unwrap_or(0)])
}

/// Presses on the row at `from` at its spot `from_offset` characters in, drags to `to`, a place in the screen's pixels,
/// and releases there: a frame laid out after each, as the screen's `Prepare`s lay it out.
fn dragged(screen: &mut Screen, lines: &[String], (from, from_offset): (u64, f32), to: Vec2) {
    let (number, moved) = entry(screen, &row(from));
    let start = spot(screen, from, from_offset) - moved;
    press(screen, number, start.x, start.y);
    let tree = listed(screen, lines);
    laid(screen, tree, &[]);
    let end = to - moved;
    point(screen, number, PointerEvent::PointerMoved { x: end.x, y: end.y });
    let tree = listed(screen, lines);
    laid(screen, tree, &[]);
    release(screen, number, end.x, end.y, over(number, end.x, end.y));
    let tree = listed(screen, lines);
    laid(screen, tree, &[]);
}

/// Holds Control and presses the key at `code`, meaning `key`, through the keys the screen declares.
fn chorded(screen: &mut Screen, code: &str, key: &str) -> Heard {
    let modifiers = keys(screen, Role::Modifiers);
    key_down(screen, modifiers, "ControlLeft", "Control");
    let declared = keys(screen, Role::Screen);

    key_down(screen, declared, code, key)
}

/// Copies the screen's selection with Control and C, its view of `lines` built again to copy it as `screen!` builds
/// it: what the turn asked of the clipboard.
fn copied(screen: &mut Screen, lines: &[String]) -> Vec<Clipping> {
    let mut heard = chorded(screen, "KeyC", "c");
    assert!(heard.copies_selection(), "Control and C asks for the selection to be copied");
    screen.building_selection();
    let tree = listed(screen, lines);
    screen.copy_selection(&tree, &mut heard);

    heard.clips
}

/// The part of the text at `path` the screen's selection holds, as its last tree is drawn.
fn part(screen: &Screen, path: &KeyPath) -> Selected {
    let Last::Built(tree) = &screen.unkept.last else {
        panic!("the screen built a tree");
    };
    let mut shape = Shape::default();
    let Ok(flattened) = tree.flat(&mut shape) else {
        panic!("the screen's tree is walked");
    };
    let parts = parts(&flattened.nodes, screen.table().selection());
    let Some(at) = flattened.nodes.iter().position(|node| node.path == path) else {
        return Selected::Nothing;
    };

    parts[at]
}

#[test]
fn a_drag_from_one_row_to_another_two_down_copies_the_first_ones_tail_the_middle_and_the_last_ones_head() {
    // Catches a selection that stays within the row it began in, as a widget's own selection did; one whose rows are
    // joined without newlines, or in another order; and a copy of the rows whole rather than from the press's offset
    // in the first to the caret's in the last.
    let mut screen = screen_with_fonts();
    let lines = vec![String::from("alpha one"), String::from("bravo two"), String::from("charlie three")];
    shown(&mut screen, &lines);
    let to = spot(&screen, 2, 7.0);

    dragged(&mut screen, &lines, (0, 6.0), to);

    assert_eq!(copied(&mut screen, &lines), vec![Clipping::Copy(String::from("one\nbravo two\ncharlie"))]);
}

#[test]
fn each_row_a_selection_runs_through_is_drawn_holding_its_part_in_the_scrolls_colour() {
    // Catches the rows between the ends drawn unselected, as only the texts the ends stand in would be told; the first
    // row drawn from its start, or the last to its end; a row past the caret drawn selected; and the selection drawn in
    // a colour of its own rather than its scroll's.
    let mut screen = screen_with_fonts();
    let lines = numbered(4);
    shown(&mut screen, &lines);
    let to = spot(&screen, 2, 5.0);

    dragged(&mut screen, &lines, (0, 2.0), to);

    let from = |start: u32, end: u32| Selected::Part { start, end, colour: INK };
    assert_eq!(part(&screen, &row(0)), from(2, u32::MAX));
    assert_eq!(part(&screen, &row(1)), from(0, u32::MAX));
    assert_eq!(part(&screen, &row(2)), from(0, 5));
    assert_eq!(part(&screen, &row(3)), Selected::Nothing);
}

#[test]
fn select_all_copies_every_row_of_the_list_though_the_window_built_only_a_few() {
    // Catches a copy that walks only the rows the window built, so the rows scrolled out of view are lost; and Select
    // All scoped to the focused row rather than the scroll it stands in.
    let mut screen = screen_with_fonts();
    let lines = numbered(40);
    shown(&mut screen, &lines);
    let (number, moved) = entry(&screen, &row(0));
    let start = spot(&screen, 0, 1.0) - moved;
    press(&mut screen, number, start.x, start.y);
    release(&mut screen, number, start.x, start.y, over(number, start.x, start.y));
    let tree = listed(&screen, &lines);
    laid(&mut screen, tree, &[]);
    let unbuilt = screen.laid(&row(30)).is_none();

    chorded(&mut screen, "KeyA", "a");

    assert!(unbuilt, "the window does not build the row at 30");
    assert_eq!(copied(&mut screen, &lines), vec![Clipping::Copy(lines.join("\n"))]);
}

#[test]
fn a_drag_held_below_the_view_slides_the_scroll_on_and_copies_the_rows_it_passed_that_are_no_longer_built() {
    // Catches a drag that stops at the view's edge, so a selection can never run past what shows; a caret that stays
    // where it was as the scroll slides under a still pointer; and a copy that leaves out the rows between the anchor's
    // row and the window, which the scroll slid past and no longer builds.
    let mut screen = screen_with_fonts();
    let lines = numbered(40);
    shown(&mut screen, &lines);
    let below = spot(&screen, 0, 4.0) + Vec2::new(0.0, VIEW + ROW);

    let (number, moved) = entry(&screen, &row(0));
    let start = spot(&screen, 0, 5.0) - moved;
    press(&mut screen, number, start.x, start.y);
    let end = below - moved;
    point(&mut screen, number, PointerEvent::PointerMoved { x: end.x, y: end.y });
    for _ in 0..8 {
        let tree = listed(&screen, &lines);
        laid(&mut screen, tree, &[]);
    }
    release(&mut screen, number, end.x, end.y, over(number, end.x, end.y));
    let tree = listed(&screen, &lines);
    laid(&mut screen, tree, &[]);

    let Selection::Made(made) = screen.table().selection().clone() else {
        panic!("the drag made a selection");
    };
    let Mark::At { path, offset } = made.caret else {
        panic!("the caret stands in a row: {made:?}");
    };
    let Some(reached) = (0..lines.len()).find(|place| row(u64::try_from(*place).unwrap_or(0)) == path) else {
        panic!("the caret stands in a row of the list: {path:?}");
    };
    assert!(reached > 10, "the caret followed the slide past the first view, to the row at {reached}");
    assert!(screen.laid(&row(1)).is_none(), "the row at 1 is no longer built");
    let mut expected = vec![String::from("00")];
    expected.extend(lines[1..reached].iter().cloned());
    expected.push(String::from(&lines[reached][..usize::try_from(offset).unwrap_or(0)]));
    assert_eq!(copied(&mut screen, &lines), vec![Clipping::Copy(expected.join("\n"))]);
}

/// The path of the transcript's one reply, a `Rich` with a link, within its row.
fn reply() -> KeyPath {
    list().child(Segment::Place(0)).child(Segment::Place(0))
}

/// A scroll that selects every text, holding a row of one reply: `see here now`, `here` a link to `there`.
fn transcript() -> Tree {
    let runs = vec![Run::new("see ", Colour::WHITE), Run::new("here", INK), Run::new(" now", Colour::WHITE)];
    let link = Link { run: 1, target: String::from("there") };
    let rich = Rich { runs, links: vec![link], flow: Flow::Wraps, link: Raises::Event(Ev::Click(9)) };
    let text = Node::new(Segment::Place(0), rich, mono_box(200.0, ROW), vec![]);
    let row = Node::new(Segment::Place(0), Column, Style::default(), vec![text]);
    let scroll = Scroll { selects: Selects::Texts, ..Scroll::down() };
    let style = Style { height: Length::Pixels(VIEW), colour: INK, ..Style::default() };
    let scroll = Node::new(Segment::Place(0), scroll, style, vec![row]);

    Tree::new(Node::new(Segment::Place(0), Column, Style::default(), vec![scroll]))
}

#[test]
fn a_press_on_a_link_in_a_scroll_that_selects_every_text_follows_it_and_one_beside_it_selects_there() {
    // Catches a press on a link that starts a selection, or swallows the click, so a reply's links stop working once
    // its text is selectable; and a press on a reply's plain text, which only its scroll's panel hears, that selects
    // nothing.
    let mut screen = screen_with_fonts();
    laid(&mut screen, transcript(), &[]);
    let Some(text) = screen.laid(&reply()) else {
        panic!("the reply is laid out");
    };
    let on_link = text.content.min + Vec2::new(cell() * 5.5, ROW / 2.0);
    let (link_entry, moved) = entry(&screen, &reply());
    let at = on_link - moved;
    point(&mut screen, link_entry, PointerEvent::PointerMoved { x: at.x, y: at.y });
    laid(&mut screen, transcript(), &[]);
    let (link_entry, moved) = entry(&screen, &reply());
    let at = on_link - moved;

    press(&mut screen, link_entry, at.x, at.y);
    let followed = raised(release(&mut screen, link_entry, at.x, at.y, over(link_entry, at.x, at.y)));
    let after_link = screen.table().selection().clone();
    let on_now = text.content.min + Vec2::new(cell().mul_add(9.0, cell() * 0.2), ROW / 2.0);
    let (panel, moved) = entry(&screen, &list());
    let at = on_now - moved;
    press(&mut screen, panel, at.x, at.y);

    assert_eq!(followed, vec![Raised::Text(Ev::Click(9), String::from("there"))]);
    assert_eq!(after_link, Selection::Nothing);
    let Selection::Made(made) = screen.table().selection() else {
        panic!("a press on the reply's text selects there");
    };
    assert_eq!(made.anchor, Mark::At { path: reply(), offset: 9 });
    assert_eq!(made.scope, list());
}
