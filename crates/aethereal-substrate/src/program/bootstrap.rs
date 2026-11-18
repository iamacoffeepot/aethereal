//! How a root stands itself up on a host: its settings, what it needs and provides, and the one actor it begins
//! (ADR-0029).

use super::{Host, Serves, Stage, Unbuilt};
use crate::{Feed, table};
use aethereal_actor::{
    ActorRef, Actors, Bytes, ChildOf, Controller, Given, Path, PathTo, Publisher, Requests, Resource, Root, Rooted,
    SeatNumber, Source,
};
use aethereal_data::config::RootSettings;
use aethereal_data::{ActorId, Encode, Kind, KindId};
use core::marker::PhantomData;
use core::time::Duration;

/// How a root bootstraps itself on the host `H` (ADR-0029).
///
/// The host's crate writes it for a root that needs the host; a library or the program writes it for a root that
/// needs no host, as `impl<H: Host> Bootstrap<H> for Scene`. An actor whose config is its settings, needing nothing
/// and providing nothing, derives it: `#[derive(Bootstrap)]` reads its settings from the section its kind name's last
/// segment names, and begins it as a singleton.
///
/// A bootstrap is a future, as a host whose setup awaits, the browser's, needs it to be: it opens its GPU there. A
/// host that does not await, the desktop's or the headless one, blocks on it.
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

    /// The most bytes a turn reads into any one piece of mail: past it, the turn fails, which ends the actor.
    #[must_use]
    pub const fn read_limit(&self) -> usize {
        self.stage.engine.read_limit()
    }

    /// The most bytes the engine's blobs hold, which hold every byte that crosses between actors: past it, a put is
    /// refused (ADR-0014, ADR-0042).
    #[must_use]
    pub const fn blob_bytes(&self) -> usize {
        self.stage.engine.blob_bytes()
    }

    /// The most one `Tick`'s delta may be, as the engine limits it (ADR-0020, ADR-0045).
    #[must_use]
    pub const fn delta_limit(&self) -> Duration {
        self.stage.engine.delta_limit()
    }

    /// Begins the root under `key`, from `config`, after the roots begun before it, and answers with its reference,
    /// for the bootstrap to hand over in what it provides (ADR-0029).
    ///
    /// # Errors
    ///
    /// [`Unbuilt::Refused`] when it was refused as it began, as [`Engine::add_root`](crate::Engine::add_root) says.
    pub fn begin(&mut self, key: &A::Key, config: A::Config) -> Result<ActorRef<A>, Unbuilt> {
        Ok(self.stage.engine.begin_root::<A>(key, config)?)
    }

    /// Begins the root under `key`, from `config`, in the guest code `code` holds (ADR-0015), and answers with its
    /// reference, as [`Self::begin`] does.
    ///
    /// # Errors
    ///
    /// [`Unbuilt::Refused`] when it was refused as it began, as [`Engine::add_root_from`](crate::Engine::add_root_from)
    /// says.
    pub fn begin_from(&mut self, code: &Bytes, key: &A::Key, config: &A::Config) -> Result<ActorRef<A>, Unbuilt> {
        Ok(self.stage.engine.begin_root_from::<A>(code, key, config)?)
    }

    /// A reference to the live actor the typed path `path` names, proven of its type, as a turn's `found` answers: none
    /// when nothing live stands there, or an actor of another type does (ADR-0029). It is for what the root spawned in
    /// its `begin`, which stands in the engine's table once [`Self::begin`] has answered, as a `begin`'s effects are
    /// applied before the root is made: the window root's bootstrap finds each window's actor so.
    ///
    /// It builds only for a path beneath the root `A` this bootstrap makes:
    ///
    /// ```
    /// # use aethereal_actor::{Actor, ActorRef, Beginning, ChildOf, Ending, Fault, Top, path};
    /// # use aethereal_data::Plain;
    /// # use aethereal_substrate::{Bootstrapping, Host};
    /// #
    /// # macro_rules! actor {
    /// #     ($name:ident, $named:literal, $parent:ty, $key:ty) => {
    /// #         #[derive(Plain)]
    /// #         #[kind(name = $named)]
    /// #         struct $name;
    /// #
    /// #         impl ChildOf for $name {
    /// #             type Parent = $parent;
    /// #         }
    /// #
    /// #         impl Actor for $name {
    /// #             type Config = $name;
    /// #             type Key = $key;
    /// #             type Handles = ();
    /// #             type Depends = ();
    /// #
    /// #             fn begin(actor: $name, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
    /// #                 Ok(actor)
    /// #             }
    /// #
    /// #             fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
    /// #                 Ok(())
    /// #             }
    /// #         }
    /// #     };
    /// # }
    /// #
    /// # actor!(Windows, "example.windows", Top, ());
    /// # actor!(Window, "example.window", Windows, u64);
    /// # actor!(Renderer, "example.renderer", Top, ());
    /// // The window root's bootstrap finds the first window its root made.
    /// fn first<H: Host>(cx: &Bootstrapping<'_, H, Windows>) -> Option<ActorRef<Window>> {
    ///     cx.found(&path::<Window>(0))
    /// }
    /// ```
    ///
    /// A path beneath another root does not build, as that root's bootstrap hands over what it made:
    ///
    /// ```compile_fail
    /// # use aethereal_actor::{Actor, ActorRef, Beginning, ChildOf, Ending, Fault, Top, path};
    /// # use aethereal_data::Plain;
    /// # use aethereal_substrate::{Bootstrapping, Host};
    /// #
    /// # macro_rules! actor {
    /// #     ($name:ident, $named:literal, $parent:ty, $key:ty) => {
    /// #         #[derive(Plain)]
    /// #         #[kind(name = $named)]
    /// #         struct $name;
    /// #
    /// #         impl ChildOf for $name {
    /// #             type Parent = $parent;
    /// #         }
    /// #
    /// #         impl Actor for $name {
    /// #             type Config = $name;
    /// #             type Key = $key;
    /// #             type Handles = ();
    /// #             type Depends = ();
    /// #
    /// #             fn begin(actor: $name, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
    /// #                 Ok(actor)
    /// #             }
    /// #
    /// #             fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
    /// #                 Ok(())
    /// #             }
    /// #         }
    /// #     };
    /// # }
    /// #
    /// # actor!(Windows, "example.windows", Top, ());
    /// # actor!(Window, "example.window", Windows, u64);
    /// # actor!(Renderer, "example.renderer", Top, ());
    /// // The renderer's bootstrap looks for a window the window root made.
    /// fn first<H: Host>(cx: &Bootstrapping<'_, H, Renderer>) -> Option<ActorRef<Window>> {
    ///     cx.found(&path::<Window>(0))
    /// }
    /// ```
    #[must_use]
    pub fn found<L>(&self, path: &Path<L>) -> Option<ActorRef<L>>
    where
        L: ChildOf + PathTo + Rooted,
        L::Root: FoundBy<L, A>,
    {
        let reached = table::reach(self.stage.engine.table(), path.names(), KindId::of::<L>());

        (reached.seat != SeatNumber::NOWHERE).then_some(ActorRef::found(reached))
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

    /// The program's one publisher, for the root's config to carry: the right to say which blob each kind is, which the
    /// root that holds a program's code takes (ADR-0054). Any root's bootstrap may ask, and the first to ask has it, as
    /// [`Engine::publisher`](crate::Engine::publisher) hands it out once.
    ///
    /// # Errors
    ///
    /// [`Unbuilt::SecondPublisher`] when a root bootstrapped earlier was handed it.
    pub fn publisher(&mut self) -> Result<Given<Publisher>, Unbuilt> {
        Ok(self.stage.engine.publisher()?)
    }

    /// Puts `bytes` in the store as code, for the root to begin from, as [`Engine::put_code`](crate::Engine::put_code)
    /// does.
    ///
    /// # Errors
    ///
    /// [`Unbuilt::Code`] when the code was refused.
    pub fn put_code(&mut self, bytes: Vec<u8>) -> Result<Bytes, Unbuilt> {
        Ok(self.stage.engine.put_code(bytes)?)
    }

    /// Puts `bytes` in the store, for the root's config to refer to, as [`Engine::put_blob`](crate::Engine::put_blob)
    /// does.
    ///
    /// # Errors
    ///
    /// [`Unbuilt::Refused`] when the blob was refused.
    pub fn put_blob(&mut self, bytes: Vec<u8>) -> Result<Bytes, Unbuilt> {
        Ok(self.stage.engine.put_blob(bytes)?)
    }

    /// The program's root list, for the renderer's config to carry: the id of each root it draws, in draw order
    /// (ADR-0055). Each is a root of the program, as its list's build checks; whether it draws is its own business.
    #[must_use]
    pub fn root_list(&self) -> Vec<ActorId> {
        self.stage.root_list.iter().map(|drawn| drawn.id()).collect()
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

/// This root, of the typed path to `L`, is `A`, the root a bootstrap of `A` makes: what the path a bootstrap finds is
/// checked by, as it finds only what its own root made (ADR-0029).
#[diagnostic::on_unimplemented(
    message = "a bootstrap of `{A}` finds `{L}` beneath `{Self}`, a root it does not make",
    label = "`{L}` is found here at its typed path",
    note = "a bootstrap finds what its own root spawned in its `begin`; what another root made, its bootstrap hands \
            over in what it provides"
)]
pub trait FoundBy<L, A> {}

impl<L, A> FoundBy<L, A> for A {}
