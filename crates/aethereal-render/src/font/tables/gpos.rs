//! `GPOS`: glyph positioning, of which only pair adjustment (lookup type 2, formats 1 and 2) under the `kern` feature
//! is read, and of each pair only the first glyph's horizontal advance. The feature's lookups are taken for every script
//! and language alike, and a lookup of any other type is passed over.

use super::{Data, FontRefused};
use crate::font::kerning::{Classes, Pairs};
use std::collections::{BTreeMap, BTreeSet};

/// A lookup's type: pair adjustment.
const PAIR_ADJUSTMENT: u16 = 2;
/// A lookup's type: an extension, which holds a subtable of another type at a 32-bit offset.
const EXTENSION: u16 = 9;
/// A value record's field: the horizontal advance's adjustment.
const X_ADVANCE: u16 = 0x0004;
/// A value record's fields before the horizontal advance: the placement's adjustments.
const BEFORE_X_ADVANCE: u16 = 0x0003;

/// Reads the pair adjustments of the `kern` feature's lookups, in the order of the font's lookup list, each a list of
/// subtables.
pub fn read(data: Data<'_>) -> Result<Vec<Vec<Pairs>>, FontRefused> {
    let version = data.u16(0)?;
    if version != 1 {
        return Err(data.malformed());
    }
    let features = data.from(data.offset16(6)?)?;
    let lookups = data.from(data.offset16(8)?)?;

    let mut kerning = BTreeSet::new();
    for index in 0..features.offset16(0)? {
        let record = features.at(2, index, 6)?;
        let tag = features.array::<4>(record)?;
        if tag == *b"kern" {
            let feature = features.from(features.offset16(record + 4)?)?;
            for lookup in 0..feature.offset16(2)? {
                kerning.insert(feature.offset16(feature.at(4, lookup, 2)?)?);
            }
        }
    }

    kerning.into_iter().map(|index| lookup(lookups.from(lookups.offset16(lookups.at(2, index, 2)?)?)?)).collect()
}

/// Reads one lookup's pair adjustment subtables: none for a lookup of another type.
fn lookup(data: Data<'_>) -> Result<Vec<Pairs>, FontRefused> {
    let kind = data.u16(0)?;
    let mut subtables = Vec::new();
    for index in 0..data.offset16(4)? {
        let subtable = data.from(data.offset16(data.at(6, index, 2)?)?)?;
        let (kind, subtable) = match kind {
            EXTENSION => (subtable.u16(2)?, subtable.from(subtable.offset32(4)?)?),
            _ => (kind, subtable),
        };
        if kind == PAIR_ADJUSTMENT {
            subtables.push(pair_adjustment(subtable)?);
        }
    }

    Ok(subtables)
}

/// Reads a pair adjustment subtable, of format 1 or 2.
fn pair_adjustment(data: Data<'_>) -> Result<Pairs, FontRefused> {
    let covered = coverage(data.from(data.offset16(2)?)?)?;
    let first = ValueFormat(data.u16(4)?);
    let second = ValueFormat(data.u16(6)?);
    let record = first.bytes() + second.bytes();

    match data.u16(0)? {
        1 => {
            let sets = data.offset16(8)?;
            let mut pairs = BTreeMap::new();
            for (left, index) in covered {
                if index >= sets {
                    return Err(data.malformed());
                }
                let set = data.from(data.offset16(data.at(10, index, 2)?)?)?;
                for pair in 0..set.offset16(0)? {
                    let at = set.at(2, pair, record + 2)?;
                    let adjustment = first.x_advance(set, at + 2)?;
                    pairs.entry((left, set.u16(at)?)).or_insert(adjustment);
                }
            }
            Ok(Pairs::listed(pairs))
        }
        2 => {
            let left = classes(data.from(data.offset16(8)?)?)?;
            let right = classes(data.from(data.offset16(10)?)?)?;
            let left_classes = data.offset16(12)?;
            let row = data.u16(14)?;
            let cells = left_classes * usize::from(row);
            let fits = data.at(16, cells, record)? <= data.len();
            if !fits {
                return Err(data.malformed());
            }
            // A format with no horizontal advance adjusts no pair, and its cells, which may take no bytes, are not read.
            let adjustments = if first.has_x_advance() {
                (0..cells)
                    .map(|index| first.x_advance(data, data.at(16, index, record)?))
                    .collect::<Result<Vec<_>, FontRefused>>()?
            } else {
                Vec::new()
            };
            let covered = covered.into_iter().map(|(glyph, _)| glyph).collect::<BTreeSet<_>>();
            Ok(Pairs::Classes(Classes::new(covered, left, right, row, adjustments)))
        }
        _ => Err(data.malformed()),
    }
}

/// Which fields a value record holds, each two bytes.
#[derive(Debug, Clone, Copy)]
struct ValueFormat(u16);

impl ValueFormat {
    /// How many bytes a value record of this format takes.
    fn bytes(self) -> usize {
        self.0.count_ones() as usize * 2
    }

    /// Whether the format holds the horizontal advance's adjustment.
    const fn has_x_advance(self) -> bool {
        self.0 & X_ADVANCE != 0
    }

    /// The horizontal advance's adjustment of the value record at `at`: zero for a format without one.
    fn x_advance(self, data: Data<'_>, at: usize) -> Result<i16, FontRefused> {
        if !self.has_x_advance() {
            return Ok(0);
        }
        let before = (self.0 & BEFORE_X_ADVANCE).count_ones() as usize * 2;
        data.i16(at + before)
    }
}

/// The most glyphs a coverage or class table may list: every glyph a font numbers.
const MOST_GLYPHS: usize = 65_536;

/// Reads a coverage table: each glyph it covers, and its coverage index.
fn coverage(data: Data<'_>) -> Result<Vec<(u16, usize)>, FontRefused> {
    match data.u16(0)? {
        1 => (0..data.offset16(2)?).map(|index| Ok((data.u16(data.at(4, index, 2)?)?, index))).collect(),
        2 => {
            let mut covered = Vec::new();
            for range in 0..data.offset16(2)? {
                let at = data.at(4, range, 6)?;
                let (start, end, first) = (data.u16(at)?, data.u16(at + 2)?, data.offset16(at + 4)?);
                let fits = start <= end && covered.len() + usize::from(end - start) < MOST_GLYPHS;
                if !fits {
                    return Err(data.malformed());
                }
                covered.extend((start..=end).map(|glyph| (glyph, first + usize::from(glyph - start))));
            }
            Ok(covered)
        }
        _ => Err(data.malformed()),
    }
}

/// Reads a class definition table: each glyph's class, those not listed in class 0.
fn classes(data: Data<'_>) -> Result<BTreeMap<u16, u16>, FontRefused> {
    match data.u16(0)? {
        1 => {
            let refused = data.malformed();
            let start = data.u16(2)?;
            (0..data.u16(4)?)
                .map(|index| {
                    let glyph = start.checked_add(index).ok_or(refused)?;
                    Ok((glyph, data.u16(data.at(6, usize::from(index), 2)?)?))
                })
                .collect()
        }
        2 => {
            let mut classes = BTreeMap::new();
            let mut listed = 0;
            for range in 0..data.offset16(2)? {
                let at = data.at(4, range, 6)?;
                let (start, end, class) = (data.u16(at)?, data.u16(at + 2)?, data.u16(at + 4)?);
                let fits = start <= end && listed + usize::from(end - start) < MOST_GLYPHS;
                if !fits {
                    return Err(data.malformed());
                }
                listed += usize::from(end - start) + 1;
                classes.extend((start..=end).map(|glyph| (glyph, class)));
            }
            Ok(classes)
        }
        _ => Err(data.malformed()),
    }
}
