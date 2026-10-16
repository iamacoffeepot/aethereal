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

mod by_actor;
mod by_seat;
mod clock;
mod context;
mod engine;
mod id;
mod lives;
mod log;
mod mail;
mod stream;
mod table;
mod view;
mod watch;

pub use by_actor::{ByActor, IdBits};
pub use by_seat::BySeat;
pub use clock::Clock;
pub use context::Context;
pub use engine::{Done, Engine, Fold, Folding, Hops, Plan, Reading, Share, Split, SplitCost, Tally, Whole, Workers};
pub use id::{child_id, path_id, root_id};
pub use lives::{Life, Lives, State};
pub use log::{Entry, Log, RECENT};
pub use mail::Envelope;
pub use table::ActorTable;
pub use view::{Column, Consumes, Folded, Part, Routed, View, rounded};
