//! A screen that keeps what it worked out from one frame to the next shows what a fresh screen holding the same state
//! shows: each test lays a board out, changes one kind of input, lays it out again from what it kept, and compares the
//! tree it holds, where it laid each widget out, its draws, their order and its hits, and the list it sent, with those
//! of a screen that kept nothing, holding the same model, the same widget table, at the same size and scale.
//!
//! The board is written in markup, and keeps its lines with `memo`, so a change the screen keeps a composite through
//! wrongly shows as a line drawn as it was.

use super::input::tests::{hit, laid, over, press, release};
use super::tests::{mono, screen};
use super::{DrawKey, Last, Listed, Screen, Slot, Viewport, Worked};
use crate::Look;
use crate::draw::Content;
use crate::prelude::*;
use crate::table::{Focus, Table};
use crate::widget::Laid;
use crate::widgets::Text;
use aethereal_actor::{Actor, Asks, Beginning, ChildOf, Ending, Fault, Path, Top, Turn};
use aethereal_data::{Bound, Plain};
use aethereal_render::Renderer;
use aethereal_render::draw_list::Entry;
use aethereal_render::frame::{Frame, ShapeOf};
use aethereal_render::pass::Interface;
use aethereal_render::text::Font;

/// One note of a board.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.board_note")]
pub struct Note {
    id: u32,
    name: String,
    value: u32,
}

/// A group of a board's lines.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.board_group")]
pub struct Group {
    id: u32,
    entries: Vec<Note>,
}

/// A board: a title, a list of lines, a list of groups of lines, a pinned line shown or not, the line an event of the
/// pinned one picks, and what was searched.
#[derive(Bound)]
#[kind(name = "aether.test.board")]
pub struct Board {
    title: String,
    entries: Vec<Note>,
    groups: Vec<Group>,
    shown: bool,
    pinned: Note,
    chosen: u32,
    query: String,
    ui: Screen,
}

aethereal_ui::style! {
    pub whole { width: Length::Share(1.0), height: Length::Share(1.0) }
    pub lettered { font: Font::Mono }
}

aethereal_ui::widget! {
    /// A line: a button of its name and its value, raising `click`.
    pub struct Line { entry: Note }
    raises click;

    Button { click => click,
        Row {
            Text { style: lettered, "{entry.name}" }
            Text { style: lettered, "{entry.value}" }
        }
    }
}

aethereal_ui::view! { Board, ui,
    Column { style: whole,
        Text { style: lettered, "{self.title}" }
        Column { items: self.entries as entry key entry.id,
            Line { memo, entry: entry, click => self.pick(entry.id) }
        }
        Column { items: self.groups as group key group.id,
            Column { items: group.entries as entry key entry.id,
                Line { memo, entry: entry, click => self.pick(entry.id) }
            }
        }
        Line { memo, show: self.shown, entry: self.pinned, click => self.pick(self.chosen) }
        TextInput { style: lettered, text: self.query(), change => self.search(), submit => self.search() }
    }
}

/// What the board's methods send: none of their own.
type BoardSends = aethereal_ui::sends![];

impl Controls for Board {
    type Sends = BoardSends;
}

#[aethereal_ui::events]
impl Board {
    fn query(&self) -> String {
        self.query.clone()
    }

    fn pick(&mut self, _: &mut Turn<'_, Self, BoardSends>, id: u32) -> Result<(), Fault> {
        self.chosen = id;
        Ok(())
    }

    fn search(&mut self, _: &mut Turn<'_, Self, BoardSends>, query: String) -> Result<(), Fault> {
        self.query = query;
        Ok(())
    }
}

impl ChildOf for Board {
    type Parent = Top;
}

impl Actor for Board {
    type Config = Frame;
    type Key = ();
    type Handles = aethereal_ui::handles![];
    type BeginSends = (Asks<Path<Renderer>, ShapeOf, Shaping>,);
    type EndSends = ();

    fn begin(frame: Frame, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let ui = Screen::begin(Target::of::<Interface>(frame), Viewport::window(1, 1, 1.0), turn)?;

        Ok(Self::first(ui))
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// The line numbered `id`, of the value `value`.
fn entry(id: u32, value: u32) -> Note {
    Note { id, name: format!("line {id}"), value }
}

impl Board {
    /// The board every test starts from, over `ui`: six lines, two groups of three, and the pinned line shown.
    fn first(ui: Screen) -> Self {
        let group = |id: u32| Group { id, entries: (0..3).map(|at| entry(id * 10 + at, at)).collect() };

        Self {
            title: String::from("Board"),
            entries: (0..6).map(|id| entry(id, id * 7)).collect(),
            groups: vec![group(1), group(2)],
            shown: true,
            pinned: entry(100, 1),
            chosen: 3,
            query: String::new(),
            ui,
        }
    }

    /// The same board over `ui`.
    fn copied(&self, ui: Screen) -> Self {
        Self {
            title: self.title.clone(),
            entries: self.entries.clone(),
            groups: self.groups.clone(),
            shown: self.shown,
            pinned: self.pinned.clone(),
            chosen: self.chosen,
            query: self.query.clone(),
            ui,
        }
    }

    /// Builds its view and lays it out from what its screen kept, as a `Prepare` does.
    fn lay_out(&mut self) {
        let tree = self.view();
        let _ = laid(&mut self.ui, tree, &[]);
    }
}

/// A screen that knows the monospace font's metrics, whose program is made, the size of the test's screens.
fn ready() -> Screen {
    let mut screen = screen();
    screen.metrics_read(Font::Mono, mono(), Vec::new());
    let _ = screen.resize(Viewport::window(320, 400, 1.0));

    screen
}

/// A board laid out once.
fn laid_board() -> Board {
    let mut board = Board::first(ready());
    board.lay_out();

    board
}

/// What a screen shows, and what it sent of it: the tree it holds, where it laid each widget out, the pitch of each
/// scroll, its draws by key and their order with its hits, what each entry of the list it last sent is, each entry but
/// a draw as it was sent, and its widget table.
#[derive(Debug, PartialEq)]
struct Seen {
    tree: String,
    placed: Vec<(KeyPath, Laid)>,
    order: Vec<Slot>,
    layers: Vec<(DrawKey, Content)>,
    sent: Vec<(Listed, Option<Entry>)>,
    table: Table,
}

/// What `screen` shows.
fn seen(screen: &Screen) -> Seen {
    let Worked { last: Last::Built(tree), placed, .. } = &*screen.unkept else {
        panic!("the screen built a tree");
    };
    let layers = screen.layers.iter().map(|layer| (layer.key().clone(), layer.content().clone())).collect();
    let sent = screen
        .listing
        .iter()
        .zip(&screen.listed)
        .map(|(listed, entry)| {
            let drawn = matches!(listed, Listed::Draw(_));
            let entry = if drawn {
                None
            } else {
                Some(entry.clone())
            };
            (listed.clone(), entry)
        })
        .collect();

    Seen {
        tree: format!("{tree:?}"),
        placed: placed.clone(),
        order: screen.order.clone(),
        layers,
        sent,
        table: screen.table.clone(),
    }
}

/// What a screen that kept nothing shows of `board`, sized, scaled, and holding the widget table its screen holds.
fn fresh(board: &Board) -> Seen {
    let mut screen = ready();
    let _ = screen.resize(board.ui.viewport());
    screen.table = board.ui.table.clone();
    let mut fresh = board.copied(screen);
    fresh.lay_out();

    seen(&fresh.ui)
}

/// Lays `board` out again from what it kept, and answers what it shows beside what a fresh screen shows of it.
fn again(board: &mut Board) -> (Seen, Seen) {
    board.lay_out();

    (seen(&board.ui), fresh(board))
}

#[test]
fn a_line_whose_field_changed_is_built_again() {
    // Catches a kept composite taken whole though a field its markup writes changed: its line would show the old value.
    let mut board = laid_board();

    board.entries[2].value = 99;
    let (kept, fresh) = again(&mut board);

    assert_eq!(kept, fresh);
}

#[test]
fn a_line_whose_event_s_argument_changed_is_built_again() {
    // Catches a kept composite compared by its params alone: the pinned line's click would pick the line chosen before.
    let mut board = laid_board();

    board.chosen = 5;
    let (kept, fresh) = again(&mut board);

    assert_eq!(kept, fresh);
}

#[test]
fn a_group_whose_lines_changed_shows_them() {
    // Catches a nested list's line kept at the path of another group's line, or a group's change missed beneath it.
    let mut board = laid_board();

    board.groups[1].entries[0].name = String::from("renamed");
    board.groups[0].entries.push(entry(19, 4));
    let (kept, fresh) = again(&mut board);

    assert_eq!(kept, fresh);
}

#[test]
fn lines_inserted_removed_and_reordered_stand_as_the_list_does() {
    // Catches a line's kept tree, layout, or draws following its place rather than its key: a line inserted mid-list,
    // one removed, and the list reversed would each show a line where another stood.
    let mut board = laid_board();

    board.entries.insert(2, entry(40, 1));
    let (kept, fresh) = again(&mut board);
    assert_eq!(kept, fresh, "inserted");

    board.entries.remove(4);
    let (kept, fresh) = again(&mut board);
    assert_eq!(kept, fresh, "removed");

    board.entries.reverse();
    let (kept, fresh) = again(&mut board);
    assert_eq!(kept, fresh, "reversed");
}

#[test]
fn a_line_that_leaves_and_comes_back_is_built_again() {
    // Catches what a kept composite was built from outliving it: a line removed, then put back as it was, would be
    // taken from a tree that no longer holds it, and draw nothing.
    let mut board = laid_board();
    let gone = board.entries.remove(1);
    let (kept, fresh) = again(&mut board);
    assert_eq!(kept, fresh, "gone");

    board.entries.insert(1, gone);
    let (kept, fresh) = again(&mut board);

    assert_eq!(kept, fresh, "back");
}

#[test]
fn a_line_that_leaves_and_comes_back_between_layouts_is_built_again() {
    // Catches the same when the tree it left was built and never laid out, as input between frames builds one.
    let mut board = laid_board();
    let gone = board.entries.remove(1);
    let tree = board.view();
    board.ui.rebuilt(tree);

    board.entries.insert(1, gone);
    let (kept, fresh) = again(&mut board);

    assert_eq!(kept, fresh);
}

#[test]
fn groups_reordered_with_lines_moved_between_them_stand_as_the_lists_do() {
    // Catches a nested list kept by the place of its group: a line moved from one group to another would stay drawn
    // under the first.
    let mut board = laid_board();

    board.groups.reverse();
    let moved = board.groups[0].entries.remove(0);
    board.groups[1].entries.insert(1, moved);
    let (kept, fresh) = again(&mut board);

    assert_eq!(kept, fresh);
}

#[test]
fn a_line_hidden_and_shown_again_stands_as_the_board_does() {
    // Catches a line hidden by `show` kept in the tree or the list, and one shown again taken from nothing.
    let mut board = laid_board();

    board.shown = false;
    let (kept, fresh) = again(&mut board);
    assert_eq!(kept, fresh, "hidden");

    board.shown = true;
    let (kept, fresh) = again(&mut board);
    assert_eq!(kept, fresh, "shown");
}

#[test]
fn a_resize_and_another_scale_lay_every_line_out_anew() {
    // Catches what was kept of a line's layout or draws read at the last size or scale.
    let mut board = laid_board();

    let _ = board.ui.resize(Viewport::window(200, 300, 1.0));
    let (kept, fresh) = again(&mut board);
    assert_eq!(kept, fresh, "resized");

    let _ = board.ui.resize(Viewport::window(200, 300, 2.0));
    let (kept, fresh) = again(&mut board);
    assert_eq!(kept, fresh, "scaled");
}

#[test]
fn a_press_and_the_focus_on_a_kept_line_show_as_on_a_fresh_one() {
    // Catches a kept line drawn as it was before its button was pressed, or before focus moved onto it: input changes
    // the widget table, not the tree, so the line is kept and must still be drawn as its state stands.
    let mut board = laid_board();
    let button = KeyPath::of(&[Segment::Place(0), Segment::Place(1), Segment::Item(ItemKey::new(2))]);
    let entry = hit(&board.ui, &button);

    let _ = press(&mut board.ui, entry, 2.0, 2.0);
    let (kept, fresh) = again(&mut board);
    assert_eq!(kept, fresh, "pressed");
    assert_eq!(board.ui.table().focus(), &Focus::On(button), "the press focuses the line's button");

    let _ = release(&mut board.ui, entry, 2.0, 2.0, over(entry, 2.0, 2.0));
    let (kept, fresh) = again(&mut board);
    assert_eq!(kept, fresh, "released");
}

#[test]
fn a_text_typed_shows_as_on_a_fresh_screen() {
    // Catches the text input's state, or the query its change set, drawn as they were.
    let mut board = laid_board();

    board.query = String::from("line");
    let (kept, fresh) = again(&mut board);

    assert_eq!(kept, fresh);
}

#[test]
fn lines_laid_out_before_a_font_came_are_drawn_once_it_does() {
    // Catches what was kept of a line laid out while its font's metrics had not come: once they come, the line must be
    // measured and drawn as a fresh screen that knew the font draws it.
    let mut board = Board::first(screen());
    let _ = board.ui.resize(Viewport::window(320, 400, 1.0));
    board.lay_out();

    board.ui.metrics_read(Font::Mono, mono(), Vec::new());
    let (kept, fresh) = again(&mut board);

    assert_eq!(kept, fresh);
}

#[test]
fn a_kept_line_is_taken_whole_and_a_changed_one_is_built() {
    // Catches `memo` keeping nothing, so every line is built at every view, or keeping a line whose field changed.
    let mut board = laid_board();

    board.entries[2].value = 99;
    let tree = board.view();

    let line = |key: u64| KeyPath::of(&[Segment::Place(0), Segment::Place(1), Segment::Item(ItemKey::new(key))]);
    let kept = |key: u64| tree.find(&line(key)).is_some_and(Node::is_kept);
    assert!(kept(1), "an unchanged line is a marker the screen takes the last tree's line for");
    assert!(!kept(2), "a changed line is built");
}

#[test]
fn a_pressed_line_reordered_and_replaced_keeps_its_state_with_its_key() {
    // Catches a widget's state read at its place in tree order when the table no longer stands in that order: the
    // pressed line, its list reversed, would draw another line pressed; and a table taken as kept because it counts as
    // many states as the tree has nodes, though a line was replaced by another.
    let mut board = laid_board();
    let button = KeyPath::of(&[Segment::Place(0), Segment::Place(1), Segment::Item(ItemKey::new(4))]);
    let at = hit(&board.ui, &button);
    let _ = press(&mut board.ui, at, 2.0, 2.0);
    board.lay_out();

    board.entries.reverse();
    let (kept, fresh) = again(&mut board);
    assert_eq!(kept, fresh, "reversed");

    board.entries[0] = entry(41, 41);
    let (kept, fresh) = again(&mut board);
    assert_eq!(kept, fresh, "replaced");
}

#[test]
fn a_kept_line_placed_in_another_style_is_built_again() {
    // Catches a kept composite compared without the style it is placed with: a line whose element names another style
    // would keep the last one's background.
    let tree = |screen: &Screen, colour| {
        let note = entry(1, 1);
        let style = Style { background: colour, ..Style::default() };
        let line = screen.memo(
            &[Segment::Place(0), Segment::Place(0)],
            Look::Given(style),
            || Line { entry: note.clone(), click: BoardEvent::Pick(1) },
            |_| true,
        );
        Tree::new(Node::new(Segment::Place(0), Column, Style::default(), vec![line]))
    };
    let mut screen = ready();
    let built = tree(&screen, rgb(200, 0, 0));
    let _ = laid(&mut screen, built, &[]);

    let built = tree(&screen, rgb(0, 0, 200));
    let _ = laid(&mut screen, built, &[]);
    let mut fresh = ready();
    let built = tree(&fresh, rgb(0, 0, 200));
    let _ = laid(&mut fresh, built, &[]);

    assert_eq!(seen(&screen), seen(&fresh));
}
