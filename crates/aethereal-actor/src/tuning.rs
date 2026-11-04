//! Tuning (ADR-0050): the values an actor takes while the program runs, one settings kind read whole and set whole by
//! mail any actor may send, apart from the settings the program is set up with (ADR-0028).

use crate::{Actor, ActorRef, Bytes, Carries, Fault, Handles, Protocol, ProtocolRef, ToSender, Turn};
use aethereal_data::config::Settings;
use aethereal_data::{Bound, Budget, Plain, from_bytes, to_bytes};
use alloc::string::String;

/// The most bytes decoding a tuning may take.
pub const MOST_TUNING_BYTES: usize = 64 * 1024;

/// An actor tuned while the program runs: its tuning, one settings kind, read whole and set whole. Each field's doc
/// comment is what an agent reads of it, and a range a number keeps to is said there and held by [`Tunable::tune`].
///
/// Its two handlers are a line each, as no impl of `Handles` may be written for every `Tunable`:
///
/// ```text
/// impl Handles<ReadTuning> for Painter {
///     type Sends = (ToSender<Tuning>,);
///     fn handle(&mut self, _: ReadTuning, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
///         tuning::read(self, turn)
///     }
/// }
///
/// impl Handles<Tune> for Painter {
///     type Sends = (ToSender<Tuned>,);
///     fn handle(&mut self, tune: Tune, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
///         tuning::tune(self, &tune, turn)
///     }
/// }
/// ```
pub trait Tunable: Actor + Handles<ReadTuning> + Handles<Tune> {
    /// What it is tuned by.
    type Tuning: Settings + Plain;

    /// Its tuning as it stands.
    fn tuning(&self) -> Self::Tuning;

    /// Takes `tuning` in place of what it held, or refuses it, saying why: a value past what it can draw.
    ///
    /// # Errors
    ///
    /// Why it was refused, for the agent to read.
    fn tune(&mut self, tuning: Self::Tuning) -> Result<(), String>;

    /// This actor as one tuned: `me.protocol()`, which builds only when it handles [`Tunes`] as the protocol carries
    /// it.
    fn tunes(me: ActorRef<Self>) -> ProtocolRef<Tunes>;
}

/// The protocol a tunable actor is read and tuned through: [`ReadTuning`] and [`Tune`].
pub struct Tunes;

impl Protocol for Tunes {
    type Kinds = (ReadTuning, Tune);
}

impl Carries<ReadTuning> for Tunes {
    type Answers = (Tuning,);
}

impl Carries<Tune> for Tunes {
    type Answers = (Tuned,);
}

/// Asks an actor for its tuning, answered with [`Tuning`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.tuning.read_tuning")]
pub struct ReadTuning;

/// An actor's tuning, as the bytes of its `Tuning`.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.tuning.tuning")]
pub struct Tuning {
    pub bytes: Bytes,
}

/// Asks an actor to take a whole tuning, the bytes of its `Tuning`, answered with [`Tuned`].
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.tuning.tune")]
pub struct Tune {
    pub bytes: Bytes,
}

/// What became of a [`Tune`].
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.tuning.tuned")]
pub enum Tuned {
    /// Taken: the tuning as it now stands.
    Tuned(Tuning),
    /// Refused by the actor, saying why.
    Refused(String),
    /// The bytes were not a tuning of the actor's.
    Unread,
}

/// Answers a [`ReadTuning`] with `actor`'s tuning as it stands.
///
/// # Errors
///
/// A fault when its bytes cannot be written or put.
pub fn read<A: Tunable>(actor: &A, turn: &mut Turn<'_, A, (ToSender<Tuning>,)>) -> Result<(), Fault> {
    let tuning = tuning_of(actor, turn)?;
    turn.reply(&tuning)?;

    Ok(())
}

/// Hands `actor` the tuning `tune` carries, and answers what it now stands at, or why it was not taken.
///
/// # Errors
///
/// A fault when its bytes cannot be written or put.
pub fn tune<A: Tunable>(actor: &mut A, tune: &Tune, turn: &mut Turn<'_, A, (ToSender<Tuned>,)>) -> Result<(), Fault> {
    let read = from_bytes::<A::Tuning>(turn.read(&tune.bytes), &mut Budget::new(MOST_TUNING_BYTES));
    let tuned = match read {
        Ok(tuning) => match actor.tune(tuning) {
            Ok(()) => Tuned::Tuned(tuning_of(actor, turn)?),
            Err(why) => Tuned::Refused(why),
        },
        Err(_) => Tuned::Unread,
    };
    turn.reply(&tuned)?;

    Ok(())
}

/// Why a tuning was not answered: the engine had no room to hold its bytes. The turn fails, and its ask is answered
/// with that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.tuning.unheld")]
pub struct Unheld;

/// `actor`'s tuning as it stands, put as its bytes.
fn tuning_of<A: Tunable, S>(actor: &A, turn: &mut Turn<'_, A, S>) -> Result<Tuning, Fault> {
    let Ok(bytes) = turn.put(to_bytes(&actor.tuning())?) else {
        return Err(Fault::new(&Unheld)?);
    };

    Ok(Tuning { bytes })
}
