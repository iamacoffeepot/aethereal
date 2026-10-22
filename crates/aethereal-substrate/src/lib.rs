//! The substrate: the core of the engine, which holds what its actors share.
//!
//! An actor asks the engine for things as effects (ADR-0009). What many actors
//! read is a view, folded from the effects of finished turns (ADR-0011), and
//! the table of actors is the first view (ADR-0008).
//!
//! Turns run in steps (ADR-0011): the engine runs every actor that has mail,
//! then folds what their turns asked for and delivers their mail.
//!
//! An id is computed here and nowhere else.

#![forbid(unsafe_code)]

extern crate self as aethereal_substrate;

mod blob;
mod by_actor;
mod by_seat;
mod clock;
mod context;
mod engine;
mod id;
mod lives;
mod log;
mod mail;
mod program;
mod stages;
mod stream;
mod table;
mod view;
mod watch;

/// Derives [`Bootstrap`] for every host, for an actor whose config is its settings (ADR-0029).
pub use aethereal_data_derive::Bootstrap;
pub use blob::Blobs;
pub use by_actor::{ByActor, IdBits};
pub use by_seat::BySeat;
pub use clock::Clock;
pub use context::Context;
pub use engine::{
    Closed, CodeRefused, Done, Engine, Feed, Finish, Fold, Folding, Hops, Inline, Jobs, Lifecycle, NoRoom, NoRunner,
    Panicked, Plan, Reading, Setup, Share, Split, SplitCost, Taken, Tally, Waits, Whole, Workers,
};
pub use id::{child_id, path_id, root_id};
pub use lives::{Life, Lives, State};
pub use log::{Entry, Log, RECENT};
pub use mail::Envelope;
pub use program::{
    Bootstrap, Bootstrapping, Bootstraps, Builds, CODE, Composing, ENGINE, EngineSettings, Gathers, Host, Program,
    ProgramError, Provided, Provision, RUNNING, Reader, Serves, Settled, Stage, Unbuilt,
};
pub use table::ActorTable;
pub use view::{Column, Folded, Part, Routed, View, rounded};

/// Items the derives expand to; not part of the public interface.
#[doc(hidden)]
pub mod __derive {
    /// The actor whose config a derived `Bootstrap` reads as its settings.
    pub use aethereal_actor::Actor;
}
