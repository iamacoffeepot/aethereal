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
mod arena;
mod boundary;
mod effect;
mod ended;
mod fault;
mod item;
mod mail;
mod reference;
mod seat;
mod turn;

pub use actor::{Actor, Runs};
pub use arena::{ARENA_BYTES, Arena, Payload, Span};
pub use boundary::{Actors, ActorsReader, ActorsWriter, Made, Refers, Table, Within, WithinReader, WithinWriter};
pub use effect::{Code, Effect, Level, To, ToWithoutMail, What};
pub use ended::{Ended, How};
pub use fault::{Fault, Unreadable, Unwritable};
pub use item::Item;
pub use mail::{Mail, MailId};
pub use reference::ErasedActorRef;
pub use seat::{SeatNumber, Seated, Seats};
pub use turn::{Above, Asked, Beginning, Ending, Refused, Stood, Turn, Why};
