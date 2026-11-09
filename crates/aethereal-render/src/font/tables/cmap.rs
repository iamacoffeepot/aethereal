//! `cmap`: which glyph draws each character. Of the font's subtables, one Unicode subtable is read: a format 12 one,
//! which reaches every plane, before a format 4 one, which reaches only the basic multilingual plane.

use super::{Data, FontRefused, Table};

/// A font's characters and the glyphs that draw them, as runs of characters drawn by runs of glyphs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterMap {
    /// Sorted by their first character, none overlapping.
    groups: Vec<Group>,
}

/// A run of characters drawn by a run of glyphs: `start` by `glyph`, `start + 1` by `glyph + 1`, up to `end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Group {
    start: u32,
    end: u32,
    glyph: u32,
}

/// The last character Unicode has.
const LAST_CHARACTER: u32 = 0x10_FFFF;

impl CharacterMap {
    /// The glyph that draws `character`, if the font has one.
    pub fn glyph(&self, character: u32) -> Option<u16> {
        let group = self.groups.get(self.groups.partition_point(|group| group.end < character))?;
        let inside = group.start <= character;
        if !inside {
            return None;
        }
        u16::try_from(group.glyph + (character - group.start)).ok().filter(|&glyph| glyph != 0)
    }

    /// The map of `pairs` of a character and its glyph, sorted by character, the first pair for a character kept.
    fn from_pairs(mut pairs: Vec<(u32, u16)>) -> Self {
        pairs.sort_by_key(|&(character, _)| character);
        pairs.dedup_by_key(|&mut (character, _)| character);

        let mut groups: Vec<Group> = Vec::new();
        for (character, glyph) in pairs {
            let glyph = u32::from(glyph);
            match groups.last_mut() {
                Some(last) if last.continues(character, glyph) => last.end = character,
                _ => groups.push(Group { start: character, end: character, glyph }),
            }
        }

        Self { groups }
    }
}

impl Group {
    /// Whether `character` drawn by `glyph` follows on from this group's last.
    const fn continues(self, character: u32, glyph: u32) -> bool {
        let next_character = self.end + 1 == character;
        let next_glyph = self.glyph + (self.end - self.start) + 1 == glyph;
        next_character && next_glyph
    }
}

/// How a subtable is ranked: the lowest is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    /// Windows's full Unicode repertoire, format 12.
    WindowsFull,
    /// Unicode's own, format 12.
    UnicodeFull,
    /// Windows's basic multilingual plane, format 4.
    WindowsBasic,
    /// Unicode's own, format 4.
    UnicodeBasic,
}

/// Reads the font's best Unicode subtable, of a font of `glyph_count` glyphs. A character mapped to a glyph the font
/// has not is left out, so it draws the missing glyph.
pub fn read(data: Data<'_>, glyph_count: u16) -> Result<CharacterMap, FontRefused> {
    let count = data.offset16(2)?;
    let mut best: Vec<(Rank, Data<'_>)> = Vec::new();
    for index in 0..count {
        let at = data.at(4, index, 8)?;
        let platform = data.u16(at)?;
        let encoding = data.u16(at + 2)?;
        let subtable = data.from(data.offset32(at + 4)?)?;
        let rank = match (platform, encoding, subtable.u16(0)?) {
            (3, 10, 12) => Rank::WindowsFull,
            (0, _, 12) => Rank::UnicodeFull,
            (3, 1, 4) => Rank::WindowsBasic,
            (0, _, 4) => Rank::UnicodeBasic,
            _ => continue,
        };
        best.push((rank, subtable));
    }

    let unsupported = FontRefused::Unsupported(Table::Cmap);
    let (rank, subtable) = best.into_iter().min_by_key(|&(rank, _)| rank).ok_or(unsupported)?;
    match rank {
        Rank::WindowsFull | Rank::UnicodeFull => full(subtable, glyph_count),
        Rank::WindowsBasic | Rank::UnicodeBasic => basic(subtable, glyph_count),
    }
}

/// Reads a format 4 subtable: segments of characters, in order and apart, each mapped by a delta or through an array
/// of glyphs.
fn basic(data: Data<'_>, glyph_count: u16) -> Result<CharacterMap, FontRefused> {
    let segments = data.offset16(6)? / 2;
    let ends = 14;
    let starts = ends + segments * 2 + 2;
    let deltas = starts + segments * 2;
    let ranges = deltas + segments * 2;

    let mut pairs = Vec::new();
    let mut after = 0;
    for segment in 0..segments {
        let end = data.u16(ends + segment * 2)?;
        let start = data.u16(starts + segment * 2)?;
        let ordered = start <= end && u32::from(start) >= after;
        if !ordered {
            return Err(data.malformed());
        }
        after = u32::from(end) + 1;
        let delta = data.u16(deltas + segment * 2)?;
        let range_at = ranges + segment * 2;
        let range = data.offset16(range_at)?;
        for character in start..=end {
            let glyph = if range == 0 {
                character.wrapping_add(delta)
            } else {
                let at = range_at + range + usize::from(character - start) * 2;
                match data.u16(at)? {
                    0 => 0,
                    glyph => glyph.wrapping_add(delta),
                }
            };
            let drawn = glyph != 0 && glyph < glyph_count;
            if drawn {
                pairs.push((u32::from(character), glyph));
            }
        }
    }

    Ok(CharacterMap::from_pairs(pairs))
}

/// Reads a format 12 subtable: groups of characters, in order and apart, each drawn by a run of glyphs. Of a group
/// whose glyphs run past the font's, the characters drawn by glyphs in the font are kept.
fn full(data: Data<'_>, glyph_count: u16) -> Result<CharacterMap, FontRefused> {
    let count = data.offset32(12)?;
    let mut groups = Vec::new();
    let mut after = 0;
    for index in 0..count {
        let at = data.at(16, index, 12)?;
        let start = data.u32(at)?;
        let end = data.u32(at + 4)?;
        let glyph = data.u32(at + 8)?;
        let ordered = start <= end && end <= LAST_CHARACTER && start >= after;
        if !ordered {
            return Err(data.malformed());
        }
        after = end + 1;

        let glyphs_left = u32::from(glyph_count).saturating_sub(glyph);
        if glyphs_left > 0 {
            let end = end.min(start + (glyphs_left - 1));
            groups.push(Group { start, end, glyph });
        }
    }

    Ok(CharacterMap { groups })
}
