//! Layout: where Taffy puts each node, by its style's flex and grid settings, at the screen's size and UI scale.

use super::{Boxed, Layouts};
use crate::fonts::{Fonts, Texts};
use crate::style::{Edges, Gap, Length, Space, Style, Track};
use crate::tree::{Node, Segment, Tree};
use crate::widgets::{Column, Grid, Row, Stack};
use aethereal_math::{Rect2, Vec2};

/// A box `width` by `height` logical pixels with nothing in it.
fn sized(place: u32, width: f32, height: f32) -> Node {
    let style = Style { width: Length::Pixels(width), height: Length::Pixels(height), ..Style::default() };

    Node::new(Segment::Place(place), Column, style, Vec::new())
}

/// The boxes of `tree`, in tree order, laid out in a screen `size` across and down at `scale`.
fn boxes(tree: &Tree, size: Vec2, scale: f32) -> Vec<Boxed> {
    let Ok(flattened) = tree.flat() else {
        panic!("the tree lays out");
    };
    let fonts = Fonts::default();
    let mut texts = Texts::default();
    let mut measuring = fonts.measuring(&mut texts, scale);

    Layouts::default().lay_out(&flattened.nodes, &[], &mut measuring, size, scale).boxes.clone()
}

/// The rect from `x`, `y`, `width` by `height`.
fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect2 {
    Rect2::from_origin_size(Vec2::new(x, y), Vec2::new(width, height))
}

#[test]
fn a_row_sets_its_children_across_by_its_padding_and_gap_and_grows_the_one_that_grows() {
    // Catches a row laid out as a column; padding or a gap dropped on the way to Taffy; grow ignored; and children
    // placed relative to their parent rather than to the screen, which moves every nested box to the corner.
    let style = Style {
        padding: Edges::all(Space::Pixels(5.0)),
        gap: Gap { rows: Space::Pixels(0.0), columns: Space::Pixels(10.0) },
        ..Style::default()
    };
    let grows = Node::new(
        Segment::Place(1),
        Column,
        Style { height: Length::Pixels(20.0), grow: 1.0, ..Style::default() },
        Vec::new(),
    );
    let row = Node::new(Segment::Place(0), Row, style, vec![sized(0, 30.0, 20.0), grows]);
    let tree = Tree::new(Node::new(Segment::Place(0), Column, Style::default(), vec![row]));

    let laid = boxes(&tree, Vec2::new(200.0, 100.0), 1.0);

    assert_eq!(laid[0].rect, rect(0.0, 0.0, 200.0, 100.0), "the root is the screen's size");
    assert_eq!(laid[1].content, rect(5.0, 5.0, 190.0, 20.0));
    assert_eq!(laid[2].rect, rect(5.0, 5.0, 30.0, 20.0));
    assert_eq!(laid[3].rect, rect(45.0, 5.0, 150.0, 20.0), "past the gap, grown to the padding");
}

#[test]
fn a_grid_sets_its_children_in_its_columns_and_a_stack_sets_them_over_one_another() {
    // Catches columns dropped on the way to Taffy, so every child takes a row of its own; a fraction track taken as
    // pixels; and a stack laid out as a column, so a child meant to cover another sits below it.
    let style = Style {
        columns: vec![Track::Pixels(40.0), Track::Fraction(1.0)],
        gap: Gap { rows: Space::Pixels(2.0), columns: Space::Pixels(0.0) },
        width: Length::Pixels(100.0),
        ..Style::default()
    };
    let grid = Node::new(
        Segment::Place(0),
        Grid,
        style,
        vec![sized(0, 10.0, 10.0), sized(1, 10.0, 10.0), sized(2, 10.0, 10.0)],
    );
    let stack = Node::new(Segment::Place(1), Stack, Style::default(), vec![sized(0, 30.0, 8.0), sized(1, 10.0, 4.0)]);
    let tree = Tree::new(Node::new(Segment::Place(0), Column, Style::default(), vec![grid, stack]));

    let laid = boxes(&tree, Vec2::new(200.0, 100.0), 1.0);

    let origins: Vec<Vec2> = laid.iter().map(|boxed| boxed.rect.min).collect();
    assert_eq!(origins[2], Vec2::new(0.0, 0.0));
    assert_eq!(origins[3], Vec2::new(40.0, 0.0), "the second column starts past the first's 40 pixels");
    assert_eq!(origins[4], Vec2::new(0.0, 12.0), "the third child wraps to the next row, past the gap");
    assert_eq!(laid[3].rect.width(), 10.0);
    let stack_top = laid[5].rect.min.y;
    assert_eq!(origins[6], Vec2::new(0.0, stack_top));
    assert_eq!(origins[7], Vec2::new(0.0, stack_top), "the stack's second child covers its first");
}

#[test]
fn the_ui_scale_multiplies_every_length() {
    // Catches a scale applied to sizes and not to padding or gaps, or not at all, so a larger scale overlaps what it
    // was meant to enlarge.
    let style = Style {
        padding: Edges::all(Space::Pixels(5.0)),
        gap: Gap { rows: Space::Pixels(0.0), columns: Space::Pixels(10.0) },
        ..Style::default()
    };
    let row = Node::new(Segment::Place(0), Row, style, vec![sized(0, 30.0, 20.0), sized(1, 30.0, 20.0)]);
    let tree = Tree::new(Node::new(Segment::Place(0), Column, Style::default(), vec![row]));

    let laid = boxes(&tree, Vec2::new(400.0, 200.0), 2.0);

    assert_eq!(laid[2].rect, rect(10.0, 10.0, 60.0, 40.0));
    assert_eq!(laid[3].rect, rect(90.0, 10.0, 60.0, 40.0));
}
