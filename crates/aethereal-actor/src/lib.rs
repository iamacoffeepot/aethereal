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
mod ask;
mod blob;
mod boundary;
#[cfg(feature = "std")]
mod controller;
mod declared;
mod due;
mod effect;
mod ended;
mod fault;
#[cfg(feature = "guest")]
pub mod guest;
mod guests;
mod held;
mod instances;
mod item;
mod job;
mod log;
mod mail;
mod path;
mod placement;
pub mod records;
mod reference;
mod resource;
mod seat;
pub mod section;
#[cfg(feature = "std")]
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
mod written;

pub use actor::{Actor, Batch, Peeked, Preparing, Reads, Runs};
pub use arena::{ARENA_BYTES, Arena, CARRIED, Payload};
#[cfg(feature = "std")]
pub use arena::{PinSource, Pinned};
pub use ask::{Answer, Answered, Askable, Asking, Asks, Unanswered};
pub use blob::{BlobHash, BlobId, Maker, Puts};
#[cfg(feature = "std")]
pub use blob::{Bytes, Pinning, pins, within};
pub use boundary::{Actors, ActorsReader, ActorsWriter, Children, Refers, Table, Within, WithinReader, WithinWriter};
#[cfg(feature = "std")]
pub use controller::{Controller, Requests};
pub use declared::{
    Allows, AnsweredBy, At, CarriedBy, Carries, Conforms, Covers, Declared, DeclaredBy, Handled, HandledBy,
    HandledList, Handler, Handles, Kinds, Listed, Marker, Others, Protocol, RepliesTo, RepliesWith, RepliesWithin,
    ReplyWithin, Role, Sending, Sends, StateSchema, Target, Tells, ToAny, ToSender, ToType, Untaken,
};
pub use due::Due;
pub use effect::{BlobType, Code, Effect, Level, Line, Mailed, Seen, Tie, Tied, What};
pub use ended::{Ended, How};
pub use fault::{Fault, Unreadable, Unwritable};
pub use guests::{Birth, Guests, Link, Links, NoGuests, Unfit};
#[cfg(feature = "std")]
pub use guests::{Config, config_bytes};
#[cfg(feature = "std")]
pub use held::holders;
pub use held::{Held, NotOwed, Owed, Owes};
pub use instances::{Instances, Lent, Loadable, Memory};
pub use item::Item;
pub use job::{Copied, Driven, Fills, Job, JobCode, JobId, Runner, Running, Submitted, Unrun};
#[cfg(feature = "std")]
pub use job::{JobList, JobType, Lending, Outcome, Produced, lending};
pub use log::{ActorName, Entry, KeptLog, Logged, Named, Recent, RecentRing, Recorded};
pub use mail::{Mail, MailId};
pub use path::{
    Beneath, Child, HandlersNeedIn, HasRoot, Here, MadeAfter, NeedsIn, Ordered, Parent, Path, PathTo, Placed, There,
    TypeSegment, child, parent, path,
};
pub use placement::{Admits, Anywhere, AtTop, ChildOf, ParentOf, Parented, Placement, Recursive, Root, Spawner, Top};
pub use reference::{ActorRef, ErasedActorRef, ProtocolRef, Proven, Reachable, Reaches, Watchable};
pub use resource::{
    Carried, Claim, Claimed, Gift, Given, Giver, Handle, Holds, Keeping, Keeps, KeepsNothing, Kept, NotHeld, Resource,
    Was, carry, is_loose_claim, take_claimed, take_loose_ticket,
};
pub use seat::{SeatNumber, Seated, Seats};
#[cfg(feature = "std")]
pub use services::{Service, Services};
pub use sleep::{Never, Sleep, Slept, Timer, Until};
pub use source::Source;
pub use spares::Spares;
pub use stage::{
    Cycle, Cycled, Cycling, Drained, End, Follows, InCycle, On, Own, Place, Present, Render, Stage, StandsAfter, Start,
    Stop, Tick, Unshared,
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
pub use turn::{
    Above, Asked, Beginning, Broke, Buffers, Chain, Closing, Ending, FalseSend, FalseStage, FalseWatch, Forged,
    Garbled, Hosted, Leaves, Lives, Logger, Mailing, Refused, Request, Stood, Toward, Trapped, Turn, Turning,
    Unspawned, Vantage, Why,
};
pub use written::Written;
