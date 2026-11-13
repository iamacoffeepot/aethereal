//! What a screen measured of its texts, kept from one pass to the next (ADR-0056: Layout is Taffy; *settled by the
//! library's layout-cost stage*): each text by the key path of the widget that measured it, its runs, font, and size,
//! and what it measured as at each wrap it was asked at. Layout asks a leaf its size many times over, at each width its
//! parents try, and drawing asks again, so a text is measured once for each wrap it is asked at and never again while
//! its widget stands unchanged. A text unwrapped measures the same at any width, so its one measure answers every
//! width.
//!
//! What is kept is let go as it stops being asked. A text layout asked is kept while its widget lays out as it did, as
//! layout may ask it again whenever its parent lays out anew: the tree's comparison tells of a widget that lays out
//! otherwise, or that left the tree ([`Relaid`]). A text only drawing or input asked, as a text input measures what
//! its state holds, is kept while each pass asks it again. Every text kept is let go at another UI scale.

use super::Asked;
use crate::tree::{KeyPath, Relaid, Segment};
use aethereal_render::font::Metrics;
use aethereal_render::text::{Font, Measured, Run, Wrap, measure};
use std::collections::BTreeMap;

/// The most wraps one text keeps its measures at: as many as a resize offers it in a few passes, past which the oldest
/// is let go.
const MOST_WRAPS: usize = 8;

/// The texts a screen measured, by the key path of the widget that measured each.
#[derive(Debug, Default)]
pub struct Texts {
    by: BTreeMap<KeyPath, Vec<Measures>>,
    /// The UI scale what is kept was measured at.
    scale: f32,
    /// The number of the screen's pass now measuring, counted from its first.
    pass: u64,
}

/// What asks a text's box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asker {
    /// Layout, as it measures a leaf or asks a widget its room.
    Layout,
    /// Drawing, or input, as a widget lays out what it draws or finds where a press landed.
    Drawing,
}

/// One text a widget measured: `runs` in `font` at `size` pixels to the em, what it measured as at each wrap it was
/// asked at, the oldest first, and why it is kept.
#[derive(Debug)]
struct Measures {
    font: Font,
    size: f32,
    runs: Vec<Run>,
    wraps: Vec<(Wrap, Measured)>,
    held: Held,
}

/// Why a text is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Held {
    /// Layout asked it: it is kept while its widget lays out as it did.
    Laid,
    /// Only drawing or input asked it, last in the pass of this number: it is kept while each pass asks it.
    Drawn(u64),
}

impl Measures {
    /// Whether it is `runs` in `font` at `size`.
    fn is(&self, font: Font, size: f32, runs: &[Run]) -> bool {
        let font = self.font == font;
        let size = self.size.to_bits() == size.to_bits();
        let runs = self.runs == runs;

        font && size && runs
    }

    /// What it measured as at `wrap`, if it was measured at it.
    fn at(&self, wrap: Wrap) -> Option<Measured> {
        self.wraps.iter().find(|(kept, _)| same_wrap(*kept, wrap)).map(|(_, measured)| *measured)
    }

    /// Keeps that it measured as `measured` at `wrap`, letting the oldest wrap go past [`MOST_WRAPS`].
    fn keep(&mut self, wrap: Wrap, measured: Measured) {
        let full = self.wraps.len() == MOST_WRAPS;
        if full {
            self.wraps.remove(0);
        }
        self.wraps.push((wrap, measured));
    }
}

impl Texts {
    /// Begins the screen's next pass, at `scale` pixels to the logical pixel: every text kept is let go at another.
    pub fn begin(&mut self, scale: f32) {
        self.pass += 1;
        let rescaled = self.scale.to_bits() != scale.to_bits();
        if rescaled {
            self.by.clear();
            self.scale = scale;
        }
    }

    /// Lets go of each text only drawing or input asked that the pass ending did not ask again.
    pub fn sweep(&mut self) {
        let Self { by, pass, .. } = self;
        for kept in by.values_mut() {
            kept.retain(|entry| asked_since(entry.held, *pass));
        }
        by.retain(|_, kept| !kept.is_empty());
    }

    /// The box `asked` measures as, asked by `asker` for the widget at `path`: what was kept of it, or measured with
    /// the metrics `own` and `fallbacks` and kept.
    pub(super) fn measure(
        &mut self,
        path: &KeyPath,
        text: Asked<'_>,
        asker: Asker,
        own: &Metrics,
        fallbacks: &[Metrics],
    ) -> Measured {
        let Asked { font, size, runs, wrap } = text;
        let held = match asker {
            Asker::Layout => Held::Laid,
            Asker::Drawing => Held::Drawn(self.pass),
        };
        let met = self.by.contains_key(path);
        if !met {
            self.by.insert(path.clone(), Vec::new());
        }
        let Some(kept) = self.by.get_mut(path) else {
            return measure(own, fallbacks, size, runs, wrap);
        };
        let found = kept.iter().position(|entry| entry.is(font, size, runs));
        let at = found.unwrap_or_else(|| {
            kept.push(Measures { font, size, runs: runs.to_vec(), wraps: Vec::new(), held });
            kept.len() - 1
        });
        let entry = &mut kept[at];
        let laid = entry.held == Held::Laid;
        if !laid {
            entry.held = held;
        }
        if let Some(measured) = entry.at(wrap) {
            return measured;
        }
        let measured = measure(own, fallbacks, size, runs, wrap);
        entry.keep(wrap, measured);

        measured
    }

    /// Lets go of every text kept by the widget at `path` and those within it.
    fn within(&mut self, path: &[Segment]) {
        self.by.retain(|kept, _| !kept.segments().starts_with(path));
    }
}

/// Whether a text kept as `held` is kept past the pass numbered `pass`: one layout asked, or one asked in that pass.
const fn asked_since(held: Held, pass: u64) -> bool {
    match held {
        Held::Laid => true,
        Held::Drawn(drawn) => drawn == pass,
    }
}

/// A widget that lays out otherwise measures anew, and one that left the tree, or entered it where one stood in a tree
/// before the last, takes what it kept with it.
impl Relaid for Texts {
    fn changed(&mut self, path: &[Segment]) {
        self.by.remove(path);
    }

    fn entered(&mut self, path: &[Segment]) {
        self.within(path);
    }

    fn left(&mut self, path: &[Segment]) {
        self.within(path);
    }
}

/// Whether `one` and `other` break lines alike: both only at `\n`, or both at the same width, to the bit.
fn same_wrap(one: Wrap, other: Wrap) -> bool {
    match (one, other) {
        (Wrap::None, Wrap::None) => true,
        (Wrap::At { width: one }, Wrap::At { width: other }) => one.to_bits() == other.to_bits(),
        (Wrap::None, Wrap::At { .. }) | (Wrap::At { .. }, Wrap::None) => false,
    }
}
