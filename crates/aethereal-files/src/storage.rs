//! The host's storage, which the file capability's reads, writes, and listings are done through: a service the host
//! serves its jobs (ADR-0032, ADR-0034, ADR-0039, ADR-0054).

use aethereal_actor::Spares;
use aethereal_data::{FilePath, Plain};
use std::future::Future;
use std::pin::Pin;

/// A namespace of files: a name the engine fixes, never a place (ADR-0032). Where each one's files are is the
/// host's, set by its settings, and an actor never learns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Namespace {
    /// The files the program ships with: read, never written.
    Assets,
    /// What the program saves: read and written (ADR-0039).
    Save,
    /// The guest modules the program runs: read, never written, and listed, so they can be found (ADR-0054).
    Code,
}

impl Namespace {
    /// Whether its files are written: `Save`'s are, and `Assets`' and `Code`'s never.
    #[must_use]
    pub const fn is_written(self) -> bool {
        match self {
            Self::Assets | Self::Code => false,
            Self::Save => true,
        }
    }

    /// Whether its files are listed: `Code`'s are, so the program's modules can be found; `Assets`' and `Save`'s are
    /// not yet.
    #[must_use]
    pub const fn is_listed(self) -> bool {
        match self {
            Self::Code => true,
            Self::Assets | Self::Save => false,
        }
    }

    /// Of `paths`, files found in it, the ones its listing holds, sorted and each once: in `Code`, its modules, the
    /// files directly in it whose names end in `.wasm`; in a namespace that is not listed, none. Every host lists
    /// through it, so a namespace lists the same files on each.
    #[must_use]
    pub fn listing(self, paths: impl IntoIterator<Item = FilePath>) -> Vec<FilePath> {
        let mut listed: Vec<FilePath> = paths.into_iter().filter(|path| self.lists(path)).collect();
        listed.sort();
        listed.dedup();

        listed
    }

    /// Whether its listing holds the file at `path`.
    fn lists(self, path: &FilePath) -> bool {
        match self {
            Self::Code => {
                let is_direct = !path.as_str().contains('/');
                // A path is lower case, so its extension is spelled one way.
                let is_module = matches!(path.as_str().rsplit_once('.'), Some((_, "wasm")));

                is_direct && is_module
            }
            Self::Assets | Self::Save => false,
        }
    }
}

/// Why a file was not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unread {
    /// No file is at that path.
    Missing,
    /// The host has nowhere to read it from.
    Unavailable,
    /// The host could not read it.
    Failed,
}

/// Why a namespace was not listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unlisted {
    /// The namespace is not listed, as `Assets` is not.
    NotListed,
    /// The namespace has nothing to list from: no directory natively, and no manifest in a browser.
    Missing,
    /// The host has nowhere to list it from.
    Unavailable,
    /// The host could not list it.
    Failed,
}

/// Why a file was not written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unwritten {
    /// The namespace is not written, as `Assets` is not.
    ReadOnly,
    /// The host has nowhere to write it.
    Unavailable,
    /// The host could not write it.
    Failed,
}

/// What a read of a file comes to: the file's bytes, or why it was not read, and the buffers it was lent to read into
/// that it did not read into (ADR-0042).
#[derive(Debug)]
pub struct Read {
    /// The file's bytes, in a buffer it was lent when one fit them, or why it was not read.
    pub bytes: Result<Vec<u8>, Unread>,
    /// The buffers it was lent that it did not read into, for the job to give back.
    pub lent: Spares,
}

/// A read of a file, as its storage does it.
pub type Reading<'a> = Pin<Box<dyn Future<Output = Read> + 'a>>;

/// A listing of a namespace, as its storage does it: the paths of the files its listing holds, or why it was not
/// listed.
pub type Listing<'a> = Pin<Box<dyn Future<Output = Result<Vec<FilePath>, Unlisted>> + 'a>>;

/// A write of a file, as its storage does it: done, or why it was not written.
pub type Writing<'a> = Pin<Box<dyn Future<Output = Result<(), Unwritten>> + 'a>>;

/// The host's storage, which the file capability's reads, writes, and listings are done through (ADR-0032, ADR-0039,
/// ADR-0054): a directory for each namespace natively, and fetches relative to the page and the origin private file
/// system in a browser. A host serves it into the services its runners hand every job (ADR-0034), and a host that
/// serves none leaves every read, write, and listing unavailable.
///
/// A read, a write, or a listing never blocks the thread that polls it: a host whose work blocks does it on threads of
/// its own, and wakes each once it is done.
pub trait Storage: Send + Sync {
    /// Reads the whole file at `path` in `namespace` into a buffer taken from `lent`, the buffers its job was lent,
    /// with [`Spares::take`] once it knows the file's size, so the file lands in one of them when it fits by the rule
    /// a job's own buffers are handed out by, and in a new one otherwise; and answers with what is left of `lent`
    /// (ADR-0042).
    fn read<'a>(&'a self, namespace: Namespace, path: &'a FilePath, lent: Spares) -> Reading<'a>;

    /// Writes `bytes` as the whole file at `path` in `namespace`, making the directories its path names. A reader
    /// never sees half of it: it sees the file as it was before, or all of `bytes`. A namespace that is not written
    /// is refused [`Unwritten::ReadOnly`].
    fn write<'a>(&'a self, namespace: Namespace, path: &'a FilePath, bytes: Vec<u8>) -> Writing<'a>;

    /// Lists the files of `namespace` its listing holds ([`Namespace::listing`]): natively, of the files directly in
    /// its directory; in a browser, of the paths its [`Manifest`](crate::Manifest) names. A namespace that is not
    /// listed is refused [`Unlisted::NotListed`].
    fn list(&self, namespace: Namespace) -> Listing<'_>;
}
