//! The fonts a screen measures text by (ADR-0056: Layout is Taffy; ADR-0052): each font's metrics and its fallbacks',
//! asked of the renderer once, the first time a build meets the font, and kept for good, as a font's metrics never
//! change. A text is measured with them by the renderer's own [`measure`], so its measured box is the box it is drawn
//! in, once for each wrap it is asked at while it stands unchanged ([`Texts`]). A text whose font's metrics have not
//! come is not measured, and the layout that met it is not drawn: the next `Prepare` after they come lays it out.

mod texts;

pub use texts::{Asker, Texts};

use crate::tree::KeyPath;
use aethereal_render::font::Metrics;
use aethereal_render::text::{Font, Measured, Run, Wrap, measure};

/// What a screen knows of each font it has met.
#[derive(Debug, Default)]
pub struct Fonts {
    known: Vec<(Font, Known)>,
}

/// What a screen knows of one font.
#[derive(Debug)]
enum Known {
    /// Its metrics are asked for, and have not come.
    Asked,
    /// Its own metrics, and its fallbacks', in the order they are tried.
    Read { own: Metrics, fallbacks: Vec<Metrics> },
    /// The renderer keeps no such font, or its metrics did not read: its text measures nothing and draws nothing.
    Refused,
}

impl Fonts {
    /// What is known of `font`, if it has been met.
    fn known(&self, font: Font) -> Option<&Known> {
        self.known.iter().find(|(known, _)| *known == font).map(|(_, known)| known)
    }

    /// Notes that `font`'s metrics are asked for.
    pub fn asked(&mut self, font: Font) {
        self.set(font, Known::Asked);
    }

    /// Keeps `font`'s metrics, its own and its fallbacks'.
    pub fn read(&mut self, font: Font, own: Metrics, fallbacks: Vec<Metrics>) {
        self.set(font, Known::Read { own, fallbacks });
    }

    /// Notes that `font`'s metrics will not come.
    pub fn refused(&mut self, font: Font) {
        self.set(font, Known::Refused);
    }

    /// Sets what is known of `font`.
    fn set(&mut self, font: Font, known: Known) {
        match self.known.iter_mut().find(|(met, _)| *met == font) {
            Some((_, was)) => *was = known,
            None => self.known.push((font, known)),
        }
    }

    /// A measuring of text with these fonts, at `scale` pixels to the logical pixel, keeping what it measures for a
    /// widget in `texts`.
    pub const fn measuring<'fonts>(&'fonts self, texts: &'fonts mut Texts, scale: f32) -> Measuring<'fonts> {
        Measuring { fonts: self, texts, whose: Whose::Unplaced, unmet: Vec::new(), waited: false, scale }
    }
}

/// Text measured with the fonts a screen knows, at its UI scale, for the widget being laid out or drawn, noting each
/// font met whose metrics have not come.
pub struct Measuring<'fonts> {
    fonts: &'fonts Fonts,
    texts: &'fonts mut Texts,
    whose: Whose<'fonts>,
    /// Each font met that was never asked for, in the order first met.
    unmet: Vec<Font>,
    /// Whether a text went unmeasured for want of its font's metrics.
    waited: bool,
    scale: f32,
}

impl Measuring<'_> {
    /// How many pixels a logical pixel is.
    #[must_use]
    pub const fn scale(&self) -> f32 {
        self.scale
    }

    /// The box `runs` are laid out in, in `font` at `size` pixels to the em, their lines broken as `wrap` says, as the
    /// renderer lays them out (see [`measure`]).
    ///
    /// # Errors
    ///
    /// [`Unmeasured::Waiting`] while the font's metrics have not come, noting the font, so the screen asks for them and
    /// draws nothing of this layout; [`Unmeasured::Refused`] for a font whose metrics will not come.
    pub fn text(&mut self, font: Font, size: f32, runs: &[Run], wrap: Wrap) -> Result<Measured, Unmeasured> {
        match self.fonts.known(font) {
            Some(Known::Read { own, fallbacks }) => {
                let asked = Asked { font, size, runs, wrap };
                let measured = match self.whose {
                    Whose::Widget { path, asker } => self.texts.measure(path, asked, asker, own, fallbacks),
                    Whose::Unplaced => measure(own, fallbacks, size, runs, wrap),
                };
                Ok(measured)
            }
            Some(Known::Refused) => Err(Unmeasured::Refused),
            Some(Known::Asked) => {
                self.waited = true;
                Err(Unmeasured::Waiting)
            }
            None => {
                self.waited = true;
                if !self.unmet.contains(&font) {
                    self.unmet.push(font);
                }
                Err(Unmeasured::Waiting)
            }
        }
    }

    /// Whether a text went unmeasured for want of its font's metrics, so what was laid out is not drawn.
    pub(crate) const fn waited(&self) -> bool {
        self.waited
    }

    /// Each font met that was never asked for, in the order first met.
    pub(crate) fn into_unmet(self) -> Vec<Font> {
        self.unmet
    }
}

impl<'fonts> Measuring<'fonts> {
    /// Measures for the widget at `path` from here on, as `asker` asks, keeping what it measures under its path.
    pub(crate) const fn at(&mut self, path: &'fonts KeyPath, asker: Asker) {
        self.whose = Whose::Widget { path, asker };
    }
}

/// Whose texts a measuring measures.
#[derive(Debug, Clone, Copy)]
enum Whose<'path> {
    /// No widget's in particular: each is measured afresh, and nothing is kept.
    Unplaced,
    /// The widget's at `path`, as `asker` asks: what it measures is kept under its path, and measured once.
    Widget { path: &'path KeyPath, asker: Asker },
}

/// A text a widget asks the box of: `runs` in `font` at `size` pixels to the em, its lines broken as `wrap` says.
#[derive(Debug, Clone, Copy)]
struct Asked<'runs> {
    font: Font,
    size: f32,
    runs: &'runs [Run],
    wrap: Wrap,
}

/// Why a text was not measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unmeasured {
    /// Its font's metrics have not come.
    Waiting,
    /// Its font's metrics will not come.
    Refused,
}
