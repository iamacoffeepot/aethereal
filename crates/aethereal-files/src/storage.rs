//! The host's storage, which the file capability's reads are done through: a service the host serves its jobs
//! (ADR-0032, ADR-0034).

use aethereal_data::{FilePath, Plain};
use std::future::Future;
use std::pin::Pin;

/// A namespace of files: a name the engine fixes, never a place (ADR-0032). Where each one's files are is the
/// host's, set by its settings, and an actor never learns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Namespace {
    /// The files the program ships with: read, never written.
    Assets,
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

/// A read of a file, as its storage does it: the file's bytes, or why it was not read.
pub type Reading<'a> = Pin<Box<dyn Future<Output = Result<Vec<u8>, Unread>> + 'a>>;

/// The host's storage, which the file capability's reads are done through (ADR-0032): a directory for each namespace
/// natively, and fetches relative to the page in a browser. A host serves it into the services its runners hand every
/// job (ADR-0034), and a host that serves none leaves every read unavailable.
///
/// A read never blocks the thread that polls it: a host whose reads block does them on threads of its own, and wakes
/// the read once its bytes are in.
pub trait Storage: Send + Sync {
    /// Reads the whole file at `path` in `namespace`.
    fn read<'a>(&'a self, namespace: Namespace, path: &'a FilePath) -> Reading<'a>;
}
