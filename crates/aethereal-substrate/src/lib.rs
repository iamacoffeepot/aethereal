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

mod context;
mod engine;
mod id;
mod mail;
mod stream;
mod table;
mod view;
mod watch;

pub use context::Context;
pub use engine::Engine;
pub use id::{child_id, path_id, root_id};
pub use mail::{Envelope, Tie};
pub use table::{ActorTable, Standing};
pub use view::{Folded, View, ViewTuple, Viewed};
