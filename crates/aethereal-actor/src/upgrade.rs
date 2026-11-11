//! A guest actor that moves onto its kind's new code: what it answers when it is asked, how it builds its state from
//! the one its old code left, and what the engine lends its host to ask it and move it (ADR-0054).

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
    /// Asked before each batch of its turns while the actor is behind its kind's head, and once, in a call of its own,
    /// when it has no mail as the head moves. It takes `&self`, so deciding changes nothing.
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
    /// Before this batch: it moves at once, and the batch runs on the new code.
    Now,
    /// Not yet: the batch runs on the old code, and the actor is asked again before its next.
    Later,
}

/// What the engine lends an actor's host to ask it whether to move onto its kind's head, and to move it, in the part
/// it sits in: on the worker that runs the part, before a batch of its turns, or between steps, for an actor with no
/// mail (ADR-0054).
pub struct Moving<'a> {
    /// The kind's head: the blob last published for it, as the heads view stood when the step began.
    #[cfg(feature = "std")]
    pub head: &'a crate::Bytes,
    /// The instances of the actor's part, the old code's and the new's among them (ADR-0015).
    pub instances: &'a mut Instances,
}

/// What became of an actor reached to move onto its kind's head (ADR-0054).
#[derive(Debug)]
pub enum Upgraded {
    /// It was not asked: its code is its kind's head, it is parked on that head, or its code is the program's own.
    Unasked,
    /// It answered [`Upgrade::Later`], and stays on its code.
    Later,
    /// It answered [`Upgrade::Now`] and moved: it runs on the head from now on, its type the one `declared` declares,
    /// and its path, watchers and references as they were. The old state had no field of each name in `absent`, which
    /// the new code worked out.
    Moved { declared: &'static Declared, absent: Vec<String> },
    /// It answered [`Upgrade::Now`] and did not move, for `reason`: it runs on its old code with the state it had,
    /// parked until its kind's head moves again. Each of `ended` ended as the new code's instance broke taking it
    /// over, as a failed turn ends its actor (ADR-0015): the other actors of that instance.
    Parked { reason: String, ended: Vec<(Seated, Fault)> },
    /// The call that asked it broke the instance of its own code, before it answered: it ends, and each other actor of
    /// that instance with it, each as `ended` says (ADR-0015).
    BrokeAsking { ended: Vec<(Seated, Fault)> },
    /// It answered [`Upgrade::Now`], and a call into the instance of its own code broke as it moved: it ends, and each
    /// other actor of that instance with it, and of the new code's when that broke too, each as `ended` says.
    BrokeMoving { ended: Vec<(Seated, Fault)> },
}

impl Upgraded {
    /// What the actor answered, when it was asked and answered.
    #[must_use]
    pub const fn answer(&self) -> Option<Upgrade> {
        match self {
            Self::Unasked | Self::BrokeAsking { .. } => None,
            Self::Later => Some(Upgrade::Later),
            Self::Moved { .. } | Self::Parked { .. } | Self::BrokeMoving { .. } => Some(Upgrade::Now),
        }
    }
}
