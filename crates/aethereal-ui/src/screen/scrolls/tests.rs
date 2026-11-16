//! A screen's scrolls: the items a list in a scroll builds as it is laid out and scrolled, where it stands as the
//! wheel, its thumb, its keys, a jump, and Tab move it and as its list changes, and where what it holds stands and is
//! cut, read from the trees its view builds, its table, and where its layouts put each widget.

use super::super::input::tests::{Ev, hit, key_down, keys, laid, point, press, release, sized};
use super::super::keys::Role;
use super::super::orders::Order;
use super::super::tests::{mono, screen};
use super::super::{Outcome, Screen, Viewport};
use super::MARGIN;
use super::window::{Window, Windowing};
use crate::input::{Held, Key, Raises};
use crate::markdown::{Document, Images, Inks, Markdown, Prose};
use crate::pixels::Pixels;
use crate::style::{Border, Distribute, Edges, Gap, Length, Space, Style};
use crate::table::Focus;
use crate::tree::{Element, ItemKey, KeyPath, Node, Segment, Shape, Tree};
use crate::widget::{Anchor, Jump, Keeps, Laid, Stand};
use crate::widgets::{Button, Column, Picture, Scroll};
use aethereal_actor::Level;
use aethereal_math::{Rect2, Vec2};
use aethereal_render::text::{Colour, Font};
use aethereal_window::{Over, PointerEvent};

/// How tall each row of the tests' lists is, in pixels, where every row is as tall.
const ROW: f32 = 10.0;

/// How tall the tests' scrolls are, in pixels.
const VIEW: f32 = 100.0;

/// The path of the scroll the tests' lists are in: the first child of the screen's root column.
fn list() -> KeyPath {
    KeyPath::of(&[Segment::Place(0), Segment::Place(0)])
}

/// The path of the list's row keyed `key`.
fn row(key: u64) -> KeyPath {
    list().child(Segment::Item(ItemKey::new(key)))
}

/// A scroll's style: [`VIEW`] pixels tall.
fn tall() -> Style {
    Style { height: Length::Pixels(VIEW), ..Style::default() }
}

/// The view of a screen holding a scroll down, [`VIEW`] pixels tall, over a list of a button for each of `keys`, each
/// [`ROW`] pixels tall, built as the markup's `items` builds it in a scroll: only the rows the screen says can show.
fn listed(screen: &Screen, keys: &[u64]) -> Tree {
    listed_in(screen, keys, (Scroll::down(), tall()), |_| ROW)
}

/// The view of [`listed`], its list in `scroll` styled `style`, the row keyed `key` `length(key)` pixels tall and
/// filled, so it draws.
fn listed_in(screen: &Screen, keys: &[u64], (scroll, style): (Scroll, Style), length: impl Fn(u64) -> f32) -> Tree {
    let mut rows = screen.rows(&list(), &scroll, keys.len(), |at| ItemKey::new(keys[at]));
    for at in rows.places() {
        let key = keys[at];
        let filled = Style {
            height: Length::Pixels(length(key)),
            background: Colour { red: 0.5, green: 0.5, blue: 0.5, alpha: 1.0 },
            ..Style::default()
        };
        rows.push(at, Node::new(Segment::Item(ItemKey::new(key)), Button::new(Ev::Click(1)), filled, vec![]));
    }
    let (lines, built) = rows.finish();
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

/// Builds `tree`, the screen's view, and lays it out, every draw made: what the `Prepare` did, the rows it built, and
/// every order sent.
fn shown(screen: &mut Screen, tree: Tree) -> (Outcome, Vec<ItemKey>, Vec<Order>) {
    let rows = built(&tree);
    let (outcome, orders) = laid(screen, tree, &[]);

    (outcome, rows, orders)
}

/// Builds the screen's view of a list of `keys`, every row [`ROW`] tall, and lays it out.
fn frame(screen: &mut Screen, keys: &[u64]) -> (Outcome, Vec<ItemKey>, Vec<Order>) {
    let tree = listed(screen, keys);

    shown(screen, tree)
}

/// Where the screen's last layout put the widget at `path`.
fn placed(screen: &Screen, path: &KeyPath) -> Laid {
    let Some(laid) = screen.laid(path) else {
        panic!("{path:?} is laid out");
    };

    laid
}

/// Where the top of the list's row keyed `key` stands on the screen.
fn top(screen: &Screen, key: u64) -> f32 {
    placed(screen, &row(key)).rect.min.y
}

/// Where the scroll at `path` stands, as its table keeps it.
fn stand(screen: &Screen, path: &KeyPath) -> Stand {
    screen.table().state::<Scroll>(path).unwrap_or_default().stand
}

/// How far the scroll at `path` slides what it built, in pixels, standing where its table keeps it, as its last layout
/// found it.
fn offset(screen: &Screen, path: &KeyPath) -> f32 {
    let Some(window) = screen.unkept.windows.iter().find(|window| window.path() == path) else {
        panic!("{path:?} was laid out as a scroll");
    };
    let at = window.offset_of(stand(screen, path), Keeps::Offset);

    window.stand_of(at, Keeps::Offset).1
}

/// The wheel turned `down` pixels through the entry `entry`.
fn wheel(screen: &mut Screen, entry: u32, down: f32) {
    point(screen, entry, PointerEvent::Wheel { x: 50.0, y: 50.0, delta_x: 0.0, delta_y: down });
}

/// Whether the rows the screen's last layout put at `keys` fill the scroll's view from its top to its bottom, one
/// after another: a gap, or a view not filled to its end, shows nothing where a row should be.
fn fills(screen: &Screen, keys: &[ItemKey]) -> bool {
    let mut reached = 0.0;
    for key in keys {
        let rect = placed(screen, &list().child(Segment::Item(*key))).rect;
        let gap = rect.min.y > reached + 0.01;
        let in_view = rect.min.y < VIEW;
        let shows_gap = gap && in_view;
        if shows_gap {
            return false;
        }
        reached = rect.max.y.max(reached);
    }

    reached >= VIEW - 0.01
}

#[test]
fn a_list_in_a_scroll_builds_at_most_the_rows_that_can_show_whatever_their_lengths() {
    // Catches a window bounded by the rows' count rather than by the least row, so a mix of lengths builds the whole
    // list or too few to fill the view; a window that leaves a gap where a long row, longer than the view, stands; and
    // a bound read from the row built first, so a short row later on leaves the view part empty.
    let mut screen = screen();
    let lengths = [10.0, 37.0, 250.0, 13.0, 10.0, 64.0, 22.0];
    let length = |key: u64| lengths[usize::try_from(key).unwrap_or(0) % lengths.len()];
    let list_keys = counted(200);
    let scroll = || (Scroll::down(), tall());
    let tree = listed_in(&screen, &list_keys, scroll(), length);
    shown(&mut screen, tree);

    // The least row is 10 pixels, so a view of 100 shows at most eleven rows, and the window two more each way.
    let most = usize::try_from(11 + 2 * MARGIN).unwrap_or(usize::MAX);
    let entry = hit(&screen, &list());
    for turn in 0..80 {
        let tree = listed_in(&screen, &list_keys, scroll(), length);
        let (_, rows, _) = shown(&mut screen, tree);
        assert!(rows.len() <= most, "turn {turn}: {} rows built: {rows:?}", rows.len());
        assert!(fills(&screen, &rows), "turn {turn}: the view is filled: {rows:?}");
        wheel(&mut screen, entry, 30.0);
    }
}

#[test]
fn a_rows_padding_border_and_margins_count_in_its_least_length_and_no_scroll_shows_a_gap() {
    // Catches a least length that leaves out a row's padding, border, or margins, or a wrapper's padding within it, so
    // the window builds more rows than can show; and one that counts more than every row is sure to take, a side twice
    // or a margin that is not there, so a run of the shortest rows leaves the view part empty at some scroll.
    let mut screen = screen();
    let spaced = |top: f32, bottom: f32| Edges {
        top: Space::Pixels(top),
        bottom: Space::Pixels(bottom),
        ..Edges::all(Space::Pixels(5.0))
    };
    let lengths = [2.0, 2.0, 2.0, 2.0, 2.0, 2.0, 2.0, 2.0, 2.0, 2.0, 2.0, 2.0, 40.0, 2.0, 7.0, 130.0];
    let length = |key: u64| lengths[usize::try_from(key).unwrap_or(0) % lengths.len()];
    let list_keys = counted(300);
    let view = |screen: &Screen| {
        let scroll = Scroll::down();
        let mut rows = screen.rows(&list(), &scroll, list_keys.len(), |at| ItemKey::new(list_keys[at]));
        for at in rows.places() {
            let key = list_keys[at];
            let filled = Style { height: Length::Pixels(length(key)), ..Style::default() };
            let inner = Node::new(Segment::Place(0), Column, filled, vec![]);
            let wrapper = Node::new(
                Segment::Place(0),
                Column,
                Style { padding: spaced(1.0, 1.0), ..Style::default() },
                vec![inner],
            );
            let style = Style {
                padding: spaced(3.0, 1.0),
                border: Border { width: 1.0, colour: Colour::CLEAR },
                margin: Edges { top: Length::Pixels(2.0), bottom: Length::Pixels(1.0), ..Edges::all(Length::Auto) },
                ..Style::default()
            };
            rows.push(at, Node::new(Segment::Item(ItemKey::new(key)), Column, style, vec![wrapper]));
        }
        let (lines, built) = rows.finish();
        let style = Style { gap: Gap { rows: Space::Pixels(1.0), columns: Space::Pixels(0.0) }, ..tall() };
        let scroll = Node::new(Segment::Place(0), scroll.list(lines), style, built);

        Tree::new(Node::new(Segment::Place(0), Column, Style::default(), vec![scroll]))
    };
    let tree = view(&screen);
    shown(&mut screen, tree);
    let tree = view(&screen);
    shown(&mut screen, tree);
    let windowing = screen.unkept.windows.iter().find(|window| *window.path() == list()).map(Window::windowing);

    // The shortest row: 2 pixels, 1 of its wrapper's padding each side, 3 and 1 of its own padding, 1 of border each
    // side, 2 and 1 of margin; 13 in all, and the gap of 1 after it. A view of 100 shows at most ceil(100 / 14) + 1.
    assert_eq!(windowing, Some(Windowing::Bounded { least: 14.0 }));
    let most = usize::try_from(9 + 2 * MARGIN).unwrap_or(usize::MAX);
    let entry = hit(&screen, &list());
    for turn in 0..120 {
        let tree = view(&screen);
        let (_, rows, _) = shown(&mut screen, tree);
        assert!(rows.len() <= most, "turn {turn}: {} rows built: {rows:?}", rows.len());
        // Each row stands 4 pixels past the one before, its margins and the gap between; the first 2 past the start.
        let mut reached = -2.0;
        for key in &rows {
            let rect = placed(&screen, &list().child(Segment::Item(*key))).rect;
            let gap = rect.min.y > reached + 4.0 + 0.01;
            let in_view = rect.min.y < VIEW;
            let shows_gap = gap && in_view;
            assert!(!shows_gap, "turn {turn}: a gap before row {key:?} at {}: {rows:?}", rect.min.y);
            reached = rect.max.y.max(reached);
        }
        assert!(reached >= VIEW - 0.01, "turn {turn}: the rows built reach the view's end: {rows:?}");
        wheel(&mut screen, entry, 9.0);
    }
}

#[test]
fn a_fling_past_the_rows_built_settles_over_passes_with_no_gap() {
    // Catches a fling that stops at the last row built, so the wheel cannot go further than the window in one turn;
    // one that jumps there with nothing built to show; and a list in a scroll that builds every item to reach it.
    let mut screen = screen();
    let list_keys = counted(1000);
    let (_, unlaid, _) = frame(&mut screen, &list_keys);
    let (_, first, _) = frame(&mut screen, &list_keys);

    let entry = hit(&screen, &list());
    wheel(&mut screen, entry, 500.0);
    let fiftieth = Stand::At(Anchor { item: 50, key: Segment::Item(ItemKey::new(50)), into: 0.0 });
    let mut passes = 0;
    loop {
        let settled = stand(&screen, &list()) == fiftieth;
        if settled {
            break;
        }
        let (_, rows, _) = frame(&mut screen, &list_keys);
        assert!(fills(&screen, &rows), "pass {passes}: {rows:?}");
        passes += 1;
        assert!(passes < 10, "the fling settles: {:?}", stand(&screen, &list()));
    }
    frame(&mut screen, &list_keys);

    let shown = 11;
    assert_eq!(unlaid, items([0]), "a list never laid out builds the row it stands at, to measure");
    assert_eq!(first, items(0..u64::from(shown + MARGIN)));
    assert_eq!(top(&screen, 50), 0.0, "the fiftieth row stands at the top, 500 pixels down");
    let fiftieth = placed(&screen, &row(50));
    assert_eq!(fiftieth.clip, Rect2 { min: Vec2::ZERO, max: Vec2::new(192.0, VIEW) }, "cut to the scroll's box");
}

#[test]
fn sliding_past_a_row_moves_the_stand_to_the_next_and_what_shows_stays_put() {
    // Catches a stand kept in pixels from a start that moves as the window does, so the rows jump when the next view
    // builds from the new item; a stand that does not move on past the item it stood in, so the window never follows;
    // and one that does not move back onto the item before as the wheel turns up.
    let mut screen = screen();
    let list_keys = counted(1000);
    frame(&mut screen, &list_keys);
    frame(&mut screen, &list_keys);
    let entry = hit(&screen, &list());

    wheel(&mut screen, entry, 25.0);
    let down = stand(&screen, &list());
    frame(&mut screen, &list_keys);
    let slid = top(&screen, 2);
    frame(&mut screen, &list_keys);
    let rebuilt = top(&screen, 2);
    wheel(&mut screen, entry, -10.0);
    let up = stand(&screen, &list());
    frame(&mut screen, &list_keys);

    assert_eq!(down, Stand::At(Anchor { item: 2, key: Segment::Item(ItemKey::new(2)), into: 5.0 }));
    assert_eq!((slid, rebuilt), (-5.0, -5.0), "the row stays where it showed as the window follows");
    assert_eq!(up, Stand::At(Anchor { item: 1, key: Segment::Item(ItemKey::new(1)), into: 5.0 }));
    assert_eq!(top(&screen, 1), -5.0);
}

#[test]
fn sliding_past_rows_sends_nothing_for_a_row_that_stays_built() {
    // Catches rows laid out from the window's first one, so each row the window moves past moves every row built, in
    // what the scroll holds, and redraws it: the window would cost a rewrite of every row per row scrolled by, where
    // only the shift around them should move. The rows' lengths are not whole pixels, so a row placed by summing the
    // rows before it in another order lands a part of a pixel away, and is drawn anew.
    let mut screen = screen();
    let lengths = [10.3, 17.6, 13.9];
    let length = |key: u64| lengths[usize::try_from(key).unwrap_or(0) % lengths.len()];
    let list_keys = counted(1000);
    let view = |screen: &mut Screen| {
        let tree = listed_in(screen, &list_keys, (Scroll::down(), tall()), length);
        shown(screen, tree)
    };
    view(&mut screen);
    view(&mut screen);
    let entry = hit(&screen, &list());

    let mut rewrites = Vec::new();
    for _ in 0..12 {
        wheel(&mut screen, entry, 35.0);
        let (_, rows, orders) = view(&mut screen);
        assert!(fills(&screen, &rows), "{rows:?}");
        let moved =
            orders.iter().filter(|order| matches!(order, Order::WriteInstances { .. } | Order::MoveText(_))).count();
        rewrites.push(moved);
    }

    assert_eq!(rewrites, vec![1; 12], "each pass rewrites the scroll's bar alone");
}

#[test]
fn an_unchanged_list_at_an_unchanged_stand_lays_nothing_out() {
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
fn a_scroll_kept_to_its_end_shows_what_is_appended_until_it_is_slid_back() {
    // Catches a console's scrollback or a chat that stays where it stood as rows are appended, so the newest never
    // shows; one that builds its window from where it stood, so the rows appended show a frame late as a gap; one that
    // follows the end after the wheel slid it back, so a person reading what scrolled by is pulled away by each row
    // appended; and one that never keeps to its end again once slid back down to it.
    let mut screen = screen();
    let at_end = |screen: &mut Screen, count: u64| {
        let tree = listed_in(screen, &counted(count), (Scroll::down().keeping_to_end(), tall()), |_| ROW);
        shown(screen, tree).1
    };
    at_end(&mut screen, 20);
    at_end(&mut screen, 20);
    let first = top(&screen, 10);

    let rows = at_end(&mut screen, 40);
    let grown = placed(&screen, &row(39)).rect.max.y;
    let entry = hit(&screen, &list());
    wheel(&mut screen, entry, -50.0);
    at_end(&mut screen, 40);
    let before = top(&screen, 25);
    at_end(&mut screen, 41);
    let slid_back = top(&screen, 25);
    wheel(&mut screen, entry, 1000.0);
    at_end(&mut screen, 41);
    at_end(&mut screen, 42);

    assert_eq!(first, 0.0, "a scroll never laid out starts at its end");
    assert!(rows.contains(&ItemKey::new(39)), "the newest row is built as it is appended: {rows:?}");
    assert_eq!(grown, VIEW, "the newest row stands at the end");
    assert_eq!((before, slid_back), (0.0, 0.0), "slid back, it keeps its row as rows are appended");
    assert!(matches!(stand(&screen, &list()), Stand::End { .. }));
    assert_eq!(placed(&screen, &row(41)).rect.max.y, VIEW, "slid down again, it keeps to its end");
}

#[test]
fn dropping_rows_above_the_stand_keeps_the_same_row_on_screen() {
    // Catches a stand kept by place alone, so dropping the oldest rows, as a chat lets go of its oldest five hundred,
    // shows rows five hundred further on.
    let mut screen = screen();
    let list_keys = counted(1000);
    frame(&mut screen, &list_keys);
    screen.jump(&list(), Jump::Item(700));
    frame(&mut screen, &list_keys);
    frame(&mut screen, &list_keys);
    let entry = hit(&screen, &list());
    wheel(&mut screen, entry, 5.0);
    frame(&mut screen, &list_keys);
    let before = top(&screen, 700);

    let kept: Vec<u64> = (500..1000).collect();
    frame(&mut screen, &kept);

    assert_eq!(before, -5.0);
    assert_eq!(top(&screen, 700), -5.0);
    assert_eq!(
        stand(&screen, &list()),
        Stand::At(Anchor { item: 200, key: Segment::Item(ItemKey::new(700)), into: 5.0 })
    );
}

#[test]
fn a_resize_keeps_the_row_the_view_starts_in_at_its_start() {
    // Catches a stand kept as a pixel offset from the list's start, or a share of it, so a view resized starts at
    // another row.
    let mut screen = screen();
    let list_keys = counted(1000);
    let filling = || (Scroll::down(), Style { height: Length::Share(1.0), ..Style::default() });
    let view = |screen: &mut Screen| {
        let tree = listed_in(screen, &list_keys, filling(), |key| {
            if key % 2 == 0 {
                ROW
            } else {
                2.0 * ROW
            }
        });
        shown(screen, tree);
    };
    view(&mut screen);
    view(&mut screen);
    let entry = hit(&screen, &list());
    wheel(&mut screen, entry, 35.0);
    view(&mut screen);
    let before = top(&screen, 2);

    let _ = screen.resize(Viewport::window(200, 60, 1.0));
    view(&mut screen);
    view(&mut screen);

    assert_eq!(before, -5.0);
    assert_eq!(top(&screen, 2), -5.0);
    assert_eq!(placed(&screen, &list()).content.max.y, 60.0);
}

#[test]
fn a_scroll_that_fails_a_condition_builds_every_item_and_says_so_once() {
    // Catches a window built for a list whose rows the scroll spreads by all of them, so the rows it did not build
    // would move those it did; rows that never stop being built a window of; and a warning logged every frame.
    let mut screen = screen();
    let list_keys = counted(40);
    let spread = || (Scroll::down(), Style { justify_content: Distribute::SpaceBetween, ..tall() });
    let tree = listed_in(&screen, &list_keys, spread(), |_| ROW);
    let (_, _, first) = shown(&mut screen, tree);
    let tree = listed_in(&screen, &list_keys, spread(), |_| ROW);
    let (_, every, second) = shown(&mut screen, tree);

    let warned = |orders: &[Order]| orders.iter().filter(|order| matches!(order, Order::Log(Level::Warn, _))).count();
    assert_eq!(every, items(0..40));
    assert_eq!((warned(&first), warned(&second)), (1, 0));
}

#[test]
fn a_list_in_a_plain_child_of_a_scroll_is_built_whole_and_said_so_once() {
    // Catches a list held by a column inside a scroll, which the scroll does not window, left silent, so a chat written
    // that way builds every message each frame unseen.
    let mut screen = screen();
    let nested = || {
        let rows = (0..30).map(|key| Node::new(Segment::Item(ItemKey::new(key)), Column, sized(10.0, ROW), vec![]));
        let column = Node::new(Segment::Place(0), Column, Style::default(), rows.collect());
        let scroll = Node::new(Segment::Place(0), Scroll::down(), tall(), vec![column]);
        Tree::new(Node::new(Segment::Place(0), Column, Style::default(), vec![scroll]))
    };
    let (_, first) = laid(&mut screen, nested(), &[]);
    let (_, second) = laid(&mut screen, nested(), &[]);

    let warned = |orders: &[Order]| orders.iter().filter(|order| matches!(order, Order::Log(Level::Warn, _))).count();
    assert_eq!((warned(&first), warned(&second)), (1, 0));
}

#[test]
fn the_thumb_dragged_goes_to_the_item_at_its_share_and_to_the_end_past_it() {
    // Catches a windowed scroll's thumb read as pixels of the rows built, so a drag moves through only the window; a
    // drag that stops at the scroll's edge rather than following the press past it, so the end is never reached; and
    // a drag that goes on after the release.
    let mut screen = screen();
    let list_keys = counted(1000);
    frame(&mut screen, &list_keys);
    frame(&mut screen, &list_keys);
    let entry = hit(&screen, &list());

    // The bar is the 8 pixels at the right; its thumb, 16 pixels at least, has 84 pixels to travel.
    press(&mut screen, entry, 196.0, 8.0);
    point(&mut screen, entry, PointerEvent::PointerMoved { x: 196.0, y: 50.0 });
    let halfway = stand(&screen, &list());
    point(&mut screen, entry, PointerEvent::PointerMoved { x: 500.0, y: 400.0 });
    release(&mut screen, entry, 500.0, 400.0, Over::Nothing);
    point(&mut screen, entry, PointerEvent::PointerMoved { x: 196.0, y: 20.0 });
    let after = stand(&screen, &list());
    frame(&mut screen, &list_keys);
    frame(&mut screen, &list_keys);

    assert!(matches!(halfway, Stand::Share(share) if (share - 0.495).abs() < 0.001), "{halfway:?}");
    assert!(matches!(after, Stand::End { .. }), "the release ends the drag: {after:?}");
    assert_eq!(placed(&screen, &row(999)).rect.max.y, VIEW);
}

#[test]
fn a_list_that_shrinks_past_where_its_scroll_stands_shows_its_last_rows() {
    // Catches a stand left past the end when the list shrinks, which shows an empty box until the wheel is turned back
    // the whole way.
    let mut screen = screen();
    frame(&mut screen, &counted(1000));
    screen.jump(&list(), Jump::End);
    frame(&mut screen, &counted(1000));
    frame(&mut screen, &counted(1000));

    frame(&mut screen, &counted(20));
    let (_, shrunk, _) = frame(&mut screen, &counted(20));

    assert!(fills(&screen, &shrunk), "{shrunk:?}");
    assert_eq!(placed(&screen, &row(19)).rect.max.y, VIEW);
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
    assert_eq!(placed(&screen, &row(10)).rect.max.y, VIEW);
}

#[test]
fn page_down_around_a_focused_row_slides_its_scroll_and_the_row_keeps_focus_out_of_view() {
    // Catches Page Down not declared or not handed on from a focused row to the scroll around it, so it does nothing
    // until the scroll itself is clicked; and a focused row dropped from the list once it scrolls out of the window,
    // which takes its focus, so the next Page Down goes nowhere. A page reaches past the rows built, so it settles over
    // two passes.
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
    frame(&mut screen, &list_keys);
    let (_, paged, _) = frame(&mut screen, &list_keys);
    let declared = keys(&screen, Role::Screen);
    key_down(&mut screen, declared, "PageDown", "PageDown");
    frame(&mut screen, &list_keys);
    let (_, twice, _) = frame(&mut screen, &list_keys);

    assert_eq!(top(&screen, 20), 0.0, "two pages down");
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
    let outer = Node::new(Segment::Place(0), Scroll::down(), tall(VIEW), vec![inner, below]);

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
    frame(&mut screen, &list_keys);
    frame(&mut screen, &list_keys);

    assert_eq!(screen.table().focus(), &Focus::Nowhere);
    assert_eq!(top(&screen, 10), 0.0);
}

/// The view of a screen holding a scroll down, [`VIEW`] pixels tall, over a list of `count` replies, each the markdown
/// `reply` built whole, in the monospace font, with no padding of its own, its images' pictures `images`: the replies the
/// screen says can show.
fn replies(screen: &Screen, count: u64, reply: &str, images: &Images) -> Tree {
    let prose = Prose::new(Inks {
        document: Style::default(),
        paragraph: Style { font: Font::Mono, ..Inks::default().paragraph },
        ..Inks::default()
    });
    let blocks = Document::new(reply).blocks();
    let scroll = Scroll::down();
    let mut rows = screen.rows(&list(), &scroll, usize::try_from(count).unwrap_or(0), ItemKey::from);
    for at in rows.places() {
        let markdown = Markdown::<Ev> {
            blocks: blocks.clone(),
            prose: prose.clone(),
            images: images.clone(),
            link: Raises::Nothing,
        };
        let key = Segment::Item(ItemKey::from(at));
        rows.push(at, markdown.node(key, Vec::new()));
    }
    let (lines, built) = rows.finish();
    let scroll = Node::new(Segment::Place(0), scroll.list(lines), tall(), built);

    Tree::new(Node::new(Segment::Place(0), Column, Style::default(), vec![scroll]))
}

#[test]
fn a_list_of_replies_each_only_a_rule_or_only_an_image_still_windows() {
    let Ok(wide) = Pixels::new(400, 40, vec![0; 400 * 40 * 4]) else {
        panic!("the pixels are four bytes a pixel");
    };
    let pictured = Images::new(vec![(String::from("wide.png"), wide)]);
    // Catches a reply whose blocks hold no line of text measured as sure to take nothing, which leaves the transcript
    // unbounded, so it builds every reply.
    let mono = mono();
    // A line's height is some thousands of font units, which an f32 holds exactly.
    #[allow(clippy::cast_precision_loss)]
    let line = mono.line_metrics().line_height() as f32 * mono.pixels_per_unit(Inks::default().paragraph.font_size);
    let cases = [
        ("---", Images::default(), 9.0),
        ("![a picture](picture.png)", Images::default(), line),
        ("![a picture](wide.png)", pictured, Picture::LEAST),
    ];
    for (reply, images, least) in cases {
        let mut screen = screen();
        screen.metrics_read(Font::Mono, mono.clone(), Vec::new());
        for _ in 0..2 {
            let tree = replies(&screen, 1000, reply, &images);
            shown(&mut screen, tree);
        }
        let windowing = screen.unkept.windows.iter().find(|window| *window.path() == list()).map(Window::windowing);

        // A rule is 1 pixel and 4 of margin each side; an image's alt text one line; a picture its least, whatever the
        // width shrinks it to.
        assert_eq!(windowing, Some(Windowing::Bounded { least }), "{reply}");
    }
}
