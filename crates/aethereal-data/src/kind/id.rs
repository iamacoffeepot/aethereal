//! What a piece of mail is (ADR-0006).

use super::layout::{nodes_layout, shape_layout};
use super::{Kind, KindName};
use crate::{Fnv, Key, Plain, SchemaNodes};
use core::marker::PhantomData;

/// What a piece of mail is: 64 bits computed from a kind's name and layout (ADR-0006).
#[derive(Debug, Clone, Copy, Hash, Plain, Key)]
pub struct KindId(u64);

impl KindId {
    /// The id of `K`, computed when the program is built.
    ///
    /// ```rust
    /// use aethereal_data::{KindId, Plain};
    ///
    /// #[derive(Plain)]
    /// #[kind(name = "aether.audio.note_on")]
    /// struct NoteOn {
    ///     pitch: u8,
    ///     velocity: u8,
    /// }
    ///
    /// const ID: KindId = KindId::of::<NoteOn>();
    /// ```
    #[must_use]
    pub const fn of<K: Kind>() -> Self {
        const { Self::from_layout(Constant::<K>::NAME, shape_layout(&K::SHAPE)) }
    }

    /// The id of the kind a received name and schema describe.
    #[must_use]
    pub fn from_description(name: &KindName, schema: &SchemaNodes) -> Self {
        Self::from_layout(name, nodes_layout(schema))
    }

    /// The id's bytes as plain data writes them (ADR-0002), for a writer that runs in a constant: a guest's section of
    /// what it makes (ADR-0015). They say nothing encoding the id does not.
    #[must_use]
    pub const fn plain_bytes(self) -> [u8; 8] {
        self.0.to_le_bytes()
    }

    /// Whether it is `other`, compared where a constant is made: a declaration checked as the program is built.
    #[must_use]
    pub const fn same(self, other: Self) -> bool {
        self.0 == other.0
    }

    /// The id of a name and the layout hash of its schema; the only place an id is made.
    const fn from_layout(name: &KindName, layout: u64) -> Self {
        Self(Fnv::new().bytes(name.bytes()).hash(layout).finish())
    }
}

/// `K`'s name held by reference, so a constant can read it without dropping it.
struct Constant<K>(PhantomData<K>);

impl<K: Kind> Constant<K> {
    const NAME: &'static KindName = &K::NAME;
}

#[cfg(test)]
mod tests {
    use super::super::samples::{Command, Ledger, NoteOff, NoteOn, Path, Renamed, Reselected, Route, Wide, doubling};
    use super::*;
    use crate::{Schema, SchemaNode, to_bytes};

    /// An id's 64 bits, read back from its bytes; its value is private.
    fn bits(id: KindId) -> u64 {
        let Ok(bytes) = to_bytes(&id) else {
            panic!("an id encodes");
        };
        let Ok(eight) = <[u8; 8]>::try_from(bytes) else {
            panic!("an id is eight bytes");
        };

        u64::from_le_bytes(eight)
    }

    #[test]
    fn pinned_ids_match() {
        // Catches a changed name rule, tag, or fold order.
        // Tripwire: ids computed from ADR-0006's text by a separate program.
        const NOTE_ON: KindId = KindId::of::<NoteOn>();
        assert_eq!(bits(NOTE_ON), 0x9251_7d75_a231_5db8);
        assert_eq!(bits(KindId::of::<Path>()), 0xab80_51b7_e01c_dcfa);
        assert_eq!(bits(KindId::of::<Command>()), 0xfdcb_694d_1629_889f);
        assert_eq!(bits(KindId::of::<Ledger>()), 0xac5f_d9f5_f668_7cea);
    }

    #[test]
    fn deep_description_id_matches() {
        // Catches a flat walk that unfolds sharing: 64 levels unfold to 2^64 leaves and never return.
        // Tripwire: the id of a shared 64-level description, computed separately.
        let deep = KindId::from_description(&KindName::new("test.deep"), &doubling(64, "level", SchemaNode::Boolean));
        assert_eq!(bits(deep), 0x4309_90cc_9c7c_f8cb);
    }

    #[test]
    fn description_ids_equal_const_ids() {
        // Catches `of` and `from_description` disagreeing on one id.
        assert_eq!(KindId::from_description(&NoteOn::NAME, &SchemaNodes::of(&NoteOn::SHAPE)), KindId::of::<NoteOn>());
        assert_eq!(KindId::from_description(&Path::NAME, &SchemaNodes::of(&Path::SHAPE)), KindId::of::<Path>());
        assert_eq!(
            KindId::from_description(&Command::NAME, &SchemaNodes::of(&Command::SHAPE)),
            KindId::of::<Command>()
        );
        assert_eq!(KindId::from_description(&Ledger::NAME, &SchemaNodes::of(&Ledger::SHAPE)), KindId::of::<Ledger>());
    }

    #[test]
    fn renames_keep_the_id() {
        // Catches an id computed from names inside the schema.
        assert_eq!(KindId::of::<Route>(), KindId::of::<Path>());
        assert_eq!(KindId::of::<Renamed>(), KindId::of::<Command>());
    }

    #[test]
    fn type_selector_and_name_changes_change_the_id() {
        // Catches an id blind to a field type, a selector, or the kind name.
        assert_ne!(KindId::of::<Wide>(), KindId::of::<NoteOn>());
        assert_ne!(KindId::of::<Reselected>(), KindId::of::<Command>());
        assert_ne!(KindId::of::<NoteOff>(), KindId::of::<NoteOn>());
    }
}
