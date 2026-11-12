//! `Text`: the box it measures is the box the renderer lays it out in, as it is made to be drawn.

use super::Text;
use crate::fonts::Fonts;
use crate::layout::lay_out;
use crate::style::{Alignment, Length, Style};
use crate::tree::{Node, Segment, Tree};
use crate::widget::{Draws, Focused, Laid, Piece, Stateless, TextDraw, Widget};
use crate::widgets::Column;
use aethereal_math::{Rect2, Vec2};
use aethereal_render::font::{Font as FontFile, Metrics};
use aethereal_render::text::{Colour, Font, Measured, Outline, Wrap, measure};

/// The renderer's own proportional font.
const SANS: &[u8] = include_bytes!("../../../../aethereal-render/fonts/IBMPlexSans-Regular.ttf");

/// The proportional font's metrics, as the renderer hands them.
fn sans() -> Metrics {
    let Ok(font) = FontFile::read(SANS) else {
        panic!("the renderer's proportional font reads");
    };

    font.metrics().clone()
}

/// What a column `column` styled holds of one text of `text` styled `style`, the column the child of a screen 400 by
/// 300 at a UI scale of 1: the text's border box, and what it draws.
fn drawn(column: Style, text: &str, style: &Style) -> (Rect2, TextDraw) {
    let text = Node::new(Segment::Place(0), Text::new(text), style.clone(), Vec::new());
    let column = Node::new(Segment::Place(0), Column, column, vec![text]);
    let tree = Tree::new(Node::new(Segment::Place(0), Column, Style::default(), vec![column]));
    let Ok(flattened) = tree.flat() else {
        panic!("the tree lays out");
    };
    let mut known = Fonts::default();
    known.read(Font::Sans, sans(), Vec::new());
    let mut measuring = known.measuring(1.0);
    let boxes = lay_out(&flattened.nodes, &mut measuring, Vec2::new(400.0, 300.0), 1.0).boxes;

    let laid_box = boxes[2];
    let screen = Rect2::from_origin_size(Vec2::ZERO, Vec2::new(400.0, 300.0));
    let laid =
        Laid { rect: laid_box.rect, content: laid_box.content, clip: screen, extent: laid_box.extent, scale: 1.0 };
    let mut pieces = Vec::new();
    let mut into = Draws::new(&mut pieces, &mut measuring, Focused::No);
    let Some(text) = flattened.nodes[2].node.widget().as_any().downcast_ref::<Text>() else {
        panic!("the third node is the text");
    };
    Widget::draw(text, &Stateless, style, &laid, &mut into);
    let [Piece::Text(drawn)] = pieces.as_slice() else {
        panic!("a text draws one text: {pieces:?}");
    };

    (laid_box.rect, drawn.clone())
}

/// The box the renderer lays `drawn` out in.
fn laid_out(drawn: &TextDraw) -> Measured {
    measure(&sans(), &[], drawn.size, &drawn.runs, drawn.wrap)
}

/// A column `width` logical pixels wide that sets its children at its start, so each is as wide as it measures.
fn column(width: f32) -> Style {
    Style { width: Length::Pixels(width), align_items: Alignment::Start, ..Style::default() }
}

const SENTENCE: &str = "The quick brown fox jumps over the lazy dog";

#[test]
fn a_wrapped_text_is_drawn_in_exactly_the_box_it_measured() {
    // Catches a text made with the wrap width it was offered rather than the width it measured, or unwrapped, so its
    // lines break elsewhere than they were measured to, and it spills out of its box or leaves it half empty.
    let (rect, drawn) = drawn(column(120.0), SENTENCE, &Style::default());

    let laid = laid_out(&drawn);

    assert!(laid.lines > 1, "the sentence wraps at 120 pixels: {laid:?}");
    assert!(matches!(drawn.wrap, Wrap::At { width } if width <= 120.0), "{:?}", drawn.wrap);
    assert_eq!((laid.width, laid.height), (rect.width(), rect.height()), "drawn as {laid:?} in {rect:?}");
    assert_eq!(drawn.at, rect.min);
}

#[test]
fn a_word_too_long_to_break_draws_at_the_width_offered_and_is_cut_by_its_box() {
    // Catches a text that wraps at the width it measured past its offer, which sets together lines the offer broke
    // apart; and one drawn uncut, so the word spills over what lies beside it.
    let style = Style { width: Length::Pixels(30.0), ..Style::default() };

    let (rect, drawn) = drawn(column(200.0), "Incomprehensibilities", &style);

    assert_eq!(drawn.wrap, Wrap::At { width: 30.0 });
    assert_eq!(rect.width(), 30.0);
    assert_eq!(drawn.clip, rect);
    assert!(laid_out(&drawn).width > 30.0, "the word measures wider than its box");
}

#[test]
fn a_text_with_a_rim_is_padded_by_the_rim_on_every_side() {
    // Catches a rim left out of a text's box, so the rim of its first and last glyphs is drawn over its neighbours and
    // cut by its own clip.
    let outline = Outline::Of { width: 2.0, colour: Colour::BLACK };
    let style = Style { outline, ..Style::default() };

    let (rect, drawn) = drawn(column(400.0), "Gold", &style);

    let laid = laid_out(&drawn);
    assert_eq!((rect.width(), rect.height()), (laid.width + 4.0, laid.height + 4.0));
    assert_eq!(drawn.at, rect.min + Vec2::new(2.0, 2.0));
}
