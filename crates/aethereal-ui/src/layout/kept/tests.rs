//! A layout from what a screen kept is the layout a fresh screen would make: each test changes one kind of what layout
//! reads between trees, lays the new tree out from what was kept, as a screen does, and compares it with the same tree
//! laid out by layouts and texts that kept nothing.

use super::Layouts;
use crate::fonts::{Fonts, Measuring, Texts};
use crate::layout::Laidout;
use crate::style::{Edges, Length, Space, Style};
use crate::tree::{Node, Segment, Tree};
use crate::widget::{Arrange, Draws, Laid, Lines, Offer, Stateless, Widget};
use crate::widgets::{Column, Row, Scroll, Text};
use aethereal_math::Vec2;
use aethereal_render::font::{Font as FontFile, Metrics};
use aethereal_render::text::{Font, Run, Wrap, measure};

/// The renderer's own monospace font.
const MONO: &[u8] = include_bytes!("../../../../aethereal-render/fonts/IBMPlexMono-Regular.ttf");

/// The size of the screen the trees are laid out in, unless a test resizes it.
const SCREEN: Vec2 = Vec2::new(320.0, 200.0);

/// The monospace font's metrics, as the renderer hands them.
fn mono() -> Metrics {
    let Ok(font) = FontFile::read(MONO) else {
        panic!("the renderer's monospace font reads");
    };

    font.metrics().clone()
}

/// The fonts of a screen that knows the monospace font's metrics.
fn known() -> Fonts {
    let mut fonts = Fonts::default();
    fonts.read(Font::Mono, mono(), Vec::new());

    fonts
}

/// What a screen keeps from one layout to the next, with the fonts it knows and the tree it last laid out.
struct Screen {
    fonts: Fonts,
    texts: Texts,
    layouts: Layouts,
    last: Tree,
}

impl Screen {
    /// A screen knowing `fonts` that has laid `tree` out in a screen `size` across and down at `scale`.
    fn new(fonts: Fonts, tree: Tree, size: Vec2, scale: f32) -> Self {
        let mut screen = Self { fonts, texts: Texts::default(), layouts: Layouts::default(), last: tree };
        let _ = screen.laid(size, scale);

        screen
    }

    /// Takes in `tree` as a screen does, compared with the last so what it kept is told what changed, without laying
    /// it out.
    fn built(&mut self, tree: Tree) {
        let Self { texts, layouts, last, .. } = self;
        let _ = last.compare(&tree, &mut (layouts, texts));
        self.last = tree;
    }

    /// Takes in `tree` and lays it out from what was kept, in a screen `size` across and down at `scale`.
    fn next(&mut self, tree: Tree, size: Vec2, scale: f32) -> Laidout {
        self.built(tree);

        self.laid(size, scale)
    }

    /// Lays the last tree out from what was kept, in a screen `size` across and down at `scale`, as one pass of a
    /// screen does.
    fn laid(&mut self, size: Vec2, scale: f32) -> Laidout {
        let Self { fonts, texts, layouts, last } = self;
        let Ok(flattened) = last.flat() else {
            panic!("the tree lays out");
        };
        texts.begin(scale);
        let mut measuring = fonts.measuring(texts, scale);
        let Laidout { boxes, pitches } = layouts.lay_out(&flattened.nodes, &[], &mut measuring, size, scale);
        let laidout = Laidout { boxes: boxes.clone(), pitches: pitches.clone() };
        texts.sweep();

        laidout
    }
}

/// `tree` laid out in a screen `size` across and down at `scale` by layouts and texts that kept nothing, with `fonts`.
fn fresh(fonts: &Fonts, tree: &Tree, size: Vec2, scale: f32) -> Laidout {
    let Ok(flattened) = tree.flat() else {
        panic!("the tree lays out");
    };
    let mut texts = Texts::default();
    let mut layouts = Layouts::default();
    let mut measuring = fonts.measuring(&mut texts, scale);
    let Laidout { boxes, pitches } = layouts.lay_out(&flattened.nodes, &[], &mut measuring, size, scale);

    Laidout { boxes: boxes.clone(), pitches: pitches.clone() }
}

/// A text of `text` in the monospace font at `place`, styled by `style` over that font.
fn text(place: u32, text: &str, style: Style) -> Node {
    Node::new(Segment::Place(place), Text::new(text), Style { font: Font::Mono, ..style }, Vec::new())
}

/// A box `width` by `height` logical pixels at `place`, holding nothing.
fn sized(place: u32, width: f32, height: f32) -> Node {
    let style = Style { width: Length::Pixels(width), height: Length::Pixels(height), ..Style::default() };

    Node::new(Segment::Place(place), Column, style, Vec::new())
}

/// The screen's tree: a column of a row holding `label` beside a box, a text that wraps within a column a share of the
/// screen wide, and a scroll that built the items at `keys` of a list of fifty, so a change anywhere in it moves what is
/// laid out after it.
fn page(label: &str, keys: &[u32]) -> Tree {
    let row = Node::new(
        Segment::Place(0),
        Row,
        Style { padding: Edges::all(Space::Pixels(4.0)), ..Style::default() },
        vec![text(0, label, Style::default()), sized(1, 30.0, 12.0)],
    );
    let narrow = Style { width: Length::Share(0.3), ..Style::default() };
    let wrapped = Node::new(
        Segment::Place(1),
        Column,
        narrow,
        vec![text(0, "a text long enough to wrap in its column", Style::default())],
    );
    let window = Scroll::down().list(Lines::Window { count: 50, at: keys.to_vec() });
    let scroll = Node::new(Segment::Place(2), window, Style { grow: 1.0, ..Style::default() }, items(keys));

    Tree::new(Node::new(Segment::Place(0), Column, Style::default(), vec![row, wrapped, scroll]))
}

/// A scroll's items, keyed by `keys`, each a row of a text naming it.
fn items(keys: &[u32]) -> Vec<Node> {
    keys.iter()
        .map(|key| {
            let named = text(0, &format!("item {key}"), Style::default());
            Node::new(Segment::Item((*key).into()), Row, Style::default(), vec![named])
        })
        .collect()
}

#[test]
fn a_text_that_changes_lays_out_as_a_fresh_screen_would() {
    // Catches a changed text answered from what its widget measured before, by Taffy's cache of the leaf or of what
    // holds it, or by the text cache keyed without its runs; and a text measured anew but its row left at the old
    // width, as only the leaf was let go.
    let fonts = known();
    let mut screen = Screen::new(known(), page("Gold: 10", &[1, 2]), SCREEN, 1.0);

    for label in ["Gold: 1000", "Gold: 10", "Gold: 1000000"] {
        let laid = screen.next(page(label, &[1, 2]), SCREEN, 1.0);

        assert_eq!(laid, fresh(&fonts, &page(label, &[1, 2]), SCREEN, 1.0), "{label}");
    }
}

#[test]
fn a_style_that_changes_lays_out_as_a_fresh_screen_would() {
    // Catches a changed style answered from what the node, or what holds it, kept: a padding or width that takes
    // effect only once something else lays the screen out.
    let fonts = known();
    let styled = |padding: f32, width: f32| {
        let inner = Node::new(
            Segment::Place(0),
            Column,
            Style { padding: Edges::all(Space::Pixels(padding)), ..Style::default() },
            vec![sized(0, width, 10.0), text(1, "Silver", Style::default())],
        );
        Tree::new(Node::new(Segment::Place(0), Row, Style::default(), vec![inner, sized(1, 20.0, 20.0)]))
    };
    let mut screen = Screen::new(known(), styled(2.0, 40.0), SCREEN, 1.0);

    for (padding, width) in [(9.0, 40.0), (9.0, 70.0), (2.0, 40.0)] {
        let laid = screen.next(styled(padding, width), SCREEN, 1.0);

        assert_eq!(laid, fresh(&fonts, &styled(padding, width), SCREEN, 1.0), "{padding} {width}");
    }
}

#[test]
fn a_node_added_or_removed_lays_out_as_a_fresh_screen_would() {
    // Catches a parent whose children changed answered from what it kept, so the new child is laid nowhere or the
    // gone one still takes room; boxes kept by place rather than key, so a node that moved in tree order takes
    // another's; and a node that left and came back with other settings, with no layout between, answered from what
    // it kept before it left.
    let fonts = known();
    let mut screen = Screen::new(known(), page("Gold", &[1, 2, 3]), SCREEN, 1.0);

    for keys in [&[0, 1, 2, 3][..], &[1, 3], &[3, 1], &[]] {
        let laid = screen.next(page("Gold", keys), SCREEN, 1.0);

        assert_eq!(laid, fresh(&fonts, &page("Gold", keys), SCREEN, 1.0), "{keys:?}");
    }
    let labelled = |label: &str| {
        let row = vec![sized(0, 10.0, 10.0), text(1, label, Style::default())];
        Tree::new(Node::new(Segment::Place(0), Row, Style::default(), row))
    };
    let alone = Tree::new(Node::new(Segment::Place(0), Row, Style::default(), vec![sized(0, 10.0, 10.0)]));
    let _ = screen.next(labelled("a label long enough to notice"), SCREEN, 1.0);
    screen.built(alone);
    let back = screen.next(labelled("short"), SCREEN, 1.0);
    assert_eq!(back, fresh(&fonts, &labelled("short"), SCREEN, 1.0));
}

#[test]
fn a_resize_or_another_ui_scale_lays_out_as_a_fresh_screen_would() {
    // Catches the last boxes handed back though the screen changed size or scale; a text measured at one scale
    // answered at another, by the text cache or Taffy's; and a wrapped text answered at a width it was not wrapped at.
    let fonts = known();
    let tree = || page("Gold: 10", &[1, 2]);
    let mut screen = Screen::new(known(), tree(), SCREEN, 1.0);

    for (size, scale) in [(Vec2::new(160.0, 90.0), 1.0), (Vec2::new(160.0, 90.0), 2.0), (SCREEN, 1.0)] {
        let laid = screen.next(tree(), size, scale);

        assert_eq!(laid, fresh(&fonts, &tree(), size, scale), "{size:?} {scale}");
    }
}

#[test]
fn a_font_s_metrics_coming_lays_out_as_a_fresh_screen_would() {
    // Catches what was laid out while a font's metrics had not come, every text measuring nothing, kept and handed back
    // once they have.
    let fonts = known();
    let mut screen = Screen::new(Fonts::default(), page("Gold", &[1]), SCREEN, 1.0);
    screen.fonts = known();

    let laid = screen.next(page("Gold", &[1]), SCREEN, 1.0);

    assert_eq!(laid, fresh(&fonts, &page("Gold", &[1]), SCREEN, 1.0));
}

/// A chart's leaf: as tall as `tall`, and drawing what slid `slide` through its box, which layout does not read.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Chart {
    tall: f32,
    slide: f32,
}

impl Widget for Chart {
    type State = Stateless;

    fn arrange(&self) -> Arrange {
        Arrange::Leaf
    }

    fn same_layout(&self, other: &Self) -> bool {
        self.tall.to_bits() == other.tall.to_bits()
    }

    fn measure(&self, _: &Style, _: &mut Measuring<'_>, _: Offer) -> Vec2 {
        Vec2::new(0.0, self.tall)
    }

    fn draw(&self, _: &Stateless, _: &Style, _: &Laid, _: &mut Draws<'_, '_>) {}
}

#[test]
fn a_change_layout_does_not_read_keeps_the_boxes_and_one_it_does_lays_out_anew() {
    // Catches a widget's hook read the wrong way round, so what slides every frame lays out anew and what it is sized
    // by is left at its old size; and the last boxes kept for a tree where a sibling of the chart changed with it.
    let fonts = known();
    let charted = |label: &str, tall: f32, slide: f32| {
        let chart = Node::new(Segment::Place(1), Chart { tall, slide }, Style::default(), Vec::new());
        Tree::new(Node::new(Segment::Place(0), Column, Style::default(), vec![text(0, label, Style::default()), chart]))
    };
    let mut screen = Screen::new(known(), charted("Gold", 30.0, 0.0), SCREEN, 1.0);

    for (label, tall, slide) in [("Gold", 30.0, 0.5), ("Gold", 50.0, 0.75), ("Silver", 50.0, 0.25)] {
        let laid = screen.next(charted(label, tall, slide), SCREEN, 1.0);

        assert_eq!(laid, fresh(&fonts, &charted(label, tall, slide), SCREEN, 1.0), "{label} {tall} {slide}");
    }
}

/// Two texts side by side, as tabs' headers are: as wide as both, each measured on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pair {
    left: String,
    right: String,
}

impl Widget for Pair {
    type State = Stateless;

    fn arrange(&self) -> Arrange {
        Arrange::Leaf
    }

    fn measure(&self, style: &Style, measuring: &mut Measuring<'_>, _: Offer) -> Vec2 {
        let size = style.font_size * measuring.scale();
        let mut wide = |text: &str| {
            let runs = [Run::new(text, style.colour)];
            measuring.text(Font::Mono, size, &runs, Wrap::None).map_or(0.0, |measured| measured.width)
        };

        Vec2::new(wide(&self.left) + wide(&self.right), 1.0)
    }

    fn draw(&self, _: &Stateless, _: &Style, _: &Laid, _: &mut Draws<'_, '_>) {}
}

#[test]
fn each_text_a_widget_measures_is_measured_as_itself() {
    // Catches texts kept by their widget's key alone, so the second text a widget measures is answered with the
    // first's box, as every header of a tab strip would be the first's width.
    let style = Style::default();
    let pair = Pair { left: String::from("ab"), right: String::from("a much longer one") };
    let row = Node::new(
        Segment::Place(0),
        Row,
        Style::default(),
        vec![Node::new(Segment::Place(0), pair, style.clone(), Vec::new())],
    );
    let tree = Tree::new(Node::new(Segment::Place(0), Column, Style::default(), vec![row]));
    let mut screen = Screen::new(known(), tree, SCREEN, 1.0);

    let laid = screen.laid(SCREEN, 1.0);

    let wide = |text: &str| measure(&mono(), &[], style.font_size, &[Run::new(text, style.colour)], Wrap::None).width;
    assert_eq!(laid.boxes[2].rect.width(), wide("ab") + wide("a much longer one"));
}
