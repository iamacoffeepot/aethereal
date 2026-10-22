//! A fault: why a turn could not go on (ADR-0009).

use aethereal_data::{Budget, DecodeError, EncodeError, Kind, KindId, Plain, from_bytes, to_bytes};
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

    /// The fault whose reason is of the kind `kind`, written as `bytes`: for a host, handing on a fault a guest
    /// returned (ADR-0015).
    #[must_use]
    pub const fn of_parts(kind: KindId, bytes: Vec<u8>) -> Self {
        Self { kind, bytes }
    }

    /// The bytes of the reason.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The fault whose reason is the kind `K`, which holds nothing and so has no bytes.
    pub(crate) fn bare<K: Kind + Plain>() -> Self {
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
#[kind(name = "aether.actor.unreadable")]
pub enum Unreadable {
    /// The bytes are not a value of the kind.
    Malformed,
    /// Reading them would take `requested` bytes at once, more than the `remaining` left of what the read may take:
    /// mail past the engine's limit, whose large parts belong in blobs.
    Limit { requested: u64, remaining: u64 },
}

impl From<DecodeError> for Fault {
    fn from(error: DecodeError) -> Self {
        let reason = match error {
            DecodeError::BudgetExceeded { requested, remaining } => Unreadable::Limit {
                requested: u64::try_from(requested).unwrap_or(u64::MAX),
                remaining: u64::try_from(remaining).unwrap_or(u64::MAX),
            },
            _ => Unreadable::Malformed,
        };
        // A reason that holds no collection always encodes.
        let bytes = to_bytes(&reason).unwrap_or_default();

        Self { kind: KindId::of::<Unreadable>(), bytes }
    }
}

/// The reason of a turn that wrote a value which holds a collection too long to encode (ADR-0009).
///
/// It is what `?` makes of an [`EncodeError`] in a handler.
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.actor.unwritable")]
pub struct Unwritable;

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
    #[kind(name = "aether.test.file_missing")]
    struct FileMissing {
        name: String,
    }

    /// The same layout as [`FileMissing`] under another name.
    #[derive(Debug, PartialEq, Plain)]
    #[kind(name = "aether.test.file_locked")]
    struct FileLocked {
        name: String,
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

    #[test]
    fn a_read_past_its_limit_faults_naming_what_it_asked_for_and_what_was_left() {
        // Catches the sizes of a read past the engine's limit dropped as `?` makes a fault of it, which leaves the log
        // of a turn that failed on mail too large to read saying only that it could not read it.
        let past = Fault::from(DecodeError::BudgetExceeded { requested: 7168, remaining: 1024 });
        let malformed = Fault::from(DecodeError::InvalidBoolean(2));
        let read = |fault: &Fault| fault.read::<Unreadable>(&mut Budget::new(64));

        assert_eq!(read(&past), Ok(Some(Unreadable::Limit { requested: 7168, remaining: 1024 })));
        assert_eq!(read(&malformed), Ok(Some(Unreadable::Malformed)));
    }
}
