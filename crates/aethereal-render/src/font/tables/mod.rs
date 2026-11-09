//! A TrueType font's tables, each read on its own: the directory that says where each is, then one module for each
//! table read, every read bounds-checked.

#[cfg(test)]
mod tests;

pub mod cmap;
pub mod glyf;
pub mod gpos;
pub mod head;
pub mod hhea;
pub mod kern;
pub mod maxp;

pub use cmap::CharacterMap;

use super::data::Data;
use super::{Container, FontRefused, Table};

/// The table directory: where each table of the font is in its bytes.
pub struct Directory<'a> {
    bytes: &'a [u8],
    records: Vec<Record>,
}

/// One table's place in the font's bytes.
struct Record {
    tag: [u8; 4],
    offset: usize,
    length: usize,
}

impl<'a> Directory<'a> {
    /// Reads the directory at the start of a font's bytes, refusing a font whose outlines are CFF, a variable font,
    /// and any container that is not a bare TrueType font.
    pub fn read(bytes: &'a [u8]) -> Result<Self, FontRefused> {
        let data = Data::new(bytes, Table::Directory);
        let refused = match &data.array::<4>(0)? {
            [0, 1, 0, 0] | b"true" => return Self::records(bytes, data),
            b"OTTO" => FontRefused::Cff,
            b"wOFF" => FontRefused::Container(Container::Woff),
            b"wOF2" => FontRefused::Container(Container::Woff2),
            b"ttcf" => FontRefused::Container(Container::Collection),
            _ => FontRefused::Container(Container::Unknown),
        };
        Err(refused)
    }

    /// Reads the records of a TrueType font's directory.
    fn records(bytes: &'a [u8], data: Data<'a>) -> Result<Self, FontRefused> {
        let count = data.offset16(4)?;
        let records = (0..count)
            .map(|index| {
                let at = data.at(12, index, 16)?;
                Ok(Record { tag: data.array(at)?, offset: data.offset32(at + 8)?, length: data.offset32(at + 12)? })
            })
            .collect::<Result<Vec<_>, FontRefused>>()?;

        let directory = Self { bytes, records };
        let cff = directory.has(*b"CFF ") || directory.has(*b"CFF2");
        if cff {
            return Err(FontRefused::Cff);
        }
        if directory.has(*b"fvar") {
            return Err(FontRefused::Variable);
        }

        Ok(directory)
    }

    /// The bytes of the table tagged `tag`, refused as `table` when it is missing or reaches past the font's end.
    pub fn table(&self, tag: [u8; 4], table: Table) -> Result<Data<'a>, FontRefused> {
        self.optional(tag, table)?.ok_or(FontRefused::Missing(table))
    }

    /// The bytes of the table tagged `tag`, if the font has it, refused as `table` when it reaches past the font's
    /// end.
    pub fn optional(&self, tag: [u8; 4], table: Table) -> Result<Option<Data<'a>>, FontRefused> {
        let Some(record) = self.records.iter().find(|record| record.tag == tag) else {
            return Ok(None);
        };
        Data::new(self.bytes, table).slice(record.offset, record.length).map(Some)
    }

    /// Whether the font has a table tagged `tag`.
    fn has(&self, tag: [u8; 4]) -> bool {
        self.records.iter().any(|record| record.tag == tag)
    }
}
