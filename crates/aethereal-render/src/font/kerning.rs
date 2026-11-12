//! Pair kerning: how much closer, or further apart, a font sets two glyphs side by side. Read from `GPOS`'s pair
//! adjustments under its `kern` feature, or, for a font without them, from the older `kern` table. Every list is
//! sorted, each entry once, and searched by halves: a few comparisons a pair even in a font that kerns thousands, and a
//! shape that is written as it is held.

use super::MetricsMalformed;
use super::metrics::codec::{Reader, Writer};
use std::collections::{BTreeMap, BTreeSet};

/// The pairs a font kerns: lookups, each applied in turn, their adjustments summed. Each lookup is subtables, of
/// which the first that holds a pair adjusts it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Kerning {
    lookups: Vec<Vec<Pairs>>,
}

/// One subtable of pairs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pairs {
    /// Each pair listed, with its adjustment in font units, sorted by its left glyph then its right, each pair once:
    /// `GPOS` pair adjustment format 1, and `kern` format 0.
    Listed(Vec<Pair>),
    /// Pairs by the classes of their glyphs: `GPOS` pair adjustment format 2.
    Classes(Classes),
}

/// A pair kerned, and its adjustment in font units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pair {
    pub left: u16,
    pub right: u16,
    pub adjustment: i16,
}

/// Pairs kerned by class: a pair whose left glyph is covered is adjusted by the value its glyphs' classes meet at, zero
/// when either glyph is in no class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classes {
    /// The left glyphs the subtable covers, sorted, each once.
    covered: Vec<u16>,
    /// Each left glyph's class, those not listed in class 0, sorted by glyph, each once.
    left: Vec<Class>,
    /// Each right glyph's class, those not listed in class 0, sorted by glyph, each once.
    right: Vec<Class>,
    /// How many classes right glyphs have: the length of each left class's row.
    row: u16,
    /// The adjustment for each left class and right class, a row for each left class.
    adjustments: Vec<i16>,
}

/// A glyph's class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Class {
    pub glyph: u16,
    pub class: u16,
}

/// The byte written before a subtable of listed pairs.
const LISTED: u8 = 0;

/// The byte written before a subtable of pairs by class.
const CLASSED: u8 = 1;

impl Kerning {
    /// The kerning of a font from its `GPOS` lookups, if it has them, and else from its `kern` table.
    pub fn from_tables(positioned: Option<Vec<Vec<Pairs>>>, kerned: Option<Pairs>) -> Self {
        let lookups = match (positioned, kerned) {
            (Some(lookups), _) if !lookups.is_empty() => lookups,
            (_, Some(pairs)) => vec![vec![pairs]],
            _ => Vec::new(),
        };
        Self { lookups }
    }

    /// The adjustment between `left` and `right`, in font units.
    pub fn between(&self, left: u16, right: u16) -> i32 {
        self.lookups
            .iter()
            .filter_map(|lookup| lookup.iter().find_map(|pairs| pairs.adjust(left, right)))
            .map(i32::from)
            .sum()
    }

    /// Writes each lookup, each its subtables.
    pub(super) fn write(&self, writer: &mut Writer) {
        writer.count(self.lookups.len());
        for lookup in &self.lookups {
            writer.count(lookup.len());
            for pairs in lookup {
                pairs.write(writer);
            }
        }
    }

    /// Reads the lookups [`Self::write`] wrote.
    pub(super) fn read_from(reader: &mut Reader<'_>) -> Result<Self, MetricsMalformed> {
        // A lookup is at least its count of subtables, and a subtable at least its form and a count.
        let lookups = (0..reader.count(8)?)
            .map(|_| (0..reader.count(9)?).map(|_| Pairs::read_from(reader)).collect())
            .collect::<Result<Vec<Vec<Pairs>>, MetricsMalformed>>()?;

        Ok(Self { lookups })
    }
}

impl Pairs {
    /// The subtable of `pairs`, each with its adjustment.
    pub fn listed(pairs: BTreeMap<(u16, u16), i16>) -> Self {
        Self::Listed(pairs.into_iter().map(|((left, right), adjustment)| Pair { left, right, adjustment }).collect())
    }

    /// The adjustment between `left` and `right`, if this subtable holds the pair.
    fn adjust(&self, left: u16, right: u16) -> Option<i16> {
        match self {
            Self::Listed(pairs) => {
                let at = pairs.binary_search_by_key(&(left, right), |pair| (pair.left, pair.right)).ok()?;
                pairs.get(at).map(|pair| pair.adjustment)
            }
            Self::Classes(classes) => classes.adjust(left, right),
        }
    }

    /// Writes the subtable: its form, then its lists.
    fn write(&self, writer: &mut Writer) {
        match self {
            Self::Listed(pairs) => {
                writer.u8(LISTED);
                writer.count(pairs.len());
                for pair in pairs {
                    writer.u16(pair.left);
                    writer.u16(pair.right);
                    writer.i16(pair.adjustment);
                }
            }
            Self::Classes(classes) => {
                writer.u8(CLASSED);
                classes.write(writer);
            }
        }
    }

    /// Reads a subtable [`Self::write`] wrote, refusing pairs out of order or listed twice.
    fn read_from(reader: &mut Reader<'_>) -> Result<Self, MetricsMalformed> {
        match reader.u8()? {
            LISTED => {
                let pairs = (0..reader.count(6)?)
                    .map(|_| Ok(Pair { left: reader.u16()?, right: reader.u16()?, adjustment: reader.i16()? }))
                    .collect::<Result<Vec<_>, MetricsMalformed>>()?;
                let sorted =
                    pairs.is_sorted_by(|before, after| (before.left, before.right) < (after.left, after.right));
                if !sorted {
                    return Err(MetricsMalformed::Kerning);
                }
                Ok(Self::Listed(pairs))
            }
            CLASSED => Classes::read_from(reader).map(Self::Classes),
            _ => Err(MetricsMalformed::Kerning),
        }
    }
}

impl Classes {
    /// The subtable that covers `covered`, which kerns by the classes `left` and `right` give glyphs, and by
    /// `adjustments`, rows of `row`, one for each left class.
    pub fn new(
        covered: BTreeSet<u16>,
        left: BTreeMap<u16, u16>,
        right: BTreeMap<u16, u16>,
        row: u16,
        adjustments: Vec<i16>,
    ) -> Self {
        let classes = |map: BTreeMap<u16, u16>| map.into_iter().map(|(glyph, class)| Class { glyph, class }).collect();

        Self { covered: covered.into_iter().collect(), left: classes(left), right: classes(right), row, adjustments }
    }

    /// The adjustment between `left` and `right`, if `left` is covered.
    fn adjust(&self, left: u16, right: u16) -> Option<i16> {
        self.covered.binary_search(&left).ok()?;
        let row = usize::from(class_of(&self.left, left));
        let column = usize::from(class_of(&self.right, right));
        let index = row * usize::from(self.row) + column;
        Some(self.adjustments.get(index).copied().unwrap_or(0))
    }

    /// Writes the covered glyphs, the left glyphs' classes and the right's, the row's length, and the adjustments.
    fn write(&self, writer: &mut Writer) {
        writer.count(self.covered.len());
        for glyph in &self.covered {
            writer.u16(*glyph);
        }
        for classes in [&self.left, &self.right] {
            writer.count(classes.len());
            for class in classes {
                writer.u16(class.glyph);
                writer.u16(class.class);
            }
        }
        writer.u16(self.row);
        writer.count(self.adjustments.len());
        for adjustment in &self.adjustments {
            writer.i16(*adjustment);
        }
    }

    /// Reads a subtable [`Self::write`] wrote, refusing glyphs out of order or listed twice.
    fn read_from(reader: &mut Reader<'_>) -> Result<Self, MetricsMalformed> {
        let covered = (0..reader.count(2)?).map(|_| reader.u16()).collect::<Result<Vec<_>, _>>()?;
        let left = read_classes(reader)?;
        let right = read_classes(reader)?;
        let row = reader.u16()?;
        let adjustments = (0..reader.count(2)?).map(|_| reader.i16()).collect::<Result<Vec<_>, _>>()?;
        let sorted = covered.is_sorted_by(|before, after| before < after);
        if !sorted {
            return Err(MetricsMalformed::Kerning);
        }

        Ok(Self { covered, left, right, row, adjustments })
    }
}

/// The class `classes` gives `glyph`: 0 when it lists none.
fn class_of(classes: &[Class], glyph: u16) -> u16 {
    classes
        .binary_search_by_key(&glyph, |class| class.glyph)
        .ok()
        .and_then(|at| classes.get(at))
        .map_or(0, |class| class.class)
}

/// Reads a list of glyphs' classes, refusing glyphs out of order or listed twice.
fn read_classes(reader: &mut Reader<'_>) -> Result<Vec<Class>, MetricsMalformed> {
    let classes = (0..reader.count(4)?)
        .map(|_| Ok(Class { glyph: reader.u16()?, class: reader.u16()? }))
        .collect::<Result<Vec<_>, MetricsMalformed>>()?;
    let sorted = classes.is_sorted_by(|before, after| before.glyph < after.glyph);
    if !sorted {
        return Err(MetricsMalformed::Kerning);
    }

    Ok(classes)
}
