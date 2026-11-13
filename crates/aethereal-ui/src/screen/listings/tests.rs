//! What a screen lists from what its widgets kept is what a fresh screen lists: each test lays a page out, changes one
//! kind of what drawing reads, lays it out again from what was kept, and compares the draws, their order, their clips,
//! and the hits with those of a screen that kept nothing, holding the same state, laying the changed page out once.

use super::super::input::tests::laid;
use super::super::tests::{mono, screen};
use super::super::{DrawKey, Fresh, Outcome, Screen, Slot, Viewport};
use crate::draw::Content;
use crate::style::{Edges, Gap, Length, Space, Style};
use crate::table::Focus;
use crate::tree::{KeyPath, Node, Segment, Tree};
use crate::widget::{Arrange, Draws, Laid, Lines, Quad, Sample, Stateless, Widget};
use crate::widgets::{Column, Row, Scroll, Text};
use aethereal_math::Vec2;
use aethereal_render::font::Font as FontFile;
use aethereal_render::text::{Colour, Font};

/// A swatch of `colour` filling its box, which lays out as any other swatch does, so a change of colour is drawing
/// alone.
#[derive(Debug, PartialEq)]
struct Swatch {
    colour: Colour,
}

impl Widget for Swatch {
    type State = Stateless;

    fn arrange(&self) -> Arrange {
        Arrange::Leaf
    }

    fn same_layout(&self, _: &Self) -> bool {
        true
    }

    fn draw(&self, _: &Stateless, _: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        draws.quad(Quad {
            rect: laid.rect,
            clip: laid.clip,
            colour: self.colour,
            border_width: 0.0,
            border_colour: self.colour,
            radius: 0.0,
            sample: Sample::Solid,
        });
    }
}

const RED: Colour = Colour::opaque(1.0, 0.0, 0.0);
const BLUE: Colour = Colour::opaque(0.0, 0.0, 1.0);

/// What a page is built of, each part one input a test changes.
#[derive(Clone)]
struct Page {
    /// The label at the head of the page, beside its swatch: its length moves the swatch.
    label: &'static str,
    /// The head's swatch's colour.
    swatch: Colour,
    /// The background of the line beneath the head.
    line: Colour,
    /// Whether the head holds a second swatch.
    second: bool,
    /// The items of a list of fifty the scroll built.
    items: Vec<u32>,
}

impl Page {
    /// The page every test starts from.
    fn first() -> Self {
        Self { label: "Gold", swatch: RED, line: BLUE, second: false, items: (0..8).collect() }
    }

    /// Its tree: a column of a row holding the label and its swatch, or two, a line of text over a background, and a
    /// scroll down through the items built of a list of fifty, each a row of a text naming it and a swatch.
    fn tree(&self) -> Tree {
        let mono = Style { font: Font::Mono, ..Style::default() };
        let square = Style { width: Length::Pixels(10.0), height: Length::Pixels(10.0), ..Style::default() };
        let mut head = vec![
            Node::new(Segment::Place(0), Text::new(self.label), mono.clone(), Vec::new()),
            Node::new(Segment::Place(1), Swatch { colour: self.swatch }, square.clone(), Vec::new()),
        ];
        if self.second {
            head.push(Node::new(Segment::Place(2), Swatch { colour: BLUE }, square.clone(), Vec::new()));
        }
        let row = Style { gap: Gap { rows: Space::Pixels(4.0), columns: Space::Pixels(4.0) }, ..Style::default() };
        let line = Style { background: self.line, ..mono.clone() };
        let items = self
            .items
            .iter()
            .map(|item| {
                let named = Node::new(Segment::Place(0), Text::new(&format!("item {item}")), mono.clone(), Vec::new());
                let swatch = Node::new(Segment::Place(1), Swatch { colour: RED }, square.clone(), Vec::new());
                Node::new(Segment::Item((*item).into()), Row, row.clone(), vec![named, swatch])
            })
            .collect();
        let window = Scroll::down().list(Lines::Window { count: 50, at: self.items.clone() });
        let scrolled = Style { grow: 1.0, padding: Edges::all(Space::Pixels(2.0)), ..Style::default() };
        let whole = Style { width: Length::Share(1.0), height: Length::Share(1.0), ..Style::default() };

        Tree::new(Node::new(
            Segment::Place(0),
            Column,
            whole,
            vec![
                Node::new(Segment::Place(0), Row, row, head),
                Node::new(Segment::Place(1), Text::new("a line"), line, Vec::new()),
                Node::new(Segment::Place(2), window, scrolled, items),
            ],
        ))
    }
}

/// The path of the page's scroll.
fn scroll() -> KeyPath {
    KeyPath::of(&[Segment::Place(0), Segment::Place(2)])
}

/// A screen that knows the monospace font's metrics, whose program is made.
fn ready() -> Screen {
    let mut screen = screen();
    screen.metrics_read(Font::Mono, mono(), Vec::new());

    screen
}

/// A screen's draws as they stand: its order, then each layer it keeps, by key.
#[derive(Debug, PartialEq)]
struct Drawing {
    order: Vec<Slot>,
    layers: Vec<(DrawKey, Content)>,
}

/// What `screen` draws.
fn drawing(screen: &Screen) -> Drawing {
    let layers = screen.layers.iter().map(|layer| (layer.key().clone(), layer.content().clone())).collect();

    Drawing { order: screen.order.clone(), layers }
}

/// What a screen that kept nothing draws of `page`, sized and holding the state `screen` holds.
fn fresh(screen: &Screen, page: &Page) -> Drawing {
    let mut fresh = ready();
    let _ = fresh.resize(screen.viewport);
    fresh.table = screen.table.clone();
    laid(&mut fresh, page.tree(), &[]);

    drawing(&fresh)
}

/// A screen that laid `page` out.
fn after(page: &Page) -> Screen {
    let mut screen = ready();
    laid(&mut screen, page.tree(), &[]);

    screen
}

/// Lays `page` out on `screen` from what it kept, and answers what it draws beside what a fresh screen draws.
fn again(screen: &mut Screen, page: &Page) -> (Drawing, Drawing) {
    laid(screen, page.tree(), &[]);

    (drawing(screen), fresh(screen, page))
}

#[test]
fn a_change_drawing_alone_reads_is_drawn() {
    // Catches a widget whose settings changed only in what layout does not read kept as it was drawn, as the tree's
    // comparison then lays nothing out and only its telling of the redrawn node lets go of what was kept.
    let first = Page::first();
    let mut screen = after(&first);

    let changed = Page { swatch: BLUE, ..first };
    let (kept, fresh) = again(&mut screen, &changed);

    assert_eq!(kept, fresh);
}

#[test]
fn a_change_of_style_is_drawn() {
    // Catches a widget whose style changed kept as it was drawn: a style is read by layout, so only the tree's telling
    // of the node that lays out otherwise lets go of what it kept, though its box stands where it stood.
    let first = Page::first();
    let mut screen = after(&first);

    let changed = Page { line: RED, ..first };
    let (kept, fresh) = again(&mut screen, &changed);

    assert_eq!(kept, fresh);
}

#[test]
fn a_node_added_and_taken_away_is_listed_as_the_tree_stands() {
    // Catches a widget added that lists nothing, or what a widget at its key listed before; and the draws of one taken
    // away kept in the list.
    let first = Page::first();
    let mut screen = after(&first);

    let added = Page { second: true, ..first.clone() };
    let (kept, fresh) = again(&mut screen, &added);
    assert_eq!(kept, fresh, "added");

    let (kept, fresh) = again(&mut screen, &first);
    assert_eq!(kept, fresh, "taken away");
}

#[test]
fn a_node_that_leaves_and_comes_back_before_a_layout_is_drawn_anew() {
    // Catches what a widget kept outliving it, when the tree it left was never laid out: the widget that comes back at
    // its key with other settings drawn as the one that left was.
    let first = Page { second: true, ..Page::first() };
    let mut screen = after(&first);

    let _ = screen.unkept.built(Page { second: false, ..first.clone() }.tree());
    let back = Page { second: true, swatch: BLUE, ..first };
    let (kept, fresh) = again(&mut screen, &back);

    assert_eq!(kept, fresh);
}

#[test]
fn a_box_moved_by_a_change_elsewhere_is_drawn_where_it_stands() {
    // Catches a widget whose own node is unchanged kept as drawn where it stood, when a longer label beside it moved
    // its box: what the comparison tells is not all a drawing reads.
    let first = Page::first();
    let mut screen = after(&first);

    let longer = Page { label: "Gold and silver", ..first };
    let (kept, fresh) = again(&mut screen, &longer);

    assert_eq!(kept, fresh);
}

#[test]
fn a_scroll_slid_by_a_part_of_a_line_and_by_lines_and_back_lists_its_rows_where_they_stand() {
    // Catches what a scroll holds kept as drawn before it slid, by a part of a line, by whole lines, and back to where it
    // was, whose rows then stand where they were drawn first.
    let page = Page::first();
    let mut screen = after(&page);

    for by in [3.5, 28.0, -31.5] {
        let _ = screen.wheel(&scroll(), Vec2::new(0.0, by));
        let (kept, fresh) = again(&mut screen, &page);
        assert_eq!(kept, fresh, "slid by {by}");
    }
}

#[test]
fn rows_entering_and_leaving_a_scroll_s_window_are_listed_as_it_stands() {
    // Catches a row that left the window still listed, one that entered listing what another row at its place drew,
    // and the rows that stayed listed where the rows before them put them.
    let first = Page::first();
    let mut screen = after(&first);
    let _ = screen.wheel(&scroll(), Vec2::new(0.0, 30.0));

    let moved = Page { items: (2..10).collect(), ..first };
    let (kept, fresh) = again(&mut screen, &moved);

    assert_eq!(kept, fresh);
}

#[test]
fn a_resize_lists_every_widget_where_it_now_stands() {
    // Catches a widget kept as drawn at the screen's last size, its box or its clip moved by the screen's.
    let page = Page::first();
    let mut screen = after(&page);

    let _ = screen.resize(Viewport::window(160, 72, 1.0));
    let (kept, fresh) = again(&mut screen, &page);

    assert_eq!(kept, fresh);
}

#[test]
fn another_ui_scale_draws_every_widget_at_it() {
    // Catches a widget kept as drawn at the last UI scale, whose borders, radii, and text sizes the scale multiplies.
    let page = Page::first();
    let mut screen = after(&page);

    let _ = screen.resize(Viewport::window(200, 100, 1.5));
    let (kept, fresh) = again(&mut screen, &page);

    assert_eq!(kept, fresh);
}

#[test]
fn focus_moved_onto_a_widget_draws_its_ring() {
    // Catches a widget kept as drawn when focus came onto it, so the ring that shows its focus never draws.
    let page = Page::first();
    let mut screen = after(&page);

    screen.table.focus_on(Focus::On(scroll()));
    screen.fresh = Fresh::Stale;
    let (kept, fresh) = again(&mut screen, &page);

    assert_eq!(kept, fresh);
}

#[test]
fn a_change_drawn_in_a_layout_that_waited_for_a_font_is_drawn_once_it_comes() {
    // Catches what a layout that met a font whose metrics had not come listed kept, though its draws were never sent:
    // a swatch whose colour changed then, listed as it was drawn and not as its draws stand.
    let style = Style { width: Length::Pixels(10.0), height: Length::Pixels(10.0), ..Style::default() };
    let page = |colour: Colour, labelled: bool| {
        let mut children = vec![Node::new(Segment::Place(0), Swatch { colour }, style.clone(), Vec::new())];
        if labelled {
            let sans = Style { font: Font::Sans, ..Style::default() };
            children.push(Node::new(Segment::Place(1), Text::new("Gold"), sans, Vec::new()));
        }
        Tree::new(Node::new(Segment::Place(0), Column, Style::default(), children))
    };
    let mut kept = ready();
    laid(&mut kept, page(RED, false), &[]);

    let (waited, _) = laid(&mut kept, page(BLUE, true), &[]);
    kept.metrics_read(Font::Sans, sans(), Vec::new());
    laid(&mut kept, page(BLUE, true), &[]);
    let mut fresh = ready();
    fresh.metrics_read(Font::Sans, sans(), Vec::new());
    laid(&mut fresh, page(BLUE, true), &[]);

    assert_eq!(waited, Outcome::Waiting);
    assert_eq!(drawing(&kept), drawing(&fresh));
}

/// The renderer's own proportional font's metrics, which set a text otherwise than the monospace font's.
fn sans() -> aethereal_render::font::Metrics {
    let Ok(font) = FontFile::read(SANS) else {
        panic!("the renderer's proportional font reads");
    };

    font.metrics().clone()
}

/// The renderer's own proportional font.
const SANS: &[u8] = include_bytes!("../../../../aethereal-render/fonts/IBMPlexSans-Regular.ttf");
