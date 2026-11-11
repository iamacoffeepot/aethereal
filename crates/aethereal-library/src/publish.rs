//! What an actor asks the library to publish while the program runs, and what it is answered (ADR-0054).

use aethereal_actor::Bytes;
use aethereal_data::{Bound, FilePath, KindName, Plain};

/// Publish the module at `path` in `Code` as the head of every kind it makes, moving the heads of kinds another module
/// made before: asked of [`Library`](crate::Library), and answered a [`ModulePublished`].
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.library.publish_module")]
pub struct PublishModule {
    /// Where the module is in `Code`.
    pub path: FilePath,
}

/// What became of a [`PublishModule`].
#[derive(Debug, Bound)]
#[kind(name = "aether.library.module_published")]
pub enum ModulePublished {
    /// The module, as a blob, published as the head of each kind it makes. `kinds` holds each kind with the number the
    /// engine gives its publish, a `Vec<Numbered>` written as its bytes, in a blob: a module makes more kinds than one
    /// piece of mail holds (ADR-0042). The heads move as the step's effects are folded.
    Published { module: Bytes, kinds: Bytes },
    /// The module was not published, for this reason.
    Unloaded(Unloaded),
    /// The library is still publishing `Code` as the program starts, or stopped it as it started.
    Unopened,
}

/// A kind published, and the number the engine gives that publish: the head's (ADR-0054).
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.library.numbered")]
pub struct Numbered {
    pub kind: KindName,
    pub publish: u64,
}

/// Why a module was not published.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.library.unloaded")]
pub enum Unloaded {
    /// No file is at that path in `Code`.
    Missing,
    /// The host has nowhere to read `Code` from.
    Unavailable,
    /// The host could not read it, or `Files` did not answer.
    Unread,
    /// The guests' host refused it: it is no module this engine runs (ADR-0015). Why is in the library's log.
    Unfit,
    /// It makes no actor type, so no kind's head is its.
    MakesNothing,
}
