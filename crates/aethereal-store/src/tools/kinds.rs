//! What the agent writes to the store's tools, and what it reads back (ADR-0039).
//!
//! A hash is a `[u8; 32]` here, which JSON writes as 64 lowercase hexadecimal digits (ADR-0007), the same digits the
//! store's host names the entry's file with. A size is a `u64`, which JSON writes as a string of its digits.

use crate::StoreName;
use aethereal_data::Plain;

/// The input of `check_in`: where the file is on the host's machine, and the name to point at it. The name is text
/// the tool checks, so a name that does not conform is refused with the rule it breaks.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.check_in_input")]
pub struct CheckInInput {
    pub path: String,
    pub name: String,
}

/// The output of `check_in`: the hash of the file's bytes, how many bytes it holds, and whether they were not already
/// in the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.checked_in_output")]
pub struct CheckedInOutput {
    pub hash: [u8; 32],
    pub bytes: u64,
    pub fresh: bool,
}

/// The input of `list_stored`: nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.list_stored_input")]
pub struct ListStoredInput;

/// The output of `list_stored`: every entry in the store, the last checked in first.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.stored_listed")]
pub struct StoredListed {
    pub entries: Vec<EntryListed>,
}

/// One entry as `list_stored` shows it: its hash, the names that point at it, how many bytes it holds, whether it is
/// pinned, and whether an actor holds it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.entry_listed")]
pub struct EntryListed {
    pub hash: [u8; 32],
    pub names: Vec<StoreName>,
    pub bytes: u64,
    pub pinned: bool,
    pub held: bool,
}

/// The input of `pin` and of `unpin`: the entry's hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.pin_input")]
pub struct PinInput {
    pub hash: [u8; 32],
}

/// The output of `pin` and of `unpin`: nothing more than that it was done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.pin_output")]
pub struct PinOutput {}
