//! `Picture`: its box keeps its image's proportions within the width it is offered, never shorter than its least, and
//! it draws the whole image within its box.

use super::Picture;
use crate::fonts::{Fonts, Texts};
use crate::pixels::Pixels;
use crate::style::Style;
use crate::widget::{Draws, Focused, Laid, Lit, Offer, Offered, Piece, Span, Stateless, Widget};
use aethereal_math::{Rect2, Vec2};

/// A picture `width` by `height` of one gray.
fn picture(width: u32, height: u32) -> Picture {
    let bytes = usize::try_from(width * height * 4).unwrap_or(0);
    let Ok(pixels) = Pixels::new(width, height, vec![128; bytes]) else {
        panic!("the pixels are four bytes a pixel");
    };

    Picture { pixels }
}

/// The size `picture` measures at, offered `width` across, at a UI scale of 1.
fn measured(picture: &Picture, width: Offered) -> Vec2 {
    let fonts = Fonts::default();
    let mut texts = Texts::default();
    let mut measuring = fonts.measuring(&mut texts, 1.0);
    let offer = Offer { width, height: Offered::MaxContent };

    Widget::measure(picture, &Style::default(), &mut measuring, offer)
}

#[test]
fn a_picture_keeps_its_proportions_within_the_width_it_is_offered_and_is_never_enlarged() {
    // Catches a picture squeezed to the width with its own height kept, and one stretched past its own size.
    let wide = picture(200, 100);

    assert_eq!(measured(&wide, Offered::AtMost(100.0)), Vec2::new(100.0, 50.0));
    assert_eq!(measured(&wide, Offered::AtMost(400.0)), Vec2::new(200.0, 100.0));
    assert_eq!(measured(&wide, Offered::MaxContent), Vec2::new(200.0, 100.0));
}

#[test]
fn a_very_wide_picture_is_never_shorter_than_its_least() {
    // Catches a picture shrunk to a sliver, shorter than the least its scroll counts it at, which breaks the bound on
    // how many items fill a view.
    let banner = picture(4000, 40);

    let size = measured(&banner, Offered::AtMost(100.0));

    assert_eq!(size, Vec2::new(100.0, Picture::LEAST));
}

#[test]
fn a_picture_draws_its_whole_image_within_its_box_its_proportions_kept_centred() {
    // Catches an image stretched to its box, which a style that sizes the box would distort.
    let fonts = Fonts::default();
    let mut texts = Texts::default();
    let mut measuring = fonts.measuring(&mut texts, 1.0);
    let rect = Rect2::from_origin_size(Vec2::ZERO, Vec2::new(300.0, 100.0));
    let laid = Laid { rect, content: rect, clip: rect, extent: Vec2::ZERO, bar: Span::WHOLE, scale: 1.0 };
    let mut pieces = Vec::new();

    Widget::draw(
        &picture(200, 100),
        &Stateless,
        &Style::default(),
        &laid,
        &mut Draws::new(&mut pieces, &mut measuring, Focused::No, Lit::No),
    );

    let [Piece::Quad(quad)] = pieces.as_slice() else {
        panic!("a picture draws one quad: {pieces:?}");
    };
    assert_eq!(quad.rect, Rect2 { min: Vec2::new(50.0, 0.0), max: Vec2::new(250.0, 100.0) });
}
