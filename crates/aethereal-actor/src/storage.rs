//! The host's storage, which a running job reads files from, as it waits on the host's timer (ADR-0032).

use aethereal_data::{FilePath, Plain};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::future::{self, Future};
use core::pin::Pin;

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

/// The host's storage, which a running job reads files from (ADR-0032): a directory for each namespace natively,
/// and fetches relative to the page in a browser. It is handed to every running job beside the host's timer.
///
/// A read never blocks the thread that polls it: a host whose reads block does them on threads of its own, and wakes
/// the read once its bytes are in.
pub trait Storage: Send + Sync {
    /// Reads the whole file at `path` in `namespace`.
    fn read<'a>(&'a self, namespace: Namespace, path: &'a FilePath) -> Reading<'a>;
}

/// The storage of a host with nowhere to read files from: every read is [`Unread::Unavailable`].
pub struct NoStorage;

impl Storage for NoStorage {
    fn read<'a>(&'a self, _: Namespace, _: &'a FilePath) -> Reading<'a> {
        Box::pin(future::ready(Err(Unread::Unavailable)))
    }
}
