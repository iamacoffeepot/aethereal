//! The modules a system ships for the kinds it owns, and what the library does with them (ADR-0054).

use aethereal_actor::Bytes;
use aethereal_data::{Bound, KindName, Plain};

/// Modules a system ships in its own code, `include_bytes!` put as blobs, for the kinds it owns: asked of
/// [`Library`](crate::Library), and answered a [`Defaulted`], as a toolbox offers its tools to their holder.
///
/// Each kind they make that has no head is published from them; a kind that has one keeps it, whether `Code` made it
/// as the program started or a publish made it since, so a default never replaces code. A library still publishing
/// `Code` as the program starts holds the ask until it has, so `Code` comes first. They are taken all or none.
#[derive(Debug, Bound)]
#[kind(name = "aether.library.defaults")]
pub struct Defaults {
    pub modules: Vec<Bytes>,
}

/// What became of [`Defaults`].
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.library.defaulted")]
pub enum Defaulted {
    /// Taken: of the kinds the modules make, how many were published from them, having no head, and how many kept
    /// the head they had.
    Taken { published: u32, kept: u32 },
    /// The module at this place in the list was refused by the guests' host, so none was taken. Why is in the
    /// library's log.
    Unfit { module: u32 },
    /// Two of the modules make this kind, so none was taken: neither can be chosen over the other.
    Twice { kind: KindName },
    /// The library stopped the program as it started, and holds no code.
    Stopped,
}
