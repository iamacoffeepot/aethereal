//! An engine being set up: the roots its host makes before it runs, checked as the program is built (ADR-0022).

use super::bring::Feed;
use super::jobs::Jobs;
use super::{CodeRefused, Engine};
use aethereal_actor::{
    Actors, BlobRef, Controller, Cycle, Given, Guests, Links, Ordered, Refused, Requests, Resource, Root, Source,
    Unordered,
};
use aethereal_data::{Encode, Kind};
use core::marker::PhantomData;
use core::time::Duration;
use std::sync::Arc;

/// An engine being set up, with the roots `S` made so far: a list, `()` for none and `(R, S)` for the root `R` made
/// after those of `S` (ADR-0022).
///
/// Its host makes the roots the program starts with here, in order, and is handed the engine by
/// [`ready`](Self::ready), which builds only when each root reaches at a typed path only roots made before it: a
/// root that sends at the path of one made later is caught by the compiler, as `cargo check` runs. Each root is also
/// checked as it is made, as every root is, which is what a root made once the engine runs is checked by.
///
/// ```
/// # use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Handles, Path, Tells, Top, Turn};
/// # use aethereal_data::{Kind, KindName, Plain};
/// # use aethereal_substrate::Engine;
/// # use std::num::NonZeroUsize;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.note")]
/// # struct Note;
/// #
/// # macro_rules! root {
/// #     ($name:ident, $named:literal, $sends:ty) => {
/// #         #[derive(Plain)]
/// #         struct $name;
/// #
/// #         impl Kind for $name {
/// #             const NAME: KindName = KindName::new($named);
/// #         }
/// #
/// #         impl ChildOf for $name {
/// #             type Parent = Top;
/// #         }
/// #
/// #         impl Actor for $name {
/// #             type Config = $name;
/// #             type Key = ();
/// #             type Handles = (Note,);
/// #             type BeginSends = $sends;
/// #             type EndSends = ();
/// #
/// #             fn begin(root: $name, _: &mut Beginning<'_, Self, $sends>) -> Result<Self, Fault> {
/// #                 Ok(root)
/// #             }
/// #
/// #             fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
/// #                 Ok(())
/// #             }
/// #         }
/// #
/// #         impl Handles<Note> for $name {
/// #             type Sends = ();
/// #
/// #             fn handle(&mut self, _: Note, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
/// #                 Ok(())
/// #             }
/// #         }
/// #     };
/// # }
/// # root!(Lighthouse, "aether.example.lighthouse", ());
/// # root!(Sailor, "aether.example.sailor", (Tells<Path<Lighthouse>, Note>,));
/// #
/// # fn main() -> Result<(), aethereal_actor::Refused> {
/// // The sailor reaches the lighthouse, made before it.
/// let engine = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0)
///     .add_root::<Lighthouse>(&(), Lighthouse)?
///     .add_root::<Sailor>(&(), Sailor)?
///     .ready();
/// # Ok(())
/// # }
/// ```
///
/// A setup whose root reaches one made after it does not build:
///
/// ```compile_fail
/// # use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Handles, Path, Tells, Top, Turn};
/// # use aethereal_data::{Kind, KindName, Plain};
/// # use aethereal_substrate::Engine;
/// # use std::num::NonZeroUsize;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.note")]
/// # struct Note;
/// #
/// # macro_rules! root {
/// #     ($name:ident, $named:literal, $sends:ty) => {
/// #         #[derive(Plain)]
/// #         struct $name;
/// #
/// #         impl Kind for $name {
/// #             const NAME: KindName = KindName::new($named);
/// #         }
/// #
/// #         impl ChildOf for $name {
/// #             type Parent = Top;
/// #         }
/// #
/// #         impl Actor for $name {
/// #             type Config = $name;
/// #             type Key = ();
/// #             type Handles = (Note,);
/// #             type BeginSends = $sends;
/// #             type EndSends = ();
/// #
/// #             fn begin(root: $name, _: &mut Beginning<'_, Self, $sends>) -> Result<Self, Fault> {
/// #                 Ok(root)
/// #             }
/// #
/// #             fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
/// #                 Ok(())
/// #             }
/// #         }
/// #
/// #         impl Handles<Note> for $name {
/// #             type Sends = ();
/// #
/// #             fn handle(&mut self, _: Note, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
/// #                 Ok(())
/// #             }
/// #         }
/// #     };
/// # }
/// # root!(Lighthouse, "aether.example.lighthouse", ());
/// # root!(Sailor, "aether.example.sailor", (Tells<Path<Lighthouse>, Note>,));
/// #
/// # fn main() -> Result<(), aethereal_actor::Refused> {
/// // The sailor reaches the lighthouse, made after it.
/// let engine = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0)
///     .add_root::<Sailor>(&(), Sailor)?
///     .add_root::<Lighthouse>(&(), Lighthouse)?
///     .ready();
/// # Ok(())
/// # }
/// ```
#[must_use]
pub struct Setup<S> {
    engine: Engine,
    made: PhantomData<fn() -> S>,
}

impl Setup<()> {
    /// The setup of `engine`, which has no actors.
    pub(super) const fn of(engine: Engine) -> Self {
        Self { engine, made: PhantomData }
    }

    /// The engine, running the guests' code `guests` holds (ADR-0015): set before any root is made.
    pub fn with_guests(self, guests: Arc<dyn Guests>) -> Self {
        Self::of(self.engine.with_guests(guests))
    }

    /// The engine, keeping the native types `links` by name, for a guest to spawn (ADR-0009): set before any root is
    /// made.
    pub fn with_links(self, links: Links) -> Self {
        Self::of(self.engine.with_links(links))
    }

    /// The engine, its jobs run by `jobs`, which with none set answers every job that no runner runs it (ADR-0018).
    pub fn with_jobs(self, jobs: Arc<dyn Jobs>) -> Self {
        Self::of(self.engine.with_jobs(jobs))
    }

    /// The engine, running the cycle `C` between `Start` and `Stop`: its stages in order, each a barrier (ADR-0020).
    /// With none set, the lifecycle runs `Start` and `Stop` alone.
    ///
    /// # Errors
    ///
    /// [`Unordered`] when the cycle holds `Start` or `Stop`, a stage twice, or the shared stages, `Tick`, `Render`,
    /// and `Present`, out of that order.
    pub fn with_cycle<C: Cycle>(self) -> Result<Self, Unordered> {
        Ok(Self::of(self.engine.with_cycle::<C>()?))
    }

    /// The engine, limiting each `Tick`'s delta to `limit`, a quarter of a second unless set (ADR-0020).
    pub fn with_delta_limit(self, limit: Duration) -> Self {
        Self::of(self.engine.with_delta_limit(limit))
    }
}

impl<S> Setup<S> {
    /// Makes a root of type `A` under `key`, from `config`, after the roots made so far, as
    /// [`Engine::add_root`] does: what `ready` checks it by is its place among them.
    ///
    /// # Errors
    ///
    /// Why the root was refused, as `Engine::add_root` says.
    pub fn add_root<A: Root>(mut self, key: &A::Key, config: A::Config) -> Result<Setup<(A, S)>, Refused> {
        self.engine.add_root::<A>(key, config)?;

        Ok(Setup { engine: self.engine, made: PhantomData })
    }

    /// Makes a root of type `A` under each key, from its config, in order, after the roots made so far: one place among
    /// them for them all, so roots of one instanced type may be made in a loop.
    ///
    /// # Errors
    ///
    /// Why the first root refused was, as `Engine::add_root` says; those before it were made.
    pub fn add_roots<A: Root>(
        mut self,
        roots: impl IntoIterator<Item = (A::Key, A::Config)>,
    ) -> Result<Setup<(A, S)>, Refused> {
        for (key, config) in roots {
            self.engine.add_root::<A>(&key, config)?;
        }

        Ok(Setup { engine: self.engine, made: PhantomData })
    }

    /// Makes a root of type `C` under `key` from the blob `code`, after the roots made so far, as
    /// [`Engine::add_root_from`] does.
    ///
    /// # Errors
    ///
    /// Why the root was refused, as `Engine::add_root_from` says.
    pub fn add_root_from<C: Root>(
        mut self,
        code: &BlobRef,
        key: &C::Key,
        config: &C::Config,
    ) -> Result<Setup<(C, S)>, Refused> {
        self.engine.add_root_from::<C>(code, key, config)?;

        Ok(Setup { engine: self.engine, made: PhantomData })
    }

    /// Makes a root of type `C` from the blob `code` under each key, from its config, in order, after the roots made
    /// so far: one place among them for them all.
    ///
    /// # Errors
    ///
    /// Why the first root refused was, as `Engine::add_root_from` says; those before it were made.
    pub fn add_roots_from<C: Root>(
        mut self,
        code: &BlobRef,
        roots: impl IntoIterator<Item = (C::Key, C::Config)>,
    ) -> Result<Setup<(C, S)>, Refused> {
        for (key, config) in roots {
            self.engine.add_root_from::<C>(code, &key, &config)?;
        }

        Ok(Setup { engine: self.engine, made: PhantomData })
    }

    /// Hands `value` in, for a root's config to carry, as [`Engine::hand`] does.
    pub fn hand<R: Resource>(&mut self, value: R) -> Given<R> {
        self.engine.hand(value)
    }

    /// Makes a source of `K`, for a root's config to carry, and the feed the chassis pushes onto, as
    /// [`Engine::source`] does.
    pub fn source<K: Kind + Encode<Actors> + Send + 'static>(&mut self) -> (Given<Source<K>>, Feed<K>) {
        self.engine.source()
    }

    /// Makes a controller of requests `K`, for a root's config to carry, and the end the host takes its requests
    /// from, as [`Engine::controller`] does.
    pub fn controller<K: Send + 'static>(&mut self) -> (Given<Controller<K>>, Requests<K>) {
        self.engine.controller()
    }

    /// Puts `bytes` in the store as code, for a root to be made from, as [`Engine::put_code`] does.
    ///
    /// # Errors
    ///
    /// Why the code was refused, as `Engine::put_code` says.
    pub fn put_code(&mut self, bytes: Vec<u8>) -> Result<BlobRef, CodeRefused> {
        self.engine.put_code(bytes)
    }

    /// Puts `bytes` in the store, for a root's config to refer to, as [`Engine::put_blob`] does.
    ///
    /// # Errors
    ///
    /// Why the blob was refused, as `Engine::put_blob` says.
    pub fn put_blob(&mut self, bytes: Vec<u8>) -> Result<BlobRef, Refused> {
        self.engine.put_blob(bytes)
    }

    /// The engine, its roots made, for its host to start: it builds only when each root reaches at a typed path of a
    /// singleton root only one made before it (ADR-0022).
    #[must_use]
    pub fn ready<I>(self) -> Engine
    where
        S: Ordered<I>,
    {
        self.engine
    }
}
