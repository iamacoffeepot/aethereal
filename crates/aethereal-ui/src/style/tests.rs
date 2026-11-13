//! A style made by `style!` starts from its base and sets each property by its shape, and a colour written in sRGB is
//! read into the linear light the renderer's colours are in.

use super::{Edges, Gap, Space, Style, rgb, rgba};
use aethereal_render::text::Colour;

aethereal_ui::style! {
    pub panel { padding: 8, gap: 4 }
    pub title: panel { font_size: 24, colour: rgb(232, 217, 160) }
    pub cells { background: "#1c1814" }
}

#[test]
fn a_style_starts_from_its_base_and_sets_each_property_by_its_shape() {
    let expected = Style {
        padding: Edges::all(Space::Pixels(8.0)),
        gap: Gap { rows: Space::Pixels(4.0), columns: Space::Pixels(4.0) },
        font_size: 24.0,
        colour: rgb(232, 217, 160),
        ..Style::default()
    };

    assert_eq!(title(), expected);
    assert_eq!(cells().background, rgba(28, 24, 20, 255));
}

#[test]
fn a_colour_in_srgb_is_read_into_linear_light() {
    let grey = rgb(188, 188, 188).red;

    assert_eq!(rgb(255, 255, 255), Colour::WHITE);
    assert!((grey - 0.5).abs() < 0.005, "sRGB 188 is half the light of white, not {grey}");
}
