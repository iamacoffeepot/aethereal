//! `maxp`: how many glyphs the font has.

use super::{Data, FontRefused};

/// Reads how many glyphs the font has: at least one, as glyph 0 is the missing glyph every font has.
pub fn read(data: Data<'_>) -> Result<u16, FontRefused> {
    let count = data.u16(4)?;
    if count == 0 {
        return Err(data.malformed());
    }

    Ok(count)
}
