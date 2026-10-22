//! How a root stands itself up on a host: its settings, what it needs and provides, and the one actor it begins
//! (ADR-0029).

use super::{Host, Serves, Stage, Unbuilt};
use crate::{Engine, Feed};
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
    ) -> Result<Self::Provides, Unbuilt>;
}

/// What one root's bootstrap reaches: the engine, to begin that root alone and make what its config carries, and the
/// host's hosting, to give it ends (ADR-0029).
pub struct Bootstrapping<'stage, H: Host, A> {
    engine: &'stage mut Engine,
    hosting: &'stage mut H::Hosting,
    root: PhantomData<fn() -> A>,
}

impl<'stage, H: Host, A: Root> Bootstrapping<'stage, H, A> {
    /// What the root `A` bootstrapping on `stage` reaches.
    pub(super) fn of(stage: &'stage mut Stage<'_, H>) -> Self {
        let (engine, hosting) = stage.engine_and_hosting();
        Self { engine, hosting, root: PhantomData }
    }

    /// Begins the root under `key`, from `config`, after the roots begun before it.
    ///
    /// # Errors
    ///
    /// [`Unbuilt::Refused`] when it was refused as it began, as [`Engine::add_root`] says.
    pub fn begin(&mut self, key: &A::Key, config: A::Config) -> Result<(), Unbuilt> {
        self.engine.add_root::<A>(key, config)?;

        Ok(())
    }

    /// Begins the root under `key`, from `config`, in the guest code `code` holds (ADR-0015).
    ///
    /// # Errors
    ///
    /// [`Unbuilt::Refused`] when it was refused as it began, as [`Engine::add_root_from`] says.
    pub fn begin_from(&mut self, code: &BlobRef, key: &A::Key, config: &A::Config) -> Result<(), Unbuilt> {
        self.engine.add_root_from::<A>(code, key, config)?;

        Ok(())
    }

    /// Hands `value` in, for the root's config to carry, as [`Engine::hand`] does.
    pub fn hand<R: Resource>(&mut self, value: R) -> Given<R> {
        self.engine.hand(value)
    }

    /// Makes a source of `K`, for the root's config to carry, and the feed the host pushes onto, as
    /// [`Engine::source`] does.
    pub fn source<K: Kind + Encode<Actors> + Send + 'static>(&mut self) -> (Given<Source<K>>, Feed<K>) {
        self.engine.source()
    }

    /// Makes a controller of requests `K`, for the root's config to carry, and the end the host takes its requests
    /// from, as [`Engine::controller`] does.
    pub fn controller<K: Send + 'static>(&mut self) -> (Given<Controller<K>>, Requests<K>) {
        self.engine.controller()
    }

    /// Puts `bytes` in the store as code, for the root to begin from, as [`Engine::put_code`] does.
    ///
    /// # Errors
    ///
    /// [`Unbuilt::Code`] when the code was refused.
    pub fn put_code(&mut self, bytes: Vec<u8>) -> Result<BlobRef, Unbuilt> {
        Ok(self.engine.put_code(bytes)?)
    }

    /// Puts `bytes` in the store, for the root's config to refer to, as [`Engine::put_blob`] does.
    ///
    /// # Errors
    ///
    /// [`Unbuilt::Refused`] when the blob was refused.
    pub fn put_blob(&mut self, bytes: Vec<u8>) -> Result<BlobRef, Unbuilt> {
        Ok(self.engine.put_blob(bytes)?)
    }

    /// Gives the host `end`, the half of a pair that is the host's to keep: it builds only when the host serves it.
    pub fn serve<E>(&mut self, end: E)
    where
        H: Serves<E>,
    {
        H::serve(self.hosting, end);
    }

    /// The host's own setup, such as a screen to open windows on.
    pub const fn host(&mut self) -> &mut H::Hosting {
        self.hosting
    }
}
