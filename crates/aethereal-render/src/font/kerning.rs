//! Pair kerning: how much closer, or further apart, a font sets two glyphs side by side. Read from `GPOS`'s pair
//! adjustments under its `kern` feature, or, for a font without them, from the older `kern` table.

use std::collections::{HashMap, HashSet};

/// The pairs a font kerns: lookups, each applied in turn, their adjustments summed. Each lookup is subtables, of
/// which the first that holds a pair adjusts it.
#[derive(Debug, Clone, Default)]
pub struct Kerning {
    lookups: Vec<Vec<Pairs>>,
}

/// One subtable of pairs.
#[derive(Debug, Clone)]
pub enum Pairs {
    /// Each pair listed, with its adjustment in font units: `GPOS` pair adjustment format 1, and `kern` format 0.
    Listed(HashMap<(u16, u16), i16>),
    /// Pairs by the classes of their glyphs: `GPOS` pair adjustment format 2.
    Classes(Classes),
}

/// Pairs kerned by class: a pair whose left glyph is covered is adjusted by the value its glyphs' classes meet at, zero
/// when either glyph is in no class.
#[derive(Debug, Clone)]
pub struct Classes {
    /// The left glyphs the subtable covers.
    pub covered: HashSet<u16>,
    /// Each left glyph's class, those not listed in class 0.
    pub left: HashMap<u16, u16>,
    /// Each right glyph's class, those not listed in class 0.
    pub right: HashMap<u16, u16>,
    /// How many classes right glyphs have: the length of each left class's row.
    pub row: u16,
    /// The adjustment for each left class and right class, a row for each left class.
    pub adjustments: Vec<i16>,
}

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
}

impl Pairs {
    /// The adjustment between `left` and `right`, if this subtable holds the pair.
    fn adjust(&self, left: u16, right: u16) -> Option<i16> {
        match self {
            Self::Listed(pairs) => pairs.get(&(left, right)).copied(),
            Self::Classes(classes) => classes.adjust(left, right),
        }
    }
}

impl Classes {
    /// The adjustment between `left` and `right`, if `left` is covered.
    fn adjust(&self, left: u16, right: u16) -> Option<i16> {
        if !self.covered.contains(&left) {
            return None;
        }
        let row = usize::from(self.left.get(&left).copied().unwrap_or(0));
        let column = usize::from(self.right.get(&right).copied().unwrap_or(0));
        let index = row * usize::from(self.row) + column;
        Some(self.adjustments.get(index).copied().unwrap_or(0))
    }
}
