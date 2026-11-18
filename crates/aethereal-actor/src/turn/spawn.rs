//! A spawn: a child stood up beneath the actor whose turn or `begin` it is, of a type that may stand there and that its
//! actor's type lists among what it spawns (ADR-0022, ADR-0059).

#[cfg(feature = "std")]
use super::Origin;
use super::{Beginning, Doing, Refused};
use crate::guard::Guard;
use crate::{Actor, ActorRef, Admits, ChildOf, ListsChild};

/// What spawns a child: a turn, or a `begin`, of an actor of type `Me`, the child's type found in `Me`'s `Spawns` at
/// `I`, its place, which the compiler infers (ADR-0059). A trait, so the place is inferred while the child's type is
/// named at the call, `turn.spawn::<Camera>(&(), config)`: bring it into scope with `use aethereal_actor::Spawn`.
///
/// Generic code that spawns a type an [`Each<L>`](crate::Each) lists names the place, [`Through`](crate::Through) the
/// path into `L` it carries: `Spawn::<Me, Through<P>>::spawn::<T>(turn, &(), config)`.
pub trait Spawn<Me: Actor, I> {
    /// Stands up a child of type `C` under `key`, running its `begin` here, and answers with a reference to it. It
    /// builds only when `C` may stand beneath this actor's type, as its parent says ([`Admits`]), and this actor's
    /// type lists `C` among what it spawns ([`ListsChild`]). `C` is inferred from where the reference goes, or named at
    /// the call, `turn.spawn::<Camera>(&(), config)` (ADR-0022, ADR-0059).
    ///
    /// The child takes its own mail. It is live once the engine applies this turn's effects.
    ///
    /// A type that spawns what stands beneath another does not build, though it lists it:
    ///
    /// ```compile_fail,E0277
    /// use aethereal_actor::{Actor, Anyone, Beginning, ChildOf, Ending, Fault, Handles, Spawn, Top, Turn};
    /// use aethereal_data::Plain;
    ///
    /// #[derive(Plain)]
    /// #[kind(name = "aether.doc.host")]
    /// struct Host;
    ///
    /// impl ChildOf for Host {
    ///     type Parent = Top;
    /// }
    ///
    /// // Stands beneath itself, not a `Host`.
    /// #[derive(Plain)]
    /// #[kind(name = "aether.doc.lens")]
    /// struct Lens;
    ///
    /// impl ChildOf for Lens {
    ///     type Parent = Lens;
    /// }
    ///
    /// impl Actor for Lens {
    ///     type Config = Lens;
    ///     type Key = ();
    ///     type Handles = ();
    ///     type Depends = ();
    ///     type Spawns = ();
    ///
    ///     fn begin(lens: Lens, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
    ///         Ok(lens)
    ///     }
    ///
    ///     fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
    ///         Ok(())
    ///     }
    /// }
    ///
    /// impl Actor for Host {
    ///     type Config = Host;
    ///     type Key = ();
    ///     type Handles = (Lens,);
    ///     type Depends = ();
    ///     type Spawns = (Lens,);
    ///
    ///     fn begin(host: Host, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
    ///         Ok(host)
    ///     }
    ///
    ///     fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
    ///         Ok(())
    ///     }
    /// }
    ///
    /// impl Handles<Lens> for Host {
    ///     type From = Anyone;
    ///
    ///     fn handle(&mut self, lens: Lens, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
    ///         let _ = turn.spawn::<Lens>(&(), lens);
    ///
    ///         Ok(())
    ///     }
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// Why the child was not stood up, as [`Refused`] says.
    fn spawn<C>(&mut self, key: &C::Key, config: C::Config) -> Result<ActorRef<C>, Refused>
    where
        C: ChildOf,
        C::Parent: Admits<Me, C>,
        Me::Spawns: ListsChild<Me, C, I>;

    /// Stands up a child of type `C` under `key` whose code is the blob `origin` names, running its `begin` here in the
    /// blob, and answers with a reference to it (ADR-0009, ADR-0054): [`Origin::Head`], the head the heads view shows
    /// for `C`'s kind as this turn's step began, or [`Origin::Blob`], a blob this actor holds. It builds as
    /// [`spawn`](Self::spawn) does.
    ///
    /// # Errors
    ///
    /// [`Refused::NoHead`] when `C`'s kind has no head, [`Refused::NotMade`] when the blob makes no actor of `C`'s
    /// type, and why the child was not stood up otherwise, as [`Refused`] says.
    #[cfg(feature = "std")]
    fn spawn_from<C>(&mut self, origin: Origin<'_>, key: &C::Key, config: &C::Config) -> Result<ActorRef<C>, Refused>
    where
        C: ChildOf,
        C::Parent: Admits<Me, C>,
        Me::Spawns: ListsChild<Me, C, I>;
}

impl<Me: Actor, F: Guard<Me>, I> Spawn<Me, I> for Doing<'_, Me, F> {
    fn spawn<C>(&mut self, key: &C::Key, config: C::Config) -> Result<ActorRef<C>, Refused>
    where
        C: ChildOf,
        C::Parent: Admits<Me, C>,
        Me::Spawns: ListsChild<Me, C, I>,
    {
        self.seat.spawn::<C>(self.instances, key, config)
    }

    #[cfg(feature = "std")]
    fn spawn_from<C>(&mut self, origin: Origin<'_>, key: &C::Key, config: &C::Config) -> Result<ActorRef<C>, Refused>
    where
        C: ChildOf,
        C::Parent: Admits<Me, C>,
        Me::Spawns: ListsChild<Me, C, I>,
    {
        self.seat.spawn_from::<C>(self.instances, origin, key, config)
    }
}

impl<Me: Actor, I> Spawn<Me, I> for Beginning<'_, Me> {
    fn spawn<C>(&mut self, key: &C::Key, config: C::Config) -> Result<ActorRef<C>, Refused>
    where
        C: ChildOf,
        C::Parent: Admits<Me, C>,
        Me::Spawns: ListsChild<Me, C, I>,
    {
        self.seat.spawn::<C>(self.instances, key, config)
    }

    #[cfg(feature = "std")]
    fn spawn_from<C>(&mut self, origin: Origin<'_>, key: &C::Key, config: &C::Config) -> Result<ActorRef<C>, Refused>
    where
        C: ChildOf,
        C::Parent: Admits<Me, C>,
        Me::Spawns: ListsChild<Me, C, I>,
    {
        self.seat.spawn_from::<C>(self.instances, origin, key, config)
    }
}
