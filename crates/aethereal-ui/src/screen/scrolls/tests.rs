//! A screen's scrolls: the rows a list in a scroll builds as it is laid out and scrolled, its offset as the wheel, its
//! thumb, its keys, and Tab move it, and where what it holds stands and is cut, read from the trees its view builds,
//! its table, and where its layouts put each widget.

use super::super::input::tests::{Ev, hit, key_down, keys, laid, point, press, release, sized};
use super::super::keys::Role;
use super::super::orders::Order;
use super::super::tests::screen;
use super::super::{Outcome, Screen};
use super::MARGIN;
use crate::input::{Held, Key};
use crate::style::{Length, Style};
use crate::table::Focus;
use crate::tree::{ItemKey, KeyPath, Node, Segment, Shape, Tree};
use crate::widget::Laid;
use crate::widgets::{Button, Column, Scroll};
use aethereal_math::{Rect2, Vec2};
use aethereal_window::{Over, PointerEvent};

/// How tall each row of the tests' lists is, in pixels.
const ROW: f32 = 10.0;

/// The path of the scroll the tests' lists are in: the first child of the screen's root column.
fn list() -> KeyPath {
    KeyPath::of(&[Segment::Place(0), Segment::Place(0)])
}

/// The path of the list's row keyed `key`.
fn row(key: u64) -> KeyPath {
    list().child(Segment::Item(ItemKey::new(key)))
}

/// The view of a screen holding a scroll down, 100 pixels tall, over a list of a button for each of `keys`, a row 10
/// pixels tall each, built as the markup's `items` builds it in a scroll: only the rows the screen says it shows.
fn listed(screen: &Screen, keys: &[u64]) -> Tree {
    listed_in(screen, keys, Scroll::down())
}

/// The view of [`listed`], its list in `scroll`.
fn listed_in(screen: &Screen, keys: &[u64], scroll: Scroll) -> Tree {
    let mut rows = screen.rows(&list(), keys.len());
    for (at, key) in keys.iter().enumerate() {
        let key = ItemKey::new(*key);
        if rows.builds(at, key) {
            let style = Style { height: Length::Pixels(ROW), ..Style::default() };
            rows.push(at, Node::new(Segment::Item(key), Button::new(Ev::Click(1)), style, vec![]));
        }
    }
    let (lines, built) = rows.finish();
    let style = Style { height: Length::Pixels(100.0), ..Style::default() };
    let scroll = Node::new(Segment::Place(0), scroll.list(lines), style, built);

    Tree::new(Node::new(Segment::Place(0), Column, Style::default(), vec![scroll]))
}

/// The keys of the rows `tree` built, in order.
fn built(tree: &Tree) -> Vec<ItemKey> {
    let mut shape = Shape::default();
    let Ok(flattened) = tree.flat(&mut shape) else {
        panic!("the list's tree is walked");
    };

    flattened
        .nodes
        .iter()
        .filter_map(|flat| match flat.path.segments() {
            [_, _, Segment::Item(key)] => Some(*key),
            _ => None,
        })
        .collect()
}

/// The keys `0..count`.
fn counted(count: u64) -> Vec<u64> {
    (0..count).collect()
}

/// The item keys of `keys`.
fn items(keys: impl IntoIterator<Item = u64>) -> Vec<ItemKey> {
    keys.into_iter().map(ItemKey::new).collect()
}

/// Builds the screen's view of a list of `keys` and lays it out, every draw made: what the `Prepare` did, the tree it
/// was given, and every order sent.
fn frame(screen: &mut Screen, keys: &[u64]) -> (Outcome, Vec<ItemKey>, Vec<Order>) {
    let tree = listed(screen, keys);
    let rows = built(&tree);
    let (outcome, orders) = laid(screen, tree, &[]);

    (outcome, rows, orders)
}

/// Where the screen's last layout put the widget at `path`.
fn placed(screen: &Screen, path: &KeyPath) -> Laid {
    let found = screen.unkept.placed.iter().find(|(placed, _)| placed == path).map(|(_, laid)| *laid);
    let Some(laid) = found else {
        panic!("{path:?} is laid out");
    };

    laid
}

/// How far the scroll at `path` has slid, as its table keeps it.
fn offset(screen: &Screen, path: &KeyPath) -> f32 {
    screen.table().state::<Scroll>(path).unwrap_or_default().offset
}

/// The wheel turned `down` pixels through the entry `entry`.
fn wheel(screen: &mut Screen, entry: u32, down: f32) {
    point(screen, entry, PointerEvent::Wheel { x: 50.0, y: 50.0, delta_x: 0.0, delta_y: down });
}

#[test]
fn a_list_in_a_scroll_builds_the_rows_it_shows_and_those_the_wheel_brings_into_view() {
    // Catches a scroll that builds every item, which is the cost a list in a scroll exists to save; one that builds
    // its window from the offset of the frame before, so a turn of the wheel shows a gap; rows placed one after another
    // from the first built rather than at their items' lines, so the hundredth item shows at the top; content measured
    // from the rows built rather than the list's length, so the thumb and the end are wrong; and rows not cut to the
    // scroll's box.
    let mut screen = screen();
    let list_keys = counted(1000);
    let (_, unlaid, _) = frame(&mut screen, &list_keys);
    let (_, first, _) = frame(&mut screen, &list_keys);

    let entry = hit(&screen, &list());
    wheel(&mut screen, entry, 500.0);
    let (_, scrolled, _) = frame(&mut screen, &list_keys);

    let shown = 10;
    assert_eq!(unlaid, items([0]), "a list never laid out builds its first row, to measure");
    assert_eq!(first, items(0..u64::from(shown + MARGIN)));
    assert_eq!(scrolled, items(50 - u64::from(MARGIN)..60 + u64::from(MARGIN)));
    let fiftieth = placed(&screen, &row(50));
    assert_eq!(fiftieth.rect.min, Vec2::new(0.0, 0.0), "the fiftieth row stands at the top, 500 pixels down");
    assert_eq!(fiftieth.clip, Rect2 { min: Vec2::ZERO, max: Vec2::new(192.0, 100.0) }, "cut to the scroll's box");
    assert_eq!(placed(&screen, &list()).extent.y, 10_000.0, "the content is every row's");
}

#[test]
fn an_unchanged_list_at_an_unchanged_offset_lays_nothing_out() {
    // Catches a list whose window is worked out from something that moves every frame, so its tree never compares
    // equal and the screen lays out and sends every frame.
    let mut screen = screen();
    let list_keys = counted(1000);
    frame(&mut screen, &list_keys);
    frame(&mut screen, &list_keys);

    let (outcome, _, orders) = frame(&mut screen, &list_keys);

    assert_eq!(outcome, Outcome::Unchanged);
    assert!(orders.is_empty(), "{orders:?}");
}

#[test]
fn the_thumb_dragged_sets_the_offset_by_where_it_is_held_wherever_the_pointer_goes() {
    // Catches a thumb that jumps its start to the pointer, so a grab in its middle jerks the list; a drag that stops
    // at the scroll's edge rather than following the press past it, so the end is never reached; and the thumb's
    // travel taken as the track's whole length, so its end overshoots the list's.
    let mut screen = screen();
    let list_keys = counted(1000);
    frame(&mut screen, &list_keys);
    frame(&mut screen, &list_keys);
    let entry = hit(&screen, &list());

    // The bar is the 8 pixels at the right; its thumb, 16 pixels at least, has 84 pixels to travel through 9900.
    press(&mut screen, entry, 196.0, 8.0);
    point(&mut screen, entry, PointerEvent::PointerMoved { x: 196.0, y: 50.0 });
    let halfway = offset(&screen, &list());
    point(&mut screen, entry, PointerEvent::PointerMoved { x: 500.0, y: 400.0 });
    let past = offset(&screen, &list());
    release(&mut screen, entry, 500.0, 400.0, Over::Nothing);
    point(&mut screen, entry, PointerEvent::PointerMoved { x: 196.0, y: 20.0 });
    let after = offset(&screen, &list());

    assert_eq!(halfway, 42.0 / 84.0 * 9900.0);
    assert_eq!(past, 9900.0);
    assert_eq!(after, 9900.0, "the release ends the drag");
}

#[test]
fn the_offset_is_clamped_when_what_the_scroll_holds_shrinks() {
    // Catches an offset left past the end when the list shrinks, which shows an empty box until the wheel is turned
    // back the whole way; and one clamped only as it is drawn, not kept, so the list jumps back when it grows again.
    let mut screen = screen();
    frame(&mut screen, &counted(1000));
    frame(&mut screen, &counted(1000));
    let entry = hit(&screen, &list());
    wheel(&mut screen, entry, 1_000_000.0);
    let end = offset(&screen, &list());

    let (_, shrunk, _) = frame(&mut screen, &counted(20));

    assert_eq!(end, 9900.0);
    assert_eq!(offset(&screen, &list()), 100.0);
    assert_eq!(shrunk, items(8..20), "the last rows show at once");
    assert_eq!(placed(&screen, &row(19)).rect.max.y, 100.0);
}

#[test]
fn tab_into_a_row_past_the_box_slides_the_scroll_to_show_it() {
    // Catches Tab that moves focus to a row out of view and leaves it there, so the focused row cannot be seen; a
    // scroll slid so the row shows at the top rather than just inside its edge; and a list that builds no row past
    // what it shows, so Tab leaves the list rather than reaching the next row.
    let mut screen = screen();
    let list_keys = counted(1000);
    frame(&mut screen, &list_keys);
    frame(&mut screen, &list_keys);
    let entry = hit(&screen, &row(9));
    press(&mut screen, entry, 5.0, 95.0);
    release(&mut screen, entry, 5.0, 95.0, Over::Nothing);
    frame(&mut screen, &list_keys);

    let declared = keys(&screen, Role::Screen);
    key_down(&mut screen, declared, "Tab", "Tab");
    frame(&mut screen, &list_keys);

    assert_eq!(screen.table().focus(), &Focus::On(row(10)));
    assert_eq!(offset(&screen, &list()), 10.0);
    assert_eq!(placed(&screen, &row(10)).rect.max.y, 100.0);
}

#[test]
fn page_down_around_a_focused_row_slides_its_scroll_and_the_row_keeps_focus_out_of_view() {
    // Catches Page Down not declared or not handed on from a focused row to the scroll around it, so it does nothing
    // until the scroll itself is clicked; and a focused row dropped from the list once it scrolls out of the window,
    // which takes its focus, so the next Page Down goes nowhere.
    let mut screen = screen();
    let list_keys = counted(1000);
    frame(&mut screen, &list_keys);
    frame(&mut screen, &list_keys);
    let entry = hit(&screen, &row(2));
    press(&mut screen, entry, 5.0, 25.0);
    release(&mut screen, entry, 5.0, 25.0, Over::Nothing);
    frame(&mut screen, &list_keys);

    let declared = keys(&screen, Role::Screen);
    key_down(&mut screen, declared, "PageDown", "PageDown");
    let (_, paged, _) = frame(&mut screen, &list_keys);
    let declared = keys(&screen, Role::Screen);
    key_down(&mut screen, declared, "PageDown", "PageDown");
    let (_, twice, _) = frame(&mut screen, &list_keys);

    assert_eq!(offset(&screen, &list()), 200.0);
    assert_eq!(screen.table().focus(), &Focus::On(row(2)));
    assert_eq!(paged.first(), Some(&ItemKey::new(2)), "the focused row is built ahead of the window: {paged:?}");
    assert_eq!(twice.first(), Some(&ItemKey::new(2)), "{twice:?}");
}

/// A screen's view of an outer scroll 100 pixels tall over an inner scroll 50 pixels tall, holding ten rows of 10
/// pixels, and a column 200 pixels tall below it.
fn nested() -> Tree {
    let tall = |height: f32| Style { height: Length::Pixels(height), ..Style::default() };
    let rows = (0..10).map(|at| Node::new(Segment::Place(at), Column, tall(ROW), vec![])).collect();
    let inner = Node::new(Segment::Place(0), Scroll::down(), tall(50.0), rows);
    let below = Node::new(Segment::Place(1), Column, sized(40.0, 200.0), vec![]);
    let outer = Node::new(Segment::Place(0), Scroll::down(), tall(100.0), vec![inner, below]);

    Tree::new(Node::new(Segment::Place(0), Column, Style::default(), vec![outer]))
}

#[test]
fn the_wheel_slides_the_innermost_scroll_that_can_move_and_passes_outward_at_its_end() {
    // Catches the wheel taken by the outer scroll while the inner one can still move, so a list in a panel cannot be
    // scrolled; one held by the inner scroll at its end, so the panel around it never moves; and the inner scroll
    // left behind on the way back, so turning the wheel up moves the panel before the list.
    let mut screen = screen();
    laid(&mut screen, nested(), &[]);
    let outer = list();
    let inner = outer.child(Segment::Place(0));
    let entry = hit(&screen, &inner);

    let mut seen = Vec::new();
    for down in [30.0, 30.0, 30.0, -10.0] {
        wheel(&mut screen, entry, down);
        seen.push((offset(&screen, &inner), offset(&screen, &outer)));
    }

    assert_eq!(seen, vec![(30.0, 0.0), (50.0, 0.0), (50.0, 30.0), (40.0, 30.0)]);
}

#[test]
fn a_scroll_kept_to_its_end_shows_what_is_added_past_it_until_it_is_slid_back() {
    // Catches a console's scrollback that stays where it stood as lines are printed beneath it, so the newest never
    // shows; one that builds its window from where it stood, so the lines printed show a frame late as a gap; one that
    // goes back to its end after the wheel slid it back, so a person reading what scrolled by is pulled away by each
    // line printed; and one that never keeps to its end again once slid back down to it.
    let mut screen = screen();
    let at_end = |screen: &mut Screen, count: u64| {
        let tree = listed_in(screen, &counted(count), Scroll::down().keeping_to_end());
        let rows = built(&tree);
        laid(screen, tree, &[]);
        (offset(screen, &list()), rows)
    };
    at_end(&mut screen, 20);
    let (first, _) = at_end(&mut screen, 20);

    let (grown, rows) = at_end(&mut screen, 40);
    let entry = hit(&screen, &list());
    wheel(&mut screen, entry, -50.0);
    let (slid_back, _) = at_end(&mut screen, 41);
    wheel(&mut screen, entry, 1000.0);
    at_end(&mut screen, 41);
    let (kept_again, _) = at_end(&mut screen, 42);

    assert_eq!(first, 100.0, "a scroll never laid out starts at its end");
    assert_eq!(grown, 300.0);
    assert!(rows.contains(&ItemKey::new(39)), "the newest row is built as it is added: {rows:?}");
    assert_eq!(slid_back, 250.0);
    assert_eq!(kept_again, 320.0);
}

#[test]
fn a_key_handed_to_a_scroll_by_its_path_slides_it_though_it_has_no_focus() {
    // Catches a key handed by path to the focused widget instead, or to nothing, so a console's Page Down moves its
    // line's caret rather than its scrollback.
    let mut screen = screen();
    let list_keys = counted(1000);
    frame(&mut screen, &list_keys);
    frame(&mut screen, &list_keys);

    let page =
        Key { code: String::from("PageDown"), key: String::from("PageDown"), repeat: false, held: Held::default() };
    screen.press_key(&list(), &page);

    assert_eq!(screen.table().focus(), &Focus::Nowhere);
    assert_eq!(offset(&screen, &list()), 100.0);
}
