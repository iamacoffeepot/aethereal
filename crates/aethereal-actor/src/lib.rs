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
mod resource;
mod seat;
pub mod section;
mod source;
mod subscribers;
mod turn;

pub use actor::{Actor, Batch, Peeked, Preparing, Reads, Runs};
#[cfg(feature = "std")]
pub use arena::Pinned;
pub use arena::{ARENA_BYTES, Arena, CARRIED, Payload};
pub use blob::{BlobHash, BlobId, Puts};
#[cfg(feature = "std")]
pub use blob::{BlobRef, Held, pins, within};
pub use boundary::{Actors, ActorsReader, ActorsWriter, Children, Refers, Table, Within, WithinReader, WithinWriter};
pub use declared::{
    Allows, AnsweredBy, At, BackTo, CarriedBy, Carries, Conforms, Covers, Declared, Handled, HandledBy, HandledList,
    Handler, Handles, Kinds, Listed, Marker, Others, Protocol, RepliesTo, RepliesWithin, ReplyWithin, Role, Sending,
    Sends, Target, ToAny, ToSender, ToType, Watches,
};
pub use effect::{BlobType, Code, Effect, Level, Line, Mailed, Seen, To, ToWithoutMail, What};
pub use ended::{Ended, How};
pub use fault::{Fault, Unreadable, Unwritable};
pub use guests::{Birth, Guests, Link, Links, NoGuests, Unfit};
#[cfg(feature = "std")]
pub use guests::{Config, config_bytes};
pub use instances::{Instances, Lent};
pub use item::Item;
pub use mail::{Mail, MailId};
pub use reference::{ActorRef, ErasedActorRef, ProtocolRef, Reaches};
pub use resource::{
    Carried, Claim, Claimed, Gift, Given, Giver, Handle, Holds, Keeping, Keeps, KeepsNothing, Kept, NotHeld, Resource,
    Was, carry, is_loose_claim, take_claimed, take_loose_ticket,
};
pub use seat::{SeatNumber, Seated, Seats};
pub use source::Source;
pub use subscribers::Subscribers;
pub use turn::{
    Above, Asked, Beginning, Broke, Buffers, Chain, Closing, Ending, FalseSend, FalseWatch, Forged, Garbled, Hosted,
    Logger, Refused, Stood, Trapped, Turn, Turning, Why,
};
