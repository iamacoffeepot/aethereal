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
#[cfg(feature = "std")]
extern crate std;

mod actor;
mod arena;
mod blob;
mod boundary;
mod declared;
mod effect;
mod ended;
mod fault;
#[cfg(feature = "guest")]
pub mod guest;
mod guests;
mod instances;
mod item;
mod mail;
pub mod records;
mod reference;
mod seat;
pub mod section;
mod turn;

pub use actor::{Actor, Batch, Peeked, Preparing, Reads, Runs};
pub use arena::{ARENA_BYTES, Arena, CARRIED, Payload};
pub use blob::{BlobHash, BlobId, Puts};
#[cfg(feature = "std")]
pub use blob::{BlobRef, Held, pins, within};
pub use boundary::{Actors, ActorsReader, ActorsWriter, Children, Refers, Table, Within, WithinReader, WithinWriter};
pub use declared::{
    Allows, At, Declared, Handled, HandledList, Handler, Handles, Kinds, Marker, Others, Role, Sending, Sends, Target,
    ToAny, ToSender, ToType,
};
pub use effect::{BlobType, Code, Effect, Level, Line, Mailed, Seen, To, ToWithoutMail, What};
pub use ended::{Ended, How};
pub use fault::{Fault, Unreadable, Unwritable};
#[cfg(feature = "std")]
pub use guests::config_bytes;
pub use guests::{Birth, Guests, NoGuests, Unfit};
pub use instances::Instances;
pub use item::Item;
pub use mail::{Mail, MailId};
pub use reference::{ActorRef, ErasedActorRef};
pub use seat::{SeatNumber, Seated, Seats};
pub use turn::{
    Above, Address, AnyAddress, Asked, Beginning, Buffers, Chain, Closing, Ending, FalseSend, FalseWatch, Hosted,
    Logger, Refused, Stood, Trapped, Turn, Turning, Why,
};
