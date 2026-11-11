//! A guest actor that moves onto its kind's new code: what it answers when it is asked, how it builds its state from
//! the one its old code left, and what the engine asks of its host between steps to move it (ADR-0054).

use crate::{Actor, Declared, Fault, Instances, Portability, Portable, Seated, TakeOverError};
use alloc::string::String;
use alloc::vec::Vec;

/// An actor type a guest's module makes, which says when it moves onto its kind's head and how it builds its state
/// from the state its old code left (ADR-0054).
///
/// `guest!` requires it of every type it lists, so a guest that does not say how
/// it moves does not build. A native actor does not implement it: a native kind's
/// code is the program's own, and moves only with a rebuild (ADR-0009).
///
/// ```
/// use aethereal_actor::{Portable, TakeOverError, Upgrade, Upgrades};
/// # use aethereal_actor::{Actor, Anywhere, Beginning, ChildOf, Ending, Fault, Portability};
/// # use aethereal_data::{Bound, Plain};
/// # #[derive(Plain)]
/// # #[kind(name = "doc.hud_config")]
/// # struct HudConfig;
/// # #[derive(Bound, Portability)]
/// # #[kind(name = "doc.hud")]
/// # struct Hud { health: f32, saving: bool, show_cooldowns: bool }
/// # impl ChildOf for Hud { type Parent = Anywhere; }
/// # impl Actor for Hud {
/// #     type Config = HudConfig;
/// #     type Key = ();
/// #     type Handles = ();
/// #     type BeginSends = ();
/// #     type EndSends = ();
/// #     fn begin(_: HudConfig, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
/// #         Ok(Self { health: 100.0, saving: false, show_cooldowns: false })
/// #     }
/// #     fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> { Ok(()) }
/// # }
///
/// impl Upgrades for Hud {
///     // A hud in the middle of a save waits for it to finish.
///     fn upgrade(&self) -> Upgrade {
///         if self.saving { Upgrade::Later } else { Upgrade::Now }
///     }
///
///     fn take_over(old: Portable<Self>) -> Result<Self, TakeOverError> {
///         let Some(health) = old.health else { return Err(TakeOverError::missing("health")) };
///
///         Ok(Self { health, saving: false, show_cooldowns: old.show_cooldowns.unwrap_or(false) })
///     }
/// }
/// ```
pub trait Upgrades: Actor + Portability {
    /// Asked before each of its turns while the actor is behind its kind's head, and once in a call with no turn when
    /// it has no mail as the head moves. It takes `&self`, so deciding changes nothing.
    fn upgrade(&self) -> Upgrade;

    /// Builds this version's state from the state the actor had, read by field name into its portable form. Called
    /// only when the old and new kinds' layouts differ: a state of the same kind is taken in as it is.
    ///
    /// # Errors
    ///
    /// Why it cannot: the actor then stays on its old code, parked until its kind's head moves again. Unless it is
    /// written, [`TakeOverError::Unwritten`].
    fn take_over(_old: Portable<Self>) -> Result<Self, TakeOverError> {
        Err(TakeOverError::Unwritten)
    }
}

/// What an actor behind its kind's head answers when it is asked whether to move (ADR-0054).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Upgrade {
    /// Before this turn: the batch ends before it, and the turn's mail waits for the new code.
    Now,
    /// Not yet: the turn runs on the old code, and the actor is asked again before its next.
    Later,
}

/// Whether an actor was asked whether to move onto its kind's head, and what it answered: what its host hands back of
/// each turn, and of a call with no turn that asks an actor with no mail, for the engine's record of what it ran
/// (ADR-0036, ADR-0054).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AskedToMove {
    /// It was not asked: its code is its kind's head, it is parked there, or its code is the program's own.
    #[default]
    Not,
    /// It was asked, and answered [`Upgrade::Later`]: a turn so asked ran on its old code.
    Later,
    /// It was asked, and answered [`Upgrade::Now`]. No turn runs after it on the old code, so only a call with no turn
    /// hands it back.
    Now,
}

impl From<Upgrade> for AskedToMove {
    fn from(answer: Upgrade) -> Self {
        match answer {
            Upgrade::Now => Self::Now,
            Upgrade::Later => Self::Later,
        }
    }
}

/// Why the engine reaches an actor whose code may be behind its kind's head, between steps (ADR-0054).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Behind {
    /// It answered [`Upgrade::Now`] before a turn, whose mail waits: it is moved.
    Answered,
    /// It had no mail as its kind's head moved: it is asked once, in a call with no turn, and moved when it answers
    /// [`Upgrade::Now`].
    Idle,
}

/// What the engine lends an actor's host to move it onto its kind's head between steps, in the part it sits in
/// (ADR-0054).
pub struct Moving<'a> {
    /// Why it is reached.
    pub behind: Behind,
    /// The kind's head: the blob last published for it, which the actor moves onto.
    #[cfg(feature = "std")]
    pub head: &'a crate::Bytes,
    /// The instances of the actor's part, the old code's and the new's among them (ADR-0015).
    pub instances: &'a mut Instances,
}

/// What became of an actor reached between steps to move onto its kind's head (ADR-0054).
#[derive(Debug)]
pub enum Upgraded {
    /// It stays on its code: it was not behind the head, was parked there, or answered [`Upgrade::Later`].
    Stays,
    /// It runs on the head from now on, its type the one `declared` declares, and its path, watchers and references
    /// as they were. The old state had no field of each name in `absent`, which the new code worked out.
    Moved { declared: &'static Declared, absent: Vec<String> },
    /// It did not move, for `reason`, and runs on its old code with the state it had, parked until its kind's head
    /// moves again. Each of `ended` ended as the new code's instance broke taking it in, as a failed turn ends its
    /// actor (ADR-0015): the actors of that instance but this one.
    Parked { reason: String, ended: Vec<(Seated, Fault)> },
    /// A call into its own code's instance broke, so nothing runs there again: it ends, and each other actor of that
    /// instance with it, each as `ended` says (ADR-0015).
    Broke { ended: Vec<(Seated, Fault)> },
}

/// What the engine's reach between steps found of an actor whose code may be behind its kind's head (ADR-0054).
#[derive(Debug)]
pub struct Reached {
    /// What it answered asked in a call with no turn, as an actor with no mail is: [`AskedToMove::Not`] when no such
    /// call asked it, as it was reached for the answer it gave before a turn, or was not behind its kind's head, or
    /// was parked there, or the call broke before it answered.
    pub asked: AskedToMove,
    /// What became of it.
    pub upgraded: Upgraded,
}

impl Reached {
    /// An actor no call with no turn asked, which stays on its code.
    #[must_use]
    pub const fn stays() -> Self {
        Self { asked: AskedToMove::Not, upgraded: Upgraded::Stays }
    }
}
