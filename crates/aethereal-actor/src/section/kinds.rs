//! A guest module's `aethereal.kinds` section: the id of every kind it was built with (ADR-0054).
//!
//! Each kind with no parameters built for a guest writes its id, 8 bytes, little-endian, into the section from a
//! static of its own, and the linker joins them in the order it takes its inputs: no count, no order, and a kind two
//! types declare is written twice. A host reads it as a set. It cannot share `aethereal.declared`, which is one counted
//! list, read to its end.

use super::Made;
use crate::Unfit;
use aethereal_data::{Budget, KindId, from_bytes};
use alloc::collections::BTreeSet;

/// The name of the section.
pub const NAME: &str = "aethereal.kinds";

/// How many bytes one id takes in the section.
const ID_BYTES: usize = 8;

/// The kinds a module was built with: each id its section `section` lists, with every kind what it makes, `made`,
/// names, which covers a generic kind it handles or answers, as no generic kind writes an id. Refused, as its module is
/// where it is put, when the section is not a whole number of ids.
///
/// # Errors
///
/// [`Unfit::Unlisted`] when `section` is not a whole number of ids.
pub fn read(section: &[u8], made: &[Made]) -> Result<BTreeSet<KindId>, Unfit> {
    let ids = section.chunks_exact(ID_BYTES);
    if !ids.remainder().is_empty() {
        return Err(Unfit::Unlisted);
    }
    let listed = ids.map(|id| from_bytes::<KindId>(id, &mut Budget::new(ID_BYTES)).map_err(|_| Unfit::Unlisted));

    listed.chain(made.iter().flat_map(Made::kinds).map(Ok)).collect()
}
