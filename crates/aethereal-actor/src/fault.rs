//! A fault: why a turn could not go on (ADR-0009).

use aethereal_data::{Budget, DecodeError, EncodeError, Kind, KindId, KindName, Plain, from_bytes, to_bytes};
use alloc::vec::Vec;

/// Why a turn could not go on: a reason the actor chose, as its kind's id and bytes (ADR-0009).
///
/// A reason is a kind of plain data. An actor defines its own and returns one,
/// and whoever is handed the fault reads it as a kind it knows. The id names
/// the reason to a reader that cannot decode it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    kind: KindId,
    bytes: Vec<u8>,
}

impl Fault {
    /// The fault whose reason is `reason`.
    ///
    /// Refused when `reason` holds a collection of more elements than a count can state.
    pub fn new<K: Kind + Plain>(reason: &K) -> Result<Self, EncodeError> {
        Ok(Self { kind: KindId::of::<K>(), bytes: to_bytes(reason)? })
    }

    /// The fault whose reason is the kind `K`, which holds nothing and so has no bytes.
    fn bare<K: Kind + Plain>() -> Self {
        Self { kind: KindId::of::<K>(), bytes: Vec::new() }
    }

    /// The id of the reason's kind.
    #[must_use]
    pub const fn kind(&self) -> KindId {
        self.kind
    }

    /// The reason, when it is of kind `K`, allocating no more than `budget` holds.
    ///
    /// A fault can arrive as bytes, from a guest, so bytes that do not decode as the kind they claim are an error
    /// and not a reason of another kind.
    pub fn read<K: Kind + Plain>(&self, budget: &mut Budget) -> Result<Option<K>, DecodeError> {
        let is_kind = self.kind == KindId::of::<K>();
        if is_kind {
            from_bytes(&self.bytes, budget).map(Some)
        } else {
            Ok(None)
        }
    }
}

/// The reason of a turn that read bytes which did not decode as the kind they claimed (ADR-0009).
///
/// It is what `?` makes of a [`DecodeError`] in a handler. A handler that is
/// to carry on past unreadable mail matches on the error.
#[derive(Debug, PartialEq, Eq, Plain)]
pub struct Unreadable;

impl Kind for Unreadable {
    const NAME: KindName = KindName::new("aether.actor.unreadable");
}

impl From<DecodeError> for Fault {
    fn from(_: DecodeError) -> Self {
        Self::bare::<Unreadable>()
    }
}

/// The reason of a turn that wrote a value which holds a collection too long to encode (ADR-0009).
///
/// It is what `?` makes of an [`EncodeError`] in a handler.
#[derive(Debug, PartialEq, Eq, Plain)]
pub struct Unwritable;

impl Kind for Unwritable {
    const NAME: KindName = KindName::new("aether.actor.unwritable");
}

impl From<EncodeError> for Fault {
    fn from(_: EncodeError) -> Self {
        Self::bare::<Unwritable>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;

    #[derive(Debug, PartialEq, Plain)]
    struct FileMissing {
        name: String,
    }

    impl Kind for FileMissing {
        const NAME: KindName = KindName::new("aether.test.file_missing");
    }

    /// The same layout as [`FileMissing`] under another name.
    #[derive(Debug, PartialEq, Plain)]
    struct FileLocked {
        name: String,
    }

    impl Kind for FileLocked {
        const NAME: KindName = KindName::new("aether.test.file_locked");
    }

    #[test]
    fn reason_reads_back_only_as_its_own_kind() {
        // Catches a read that decodes whatever bytes fit: the two kinds have one layout and differ in name alone.
        let Ok(fault) = Fault::new(&FileMissing { name: String::from("map.bin") }) else {
            panic!("a short name encodes");
        };
        let mut budget = Budget::new(64);

        assert_eq!(fault.read(&mut budget), Ok(Some(FileMissing { name: String::from("map.bin") })));
        assert_eq!(fault.read::<FileLocked>(&mut budget), Ok(None));
    }
}
