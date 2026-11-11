//! What an actor asks the file capability to list, and what it is answered (ADR-0032, ADR-0054).

use crate::Namespace;
use aethereal_data::{FilePath, Plain};

/// List the files of `namespace`: asked of [`Files`](crate::Files), and answered a [`FilesListed`].
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.files.list_files")]
pub struct ListFiles {
    /// The namespace to list.
    pub namespace: Namespace,
}

/// What became of a [`ListFiles`]: the files the namespace's listing holds, or why it was not listed. Each host answers
/// in it the same way.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.files.files_listed")]
pub enum FilesListed {
    /// The paths of the files its listing holds ([`Namespace::listing`]), sorted: in `Code`, its modules.
    Listed {
        /// The files' paths.
        paths: Vec<FilePath>,
    },
    /// The namespace is not listed, as `Assets` is not. Answered in the turn that asks, with no job run.
    NotListed,
    /// The namespace has nothing to list from: no directory natively, and no manifest in a browser.
    Missing,
    /// The host has nowhere to list it from.
    Unavailable,
    /// The host could not list it, or the listing was larger than one piece of mail may be, the engine's `read_limit`.
    Failed,
}
