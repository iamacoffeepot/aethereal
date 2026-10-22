//! How a root stands itself up on a host: its settings, what it needs and provides, and the one actor it begins
//! (ADR-0029).

use super::{Host, Serves, Stage, Unbuilt};
use crate::Feed;
use aethereal_actor::{Actors, BlobRef, Controller, Given, Requests, Resource, Root, Source};
use aethereal_data::config::RootSettings;
use aethereal_data::{Encode, Kind};
use core::marker::PhantomData;

/// How a root bootstraps itself on the host `H` (ADR-0029).
///
/// The host's crate writes it for a root that needs the host; a library or the program writes it for a root that
/// needs no host, as `impl<H: Host> Bootstrap<H> for Scene`. An actor whose config is its settings, needing nothing
/// and providing nothing, derives it: `#[derive(Bootstrap)]` reads its settings from the section its kind name's last
/// segment names, and begins it as a singleton.
///
/// A bootstrap is a future, as a host whose setup awaits, the browser's, needs it to be: it opens its GPU there. A
/// host that does not wait, the desktop's or the headless one, bootstraps without awaiting anything, and refuses a
/// bootstrap that waits ([`ProgramError::Waited`](super::ProgramError::Waited)).
pub trait Bootstrap<H: Host>: Root {
    /// The section this root reads its settings from (ADR-0028).
    const SECTION: &'static str;
    /// Its settings: a settings kind, or `()` when it has none, which brings no section.
    type Settings: RootSettings;
    /// What it takes from roots bootstrapped before it: a tuple of types, each found among what they provided, and
    /// handed to it as a clone.
    type Needs;
    /// What it gives roots bootstrapped after it: a tuple of values, `()` for none.
    type Provides: 'static;

    /// Stands the root up from its settings and what it needs: begins it through `cx`, once for a singleton or once
    /// per key, and gives the host the ends it serves.
    ///
    /// # Errors
    ///
    /// Why it could not, which stops the program as it is set up.
    fn bootstrap(
        settings: Self::Settings,
        needs: Self::Needs,
        cx: &mut Bootstrapping<'_, H, Self>,
    ) -> impl Future<Output = Result<Self::Provides, Unbuilt>>;
}

/// What one root's bootstrap reaches: the engine, to begin that root alone and make what its config carries, and the
/// host's hosting, to give it ends (ADR-0029). It holds the stage the roots bootstrap on while this root does, and
/// gives it back once it has.
pub struct Bootstrapping<'host, H: Host, A> {
    stage: Stage<'host, H>,
    root: PhantomData<fn() -> A>,
}

impl<'host, H: Host, A: Root> Bootstrapping<'host, H, A> {
    /// What the root `A` bootstrapping on `stage` reaches.
    pub(super) const fn of(stage: Stage<'host, H>) -> Self {
        Self { stage, root: PhantomData }
    }

    /// The stage, given back once the root has bootstrapped.
    pub(super) fn into_stage(self) -> Stage<'host, H> {
        self.stage
    }

    /// Begins the root under `key`, from `config`, after the roots begun before it.
    ///
    /// # Errors
    ///
    /// [`Unbuilt::Refused`] when it was refused as it began, as [`Engine::add_root`](crate::Engine::add_root) says.
    pub fn begin(&mut self, key: &A::Key, config: A::Config) -> Result<(), Unbuilt> {
        self.stage.engine.add_root::<A>(key, config)?;

        Ok(())
    }

    /// Begins the root under `key`, from `config`, in the guest code `code` holds (ADR-0015).
    ///
    /// # Errors
    ///
    /// [`Unbuilt::Refused`] when it was refused as it began, as [`Engine::add_root_from`](crate::Engine::add_root_from)
    /// says.
    pub fn begin_from(&mut self, code: &BlobRef, key: &A::Key, config: &A::Config) -> Result<(), Unbuilt> {
        self.stage.engine.add_root_from::<A>(code, key, config)?;

        Ok(())
    }

    /// Hands `value` in, for the root's config to carry, as [`Engine::hand`](crate::Engine::hand) does.
    pub fn hand<R: Resource>(&mut self, value: R) -> Given<R> {
        self.stage.engine.hand(value)
    }

    /// Makes a source of `K`, for the root's config to carry, and the feed the host pushes onto, as
    /// [`Engine::source`](crate::Engine::source) does.
    pub fn source<K: Kind + Encode<Actors> + Send + 'static>(&mut self) -> (Given<Source<K>>, Feed<K>) {
        self.stage.engine.source()
    }

    /// Makes a controller of requests `K`, for the root's config to carry, and the end the host takes its requests
    /// from, as [`Engine::controller`](crate::Engine::controller) does.
    pub fn controller<K: Send + 'static>(&mut self) -> (Given<Controller<K>>, Requests<K>) {
        self.stage.engine.controller()
    }

    /// Puts `bytes` in the store as code, for the root to begin from, as [`Engine::put_code`](crate::Engine::put_code)
    /// does.
    ///
    /// # Errors
    ///
    /// [`Unbuilt::Code`] when the code was refused.
    pub fn put_code(&mut self, bytes: Vec<u8>) -> Result<BlobRef, Unbuilt> {
        Ok(self.stage.engine.put_code(bytes)?)
    }

    /// Puts `bytes` in the store, for the root's config to refer to, as [`Engine::put_blob`](crate::Engine::put_blob)
    /// does.
    ///
    /// # Errors
    ///
    /// [`Unbuilt::Refused`] when the blob was refused.
    pub fn put_blob(&mut self, bytes: Vec<u8>) -> Result<BlobRef, Unbuilt> {
        Ok(self.stage.engine.put_blob(bytes)?)
    }

    /// Gives the host `end`, the half of a pair that is the host's to keep: it builds only when the host serves it.
    pub fn serve<E>(&mut self, end: E)
    where
        H: Serves<E>,
    {
        H::serve(&mut self.stage.hosting, end);
    }

    /// The host's own setup, such as a screen to open windows on.
    pub const fn host(&mut self) -> &mut H::Hosting<'host> {
        &mut self.stage.hosting
    }
}
