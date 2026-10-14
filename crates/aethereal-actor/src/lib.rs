//! What an actor's code is written against (ADR-0009).
//!
//! An actor asks the engine for things as effects, and says why a turn could
//! not go on as a fault. This crate holds what an
//! author names and nothing of the engine, so a guest, which is compiled apart
//! from the engine, depends on it alone. The engine depends on it too, since
//! it runs turns and applies their effects.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

mod actor;
mod boundary;
mod effect;
mod fault;
mod mail;
mod reference;
mod turn;

pub use actor::{Actor, Runs};
pub use boundary::{Actors, ActorsReader, ActorsWriter, Made, Refers, Table, Within, WithinReader, WithinWriter};
pub use effect::{Code, Effect, To, ToWithoutMail, What};
pub use fault::{Fault, Unreadable, Unwritable};
pub use mail::{Mail, MailId};
pub use reference::ErasedActorRef;
pub use turn::{Asked, Beginning, Ending, Refused, Stood, Turn, Why};
