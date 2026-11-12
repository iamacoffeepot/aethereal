//! `kern`: the older table of kerned pairs, of which format 0 subtables, horizontal and not cross-stream, are read.
//! Both its forms are read: Microsoft's, of 16-bit headers, and Apple's, of 32-bit ones.

use super::{Data, FontRefused};
use crate::font::kerning::Pairs;
use std::collections::BTreeMap;

/// Reads the pairs of every format 0 subtable that kerns horizontally, the first adjustment of a pair kept.
pub fn read(data: Data<'_>) -> Result<Pairs, FontRefused> {
    let apple = data.u16(0)? == 1;
    let (count, mut at) = if apple {
        (data.offset32(4)?, 8)
    } else {
        (data.offset16(2)?, 4)
    };

    let mut pairs = BTreeMap::new();
    for _ in 0..count {
        let coverage = data.u16(at + 4)?;
        let (length, format, header, kerns) = if apple {
            let kerns = coverage & 0xE000 == 0;
            (data.offset32(at)?, coverage & 0xFF, 8, kerns)
        } else {
            let horizontal = coverage & 0x1 != 0;
            let plain = coverage & 0x6 == 0;
            (data.offset16(at + 2)?, coverage >> 8, 6, horizontal && plain)
        };

        if format == 0 {
            let body = at + header;
            let count = data.offset16(body)?;
            if kerns {
                for index in 0..count {
                    let pair = data.at(body + 8, index, 6)?;
                    let left = data.u16(pair)?;
                    let right = data.u16(pair + 2)?;
                    pairs.entry((left, right)).or_insert(data.i16(pair + 4)?);
                }
            }
            // A format 0 subtable's 16-bit length overflows for one of more than 10920 pairs, so its pairs say where
            // it ends.
            at = data.at(body + 8, count, 6)?;
        } else {
            let refused = data.malformed();
            at = length.checked_add(at).filter(|&end| end > at).ok_or(refused)?;
        }
    }

    Ok(Pairs::listed(pairs))
}
