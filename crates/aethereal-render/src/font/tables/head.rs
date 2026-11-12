//! `head`: the font's units per em, and whether `loca` holds short or long offsets.

use super::{Data, FontRefused};
use crate::font::UNITS_PER_EM;

/// What is read of `head`.
pub struct Head {
    /// How many font units make one em, from 16 to 16384.
    pub units_per_em: u16,
    /// The form of `loca`'s offsets.
    pub offsets: LocaOffsets,
}

/// The form of `loca`'s offsets into `glyf`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocaOffsets {
    /// 16-bit, each half the offset.
    Short,
    /// 32-bit, each the offset.
    Long,
}

/// The number every `head` holds at byte 12, which a file that is not a font almost never does.
const MAGIC: u32 = 0x5F0F_3CF5;

/// Reads `head`.
pub fn read(data: Data<'_>) -> Result<Head, FontRefused> {
    let magic = data.u32(12)?;
    let units_per_em = data.u16(18)?;
    let sized = UNITS_PER_EM.contains(&units_per_em);
    if magic != MAGIC || !sized {
        return Err(data.malformed());
    }

    let offsets = match data.i16(50)? {
        0 => LocaOffsets::Short,
        1 => LocaOffsets::Long,
        _ => return Err(data.malformed()),
    };

    Ok(Head { units_per_em, offsets })
}
