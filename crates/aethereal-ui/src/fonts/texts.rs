//! What a screen measured of its texts, kept from one pass to the next (ADR-0056: Layout is Taffy; *settled by the
//! library's layout-cost stage*): each text by the key path of the widget that measured it, its runs, font, and size,
//! and what it measured as for each wrap it was asked at. Layout asks a leaf its size many times over, at each width
//! its parents try, and every pass asks again, so a text is measured once for each wrap it is asked at and never again
//! while it stands unchanged. A text unwrapped measures the same at any width, so its one measure answers every width.
//!
//! What is kept is let go as it stops being asked: after a pass that laid out, whatever it did not ask, so a widget
//! whose key left the tree, or a text that changed, takes its measures with it; and after a pass that only drew, what
//! neither it nor the last layout asked, so a drawing that measures a new text each pass keeps no more than one pass's.

use super::Asked;
use crate::tree::KeyPath;
use aethereal_render::font::Metrics;
use aethereal_render::text::{Font, Measured, Run, Wrap, measure};
use std::collections::BTreeMap;

/// The texts a screen measured, by the key path of the widget that measured each.
#[derive(Debug, Default)]
pub struct Texts {
    by: BTreeMap<KeyPath, Vec<Measures>>,
    /// The number of the pass now measuring, counted from the screen's first.
    pass: u64,
    /// The number of the last pass that laid out: what it asked is kept through the passes that only draw after it.
    laid: u64,
}

/// What a pass of a screen does with its texts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    /// It lays the screen out, asking each leaf its size, and draws it.
    Laying,
    /// It draws the screen in the boxes the last layout gave it.
    Drawing,
}

/// One text a widget measured: `runs` in `font` at `size` pixels to the em, what it measured as at each wrap it was
/// asked at, the last pass that asked it, and the last pass that laid out and asked it.
#[derive(Debug)]
struct Measures {
    font: Font,
    size: f32,
    runs: Vec<Run>,
    wraps: Vec<(Wrap, Measured)>,
    seen: u64,
    laid: u64,
}

impl Measures {
    /// Whether it is `runs` in `font` at `size`.
    fn is(&self, font: Font, size: f32, runs: &[Run]) -> bool {
        let font = self.font == font;
        let size = self.size.to_bits() == size.to_bits();
        let runs = self.runs == runs;

        font && size && runs
    }
}

impl Texts {
    /// Begins the next pass, which does as `pass` says.
    pub fn begin(&mut self, pass: Pass) {
        self.pass += 1;
        if pass == Pass::Laying {
            self.laid = self.pass;
        }
    }

    /// Lets go of what the pass ending did not ask, and, after a pass that only drew, kept what the last layout asked.
    pub fn sweep(&mut self) {
        let Self { by, pass, laid } = self;
        for measures in by.values_mut() {
            measures.retain(|measures| measures.seen == *pass || measures.laid == *laid);
        }
        by.retain(|_, measures| !measures.is_empty());
    }

    /// The box `asked` measures as, measured for the widget at `path` with the metrics `own` and `fallbacks` unless it
    /// was measured before and kept.
    pub(super) fn measure(&mut self, path: &KeyPath, asked: Asked<'_>, own: &Metrics, fallbacks: &[Metrics]) -> Measured {
        let Self { by, pass, laid } = self;
        let Asked { font, size, runs, wrap } = asked;
        let met = by.contains_key(path);
        if !met {
            by.insert(path.clone(), Vec::new());
        }
        let Some(measures) = by.get_mut(path) else {
            return measure(own, fallbacks, size, runs, wrap);
        };
        let at = match measures.iter().position(|measures| measures.is(font, size, runs)) {
            Some(at) => at,
            None => {
                measures.push(Measures { font, size, runs: runs.to_vec(), wraps: Vec::new(), seen: 0, laid: 0 });
                measures.len() - 1
            }
        };
        let text = &mut measures[at];
        text.seen = *pass;
        if *pass == *laid {
            text.laid = *pass;
        }
        if let Some((_, measured)) = text.wraps.iter().find(|(kept, _)| same_wrap(*kept, wrap)) {
            return *measured;
        }
        let measured = measure(own, fallbacks, size, runs, wrap);
        text.wraps.push((wrap, measured));

        measured
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
