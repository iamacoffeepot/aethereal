//! Checking a binary in, and what became of it (ADR-0039).

use crate::{StoreHash, StoreName};
use aethereal_data::Plain;

/// Where a session's container sees its drop box: the one directory of the hub's machine it is given, which only it
/// writes, and the one place a check-in of its reads (ADR-0057).
pub const DROP_BOX: &str = "/session/drop";

/// Copy the file a session named `file` in its drop box into the store, and point `name` at it: asked of
/// [`Store`](crate::Store) for the session numbered `session`, and answered a [`CheckedIn`]. The file is read only
/// when it is a regular file of that name in that session's drop box: a link is not followed, so nothing the session
/// writes there reaches another file of the hub's machine (ADR-0057).
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.check_in_dropped")]
pub struct CheckInDropped {
    pub session: u32,
    pub file: DropName,
    pub name: StoreName,
}

/// The name of a file in a drop box: one plain name, never a path, so it names a file in that box and nothing beneath
/// or above it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.drop_name")]
pub struct DropName {
    name: String,
}

/// The most bytes of a drop box's file's name, as a file system takes one.
const MOST_DROP_NAME_BYTES: usize = 255;

impl DropName {
    /// The name `name` writes, when it is one plain name: not empty, not `.` or `..`, and holding no `/` and no NUL.
    #[must_use]
    pub fn read(name: &str) -> Option<Self> {
        let is_plain = Self::is_plain(name);

        is_plain.then(|| Self { name: String::from(name) })
    }

    /// The name of the file a session's container sees at `path`, when it is a file directly in its drop box,
    /// [`DROP_BOX`]: none for any other path.
    #[must_use]
    pub fn of_container_path(path: &str) -> Option<Self> {
        let name = path.strip_prefix(DROP_BOX)?.strip_prefix('/')?;

        Self::read(name)
    }

    /// Whether `name` is one plain name.
    fn is_plain(name: &str) -> bool {
        let is_sized = (1..=MOST_DROP_NAME_BYTES).contains(&name.len());
        let is_dots = matches!(name, "." | "..");
        let is_spelled = !name.contains(['/', '\0']);

        is_sized && !is_dots && is_spelled
    }

    /// The name, when it still is one plain name: one read back from bytes may hold anything, and is refused.
    #[must_use]
    pub fn as_plain(&self) -> Option<&str> {
        let is_plain = Self::is_plain(&self.name);

        is_plain.then_some(self.name.as_str())
    }
}

/// Copy the file at `path` on the host's machine into the store, and point `name` at it: asked of
/// [`Store`](crate::Store), and answered a [`CheckedIn`].
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.check_in")]
pub struct CheckIn {
    /// Where the file is on the host's machine: relative to where the program runs, or absolute.
    #[flag(position)]
    pub path: String,
    /// The name that points at it once it is in.
    #[flag(position)]
    pub name: StoreName,
}

/// What became of a [`CheckIn`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.checked_in")]
pub enum CheckedIn {
    /// The file is in the store, and its name points at it.
    Stored(StoredBinary),
    /// Nothing is at the path.
    Missing,
    /// What is at the path is not a regular file: a directory, a device, or a pipe.
    NotAFile,
    /// The file holds more than the store's `most_file_bytes`.
    TooLarge,
    /// The file does not fit within the store's `most_bytes`, even after pruning all it may.
    Full,
    /// The host could not read the file or write the store.
    Failed,
}

/// A binary in the store, as a check-in found it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.stored_binary")]
pub struct StoredBinary {
    /// The hash of its bytes.
    pub hash: StoreHash,
    /// How many bytes it holds.
    pub bytes: u64,
    /// Whether these bytes were not already in the store.
    pub fresh: bool,
}
