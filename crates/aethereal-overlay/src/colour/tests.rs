use super::{Ink, Oklch, Palette, Surface, Theme, linear};
use aethereal_render::text::Colour;
use aethereal_ui::widgets::SELECTION_STRENGTH;

/// A role's OKLCH comes out as the owner's palette sheet shows it, so a slipped coefficient, a hue in radians, or a
/// channel left unclamped or unencoded is caught.
#[test]
fn roles_come_out_as_the_palette_sheet_shows_them() {
    let sheet = |theme: Theme, surface: Surface| {
        Oklch { lightness: surface.lightness(), chroma: theme.surface_chroma(), hue: theme.hue() }.srgb()
    };

    // Tripwire: the sheet's hex for lichen grey's text and sheet, slate's raised, and warm stone's dim, which carry the
    // theme's hue and its two chromas; and for the input, mark, and error accents, at the greatest chroma.
    assert_eq!(Ink::Text.oklch(Theme::Lichen).srgb(), [0xda, 0xe4, 0xe2]);
    assert_eq!(sheet(Theme::Lichen, Surface::Sheet), [0x1d, 0x25, 0x23]);
    assert_eq!(sheet(Theme::Slate, Surface::Raised), [0x29, 0x2e, 0x34]);
    assert_eq!(Ink::Dim.oklch(Theme::Stone).srgb(), [0x9d, 0x97, 0x8f]);
    assert_eq!(Ink::Input.oklch(Theme::Slate).srgb(), [0xb6, 0xe4, 0xd0]);
    assert_eq!(Ink::Mark.oklch(Theme::Slate).srgb(), [0xd9, 0xba, 0x70]);
    assert_eq!(Ink::Error.oklch(Theme::Slate).srgb(), [0xd0, 0x64, 0x56]);
}

/// A palette hands each role back in linear light at its own place and opacity, so a colour sent as its sRGB
/// fraction, too pale on an sRGB surface, or a role read from its neighbour's place, is caught.
#[test]
fn a_palette_answers_each_role_in_linear_light() {
    let palette = Palette::new(Theme::Lichen);
    let text = palette.ink(Ink::Text);
    let sheet = palette.surface(Surface::Sheet);

    // Tripwire: 0xda is 0.7011019 in linear light, and 0x08, on the straight line, 0.0024282.
    assert!((linear(0xda) - 0.701_101_9).abs() < 1e-6);
    assert!((linear(0x08) - 0.002_428_2).abs() < 1e-6);
    assert_eq!([text.red, text.green, text.blue], [0xda, 0xe4, 0xe2].map(linear));
    assert_eq!([sheet.red, sheet.alpha], [linear(0x1d), 0.96]);
    assert_eq!(palette.surface(Surface::Band).alpha, 0.85);
    assert_eq!(palette.ink(Ink::Error), Palette::new(Theme::Stone).ink(Ink::Error));
    assert_ne!(palette.ink(Ink::Dim), Palette::new(Theme::Stone).ink(Ink::Dim));
}

/// How far apart two colours stand at least for one to be seen on the other: the ratio of their luminances, each with
/// the flare WCAG adds.
const SEEN_CONTRAST: f32 = 1.5;

/// The luminance of `colour`'s red, green, and blue, in linear light, with WCAG's flare.
fn luminance(colour: Colour) -> f32 {
    0.0722f32.mul_add(colour.blue, 0.2126f32.mul_add(colour.red, 0.7152 * colour.green)) + 0.05
}

/// `over` at `alpha` laid on `under`, in linear light, as the renderer blends.
fn laid_on(over: Colour, alpha: f32, under: Colour) -> Colour {
    let mix = |above: f32, below: f32| alpha.mul_add(above, (1.0 - alpha) * below);

    Colour::opaque(mix(over.red, under.red), mix(over.green, under.green), mix(over.blue, under.blue))
}

/// The selection, faded as the widget library fades it, stands apart from the sheet and from the raised band of the
/// owner's lines in every theme, so a selection is seen wherever a selectable line lies.
#[test]
fn a_selection_is_seen_on_every_surface_a_selectable_line_lies_on_in_every_theme() {
    // Catches a selection drawn in an ink, or faded so far, that it all but vanishes on one of the surfaces, as a dark
    // ink on the raised band would.
    for theme in Theme::ALL {
        let palette = Palette::new(theme);
        for surface in [Surface::Sheet, Surface::Raised] {
            let under = palette.surface(surface);
            let selected = laid_on(palette.selection(), SELECTION_STRENGTH, under);
            let contrast = luminance(selected) / luminance(under);

            assert!(contrast >= SEEN_CONTRAST, "{theme:?} on {surface:?}: {contrast}");
        }
    }
}
