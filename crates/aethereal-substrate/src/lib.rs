//! The substrate: the core of the engine, which holds what its actors share.
//!
//! An actor asks the engine for things as effects (ADR-0009). What many actors
//! read is a view, folded from the effects of finished turns (ADR-0011), and
//! the table of actors is the first view (ADR-0008).
//!
//! An id is computed here and nowhere else.

#![forbid(unsafe_code)]

mod effect;
mod id;
mod table;
mod view;

pub use effect::{Code, Effect, What};
pub use id::{child_id, path_id, root_id};
pub use table::{ActorTable, Standing};
pub use view::{Folded, View};
