//! Spike: a guest module's `aethereal.kinds` section, the id of every kind compiled into it (ADR-0054).
//!
//! Each `#[kind]` without parameters built for a guest writes its id, 8 bytes, into the section from a static of its
//! own, and the linker concatenates them in the order it takes its inputs: no count, no order, and a kind two types
//! declare is written twice. A host reads it as a set.

use crate::Unfit;
use aethereal_data::{Budget, KindId, from_bytes};
use alloc::collections::BTreeSet;

/// The name of the section.
pub const NAME: &str = "aethereal.kinds";

/// How many bytes one id takes in the section.
const ID_BYTES: usize = 8;

/// The kinds the section `section` names, each once: refused, as its module is where it is put, when it is not a whole
/// number of ids.
///
/// # Errors
///
/// [`Unfit::Undeclared`] when its length is not a multiple of an id's.
pub fn read(section: &[u8]) -> Result<BTreeSet<KindId>, Unfit> {
    let ids = section.chunks_exact(ID_BYTES);
    if !ids.remainder().is_empty() {
        return Err(Unfit::Undeclared);
    }

    ids.map(|id| from_bytes::<KindId>(id, &mut Budget::new(ID_BYTES)).map_err(|_| Unfit::Undeclared)).collect()
}
