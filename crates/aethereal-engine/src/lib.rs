//! The engine's names: an actor's id, and the one table of actors (ADR-0008).
//!
//! An id is computed here and nowhere else. The table says which actors there
//! are and which have closed; every change to it is numbered, and a reader
//! that holds a number sees the table as it stood at that change.

#![forbid(unsafe_code)]

mod id;
mod table;

pub use id::{child_id, path_id, root_id};
pub use table::{ActorTable, Change, NotLive, Refused, Standing};
