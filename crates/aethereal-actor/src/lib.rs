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
extern crate self as aethereal_actor;
#[cfg(feature = "std")]
extern crate std;

mod actor;
mod answering;
mod arena;
mod ask;
mod blob;
mod boundary;
mod buffer;
#[cfg(feature = "std")]
mod controller;
mod declared;
mod due;
mod effect;
mod ended;
mod fault;
pub mod guard;
#[cfg(feature = "guest")]
pub mod guest;
mod guests;
mod held;
mod instances;
mod item;
mod job;
mod log;
mod macros;
mod mail;
mod moves;
mod path;
mod placement;
mod portable;
mod publisher;
#[cfg(feature = "std")]
mod randomness;
pub mod records;
mod reference;
mod resource;
mod seat;
pub mod section;
mod services;
mod sleep;
mod source;
mod spares;
mod stage;
mod stats;
mod stream;
mod subscribers;
mod time;
#[cfg(feature = "std")]
pub mod tuning;
mod turn;
mod upgrade;
mod written;

pub use actor::{Actor, Batch, Peeked, Preparing, Reads, Runs};
pub use answering::{Answering, Answers, Later, Refusal, Reply};
pub use arena::{ARENA_BYTES, Arena, CARRIED, Payload};
#[cfg(feature = "std")]
pub use arena::{PinSource, Pinned};
pub use ask::{Answer, Answered, AnswersByType, Askable, AskedByType, Asking, Unanswered};
pub use blob::{BlobHash, BlobId, Maker, Puts};
#[cfg(feature = "std")]
pub use blob::{Bytes, Pinning, pins, within};
pub use boundary::{Actors, ActorsReader, ActorsWriter, Children, Refers, Table, Within, WithinReader, WithinWriter};
pub use buffer::{Buffer, BufferGroup, Pool};
#[cfg(feature = "std")]
pub use controller::{Controller, Requests};
pub use declared::{
    AnsweredAlike, At, CarriedBy, Carries, Conforms, Covers, Declared, Depends, DependsOn, Down, Each, Handled,
    HandledBy, HandledList, Handler, Handles, Kinds, Last, Listed, Lists, Many, One, Others, Pick, Protocol, Reached,
    Received, Receives, Role, RootIs, RootsHold, Spawns, StateSchema, Through, TypeList, Untaken, Whole,
};
pub use due::Due;
pub use effect::{BlobType, Code, Effect, Level, Line, Mailed, Seen, Tie, Tied, What};
pub use ended::{Ended, How};
pub use fault::{Fault, Unreadable, Unwritable};
pub use guard::{Admits, Anyone, Checks, Guard, Itself, Sender, SenderType, SenderTypes, Sent, SentBy, Speaks};
pub use guests::{Birth, Guests, Link, Links, NoGuests, Unfit};
#[cfg(feature = "std")]
pub use guests::{Config, config_bytes};
#[cfg(feature = "std")]
pub use held::holders;
pub use held::{Held, NotOwed, Owed, Owes};
pub use instances::{Instances, Lent, Loadable, Memory};
pub use item::Item;
pub use job::{Copied, Driven, Fills, Job, JobCode, JobId, Runner, Running, Submitted, Unrun, Working};
#[cfg(feature = "std")]
pub use job::{JobList, JobType, Lending, Outcome, Produced, lending};
pub use log::{ActorName, Entry, KeptLog, Logged, Named, Recent, RecentRing, Recorded};
pub use macros::{Guard, Sent, actor, protocol};
pub use mail::{Mail, MailId, SentAs};
pub use moves::{Behind, HeadMoves, Moves, Standing, Standings};
pub use path::{
    Beneath, Child, Fits, Follow, HasRoot, Here, HoldsRoot, MadeAfter, NeedsIn, Ordered, Parent, Path, PathTo, Placed,
    RootAbove, Rooted, Skip, There, TypeSegment, Walked, child, parent, path,
};
pub use placement::{
    Anywhere, AtTop, ChildOf, ListsChild, ParentOf, Parented, Placement, PlacesBeneath, Recursive, Root, Spawner, Top,
};
pub use portable::{OldFields, OldState, Portability, Portable, TakeOverError};
pub use publisher::Publisher;
#[cfg(feature = "std")]
pub use randomness::{Randomness, Unfilled};
pub use reference::{ActorRef, ErasedActorRef, ProtocolRef, Proven, Reachable, Reaches, Watchable};
pub use resource::{
    Carried, Claim, Claimed, Gift, Given, Giver, Handle, Holds, Keeping, Keeps, KeepsNothing, Kept, NotHeld, Resource,
    Resources, Was, carry, is_loose_claim, take_claimed, take_loose_ticket,
};
pub use seat::{SeatNumber, Seated, Seats};
pub use services::{Service, Services};
pub use sleep::{Never, Sleep, Slept, Timer, Until};
pub use source::Source;
pub use spares::Spares;
pub use stage::{
    Cycle, Cycled, Cycling, Drained, End, Follows, InCycle, On, Own, Place, Prepare, Present, Render, Stage,
    StandsAfter, Start, Stop, Tick, Unshared,
};
pub use stats::{Against, Backlog, EngineStats, Figures, GuestMemory, LARGEST, Process};
pub use stream::{
    Enough, More, Number, Open, Part, Serving, Stream, StreamKey, Streamed, Streaming, Streams, Taken, Unreachable,
    Unsent,
};
pub use subscribers::Subscribers;
pub use time::Time;
#[cfg(feature = "std")]
pub use tuning::{ReadTuning, Tunable, Tune, Tuned, Tunes, Tuning};
#[cfg(feature = "std")]
pub use turn::Origin;
pub use turn::{
    Above, Asked, Beginning, Broke, Buffers, Chain, Closing, Doing, Ending, FalseSend, FalseStage, FalseWatch, Forged,
    Garbled, Hosted, Leaves, Lives, Logger, Mailing, Refused, Request, Spawn, Stood, Toward, Trapped, Turn, Turning,
    Unspawned, Vantage, Why,
};
pub use upgrade::{Moving, Upgrade, Upgraded, Upgrades};
pub use written::Written;

/// Derives `Portability` for a struct: its portable form, a struct named `Portable` and the state's name, of its fields
/// under the same names, each an `Option` of the field's type, read by field name out of the state an earlier or a
/// later version of its code left (ADR-0054).
///
/// Only a struct's fields have names to be read by, so an enum is a compile error saying so.
///
/// ```compile_fail
/// use aethereal_actor::Portability;
///
/// #[derive(Portability)]
/// enum Mode {
///     Walk,
///     Run,
/// }
/// ```
pub use aethereal_data_derive::Portability;

/// Items the derives expand to; not part of the public interface.
#[doc(hidden)]
pub mod __derive {
    /// The codec a field of a portable form is read with.
    pub use aethereal_data::{Decode, Schema};
}
