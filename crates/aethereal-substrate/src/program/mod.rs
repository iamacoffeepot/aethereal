//! A program: a host, the engine's options, and a list of roots, in order, each bootstrapping itself (ADR-0029).
//!
//! It is set up in two phases. First its settings: the engine's section, the host's, and each root's are composed,
//! and the layers given are decoded into them once (ADR-0028), before anything is made. Then its roots: the engine is
//! made from its section and its options, and each root bootstraps in the order it was listed, handed its settings
//! and what it needs from the roots before it.

mod bootstrap;
mod error;
mod host;
mod listed;
mod provided;
mod settings;
#[cfg(test)]
mod tests;

pub use bootstrap::{Bootstrap, Bootstrapping};
pub use error::{ProgramError, Reader, Unbuilt};
pub use host::{Builds, Host, Serves};
pub use listed::{Bootstraps, Composing, Stage};
pub use provided::{Gathers, Provided, Provision};
pub use settings::{ENGINE, EngineSettings, RUNNING};

use crate::{Engine, Setup};
use aethereal_actor::{Cycle, Guests, JobList, Links, Ordered};
use aethereal_data::Budget;
use aethereal_data::config::{Configuration, FlagTable, Flags, Overlay, Parts, RootSettings};
use core::marker::PhantomData;
use core::pin::Pin;
use core::time::Duration;
use std::sync::Arc;

/// What decoding a program's layers may hold at once, in bytes (ADR-0028).
const CONFIGURATION_BYTES: usize = 1 << 24;

/// One of the engine's options, applied to its setup as it is made.
type EngineOption = Box<dyn FnOnce(Setup<()>) -> Setup<()>>;

/// One root's section, with the program's defaults for it, added to the sections composed so far.
type Section = Box<dyn Fn(Composing) -> Result<Composing, ProgramError>>;

/// One change the program makes to the flags its sections suggest (ADR-0028).
type FlagChange = Box<dyn for<'parts> Fn(Flags<'parts>) -> Flags<'parts>>;

/// A program for the host `H`: the engine's options, its defaults for each section, the layers of its configuration,
/// and its roots `R`, a list, `()` for none and `(A, S)` for the root `A` listed after those of `S` (ADR-0029).
///
/// A root listed with [`root`](Self::root) starts from its settings kind's `Default`; one listed with
/// [`root_with`](Self::root_with) starts from the value the program gives. Either is only the program's defaults:
/// every layer, the command line, the environment, a file, or one given with [`layer`](Self::layer), wins over them
/// (ADR-0028).
///
/// ```
/// # use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Top};
/// # use aethereal_data::{ActorPath, Kind, PathSegment, Plain};
/// # use aethereal_substrate::{Bootstrap, Builds, Engine, Host, ProgramError, Settled};
/// #
/// # struct Bench(Engine);
/// #
/// # #[derive(Plain, Default)]
/// # #[kind(name = "example.bench.running")]
/// # #[settings]
/// # struct Running {}
/// #
/// # impl Host for Bench {
/// #     type Running = Running;
/// #     type Hosting<'host> = ();
/// #     type Exit = Engine;
/// #     type Error = ProgramError;
/// #
/// #     fn run(settled: Settled<Self>) -> Result<Engine, ProgramError> {
/// #         Ok(pollster::block_on(settled.bootstrap(()))?.0)
/// #     }
/// # }
/// #
/// # impl Builds for Bench {
/// #     fn build(settled: Settled<Self>) -> Result<Self, ProgramError> {
/// #         Ok(Self(pollster::block_on(settled.bootstrap(()))?.0))
/// #     }
/// # }
/// /// How long the show runs.
/// #[derive(Plain, Default)]
/// #[kind(name = "example.show")]
/// #[settings]
/// struct Show {
///     seconds: u32,
/// }
///
/// /// A scene whose config is its settings, read from the section `scene`.
/// #[derive(Plain, Bootstrap)]
/// #[kind(name = "example.scene")]
/// struct Scene;
/// #
/// # impl ChildOf for Scene {
/// #     type Parent = Top;
/// # }
/// #
/// # impl Actor for Scene {
/// #     type Config = Show;
/// #     type Key = ();
/// #     type Handles = ();
/// #     type BeginSends = ();
/// #     type EndSends = ();
/// #
/// #     fn begin(_: Show, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
/// #         Ok(Self)
/// #     }
/// #
/// #     fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
///
/// let Bench(engine) = Bench::program().root_with::<Scene>(Show { seconds: 3 }).build()?;
///
/// let scene = ActorPath::from_root(&PathSegment::new(Scene::NAME, &()));
/// assert!(engine.table().resolve(&scene).is_some());
/// # Ok::<(), Box<dyn core::error::Error>>(())
/// ```
/// A root that needs what no root listed before it provides does not build:
///
/// ```compile_fail
/// # use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Top};
/// # use aethereal_data::Plain;
/// # use aethereal_substrate::{Bootstrap, Bootstrapping, Builds, Engine, Host, ProgramError, Settled, Unbuilt};
/// #
/// # struct Bench(Engine);
/// #
/// # #[derive(Plain, Default)]
/// # #[kind(name = "example.bench.running")]
/// # #[settings]
/// # struct Running {}
/// #
/// # impl Host for Bench {
/// #     type Running = Running;
/// #     type Hosting<'host> = ();
/// #     type Exit = Engine;
/// #     type Error = ProgramError;
/// #
/// #     fn run(settled: Settled<Self>) -> Result<Engine, ProgramError> {
/// #         Ok(pollster::block_on(settled.bootstrap(()))?.0)
/// #     }
/// # }
/// #
/// # impl Builds for Bench {
/// #     fn build(settled: Settled<Self>) -> Result<Self, ProgramError> {
/// #         Ok(Self(pollster::block_on(settled.bootstrap(()))?.0))
/// #     }
/// # }
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "example.nothing")]
/// # struct Nothing;
/// #
/// # macro_rules! root {
/// #     ($name:ident, $named:literal) => {
/// #         #[derive(Plain)]
/// #         #[kind(name = $named)]
/// #         struct $name;
/// #
/// #         impl ChildOf for $name {
/// #             type Parent = Top;
/// #         }
/// #
/// #         impl Actor for $name {
/// #             type Config = Nothing;
/// #             type Key = ();
/// #             type Handles = ();
/// #             type BeginSends = ();
/// #             type EndSends = ();
/// #
/// #             fn begin(_: Nothing, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
/// #                 Ok(Self)
/// #             }
/// #
/// #             fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
/// #                 Ok(())
/// #             }
/// #         }
/// #     };
/// # }
/// #
/// # root!(Windows, "example.windows");
/// # root!(Scene, "example.scene");
/// #
/// # #[derive(Clone)]
/// # struct Opened;
/// #
/// impl<H: Host> Bootstrap<H> for Windows {
///     const SECTION: &'static str = "windows";
///     type Settings = ();
///     type Needs = ();
///     type Provides = (Opened,);
///
///     async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(Opened,), Unbuilt> {
///         cx.begin(&(), Nothing)?;
///         Ok((Opened,))
///     }
/// }
///
/// impl<H: Host> Bootstrap<H> for Scene {
///     const SECTION: &'static str = "scene";
///     type Settings = ();
///     type Needs = (Opened,);
///     type Provides = ();
///
///     async fn bootstrap((): (), (_,): (Opened,), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
///         cx.begin(&(), Nothing)
///     }
/// }
///
/// // The scene needs what the windows provide, listed after it.
/// let program = Bench::program().root::<Scene>().root::<Windows>().build();
/// ```
///
/// Nor does a bootstrap that gives the host an end it does not serve:
///
/// ```compile_fail
/// # use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Top};
/// # use aethereal_data::Plain;
/// # use aethereal_substrate::{Bootstrap, Bootstrapping, Builds, Engine, Host, ProgramError, Settled, Unbuilt};
/// #
/// # struct Bench(Engine);
/// #
/// # #[derive(Plain, Default)]
/// # #[kind(name = "example.bench.running")]
/// # #[settings]
/// # struct Running {}
/// #
/// # impl Host for Bench {
/// #     type Running = Running;
/// #     type Hosting<'host> = ();
/// #     type Exit = Engine;
/// #     type Error = ProgramError;
/// #
/// #     fn run(settled: Settled<Self>) -> Result<Engine, ProgramError> {
/// #         Ok(pollster::block_on(settled.bootstrap(()))?.0)
/// #     }
/// # }
/// #
/// # impl Builds for Bench {
/// #     fn build(settled: Settled<Self>) -> Result<Self, ProgramError> {
/// #         Ok(Self(pollster::block_on(settled.bootstrap(()))?.0))
/// #     }
/// # }
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "example.nothing")]
/// # struct Nothing;
/// #
/// # macro_rules! root {
/// #     ($name:ident, $named:literal) => {
/// #         #[derive(Plain)]
/// #         #[kind(name = $named)]
/// #         struct $name;
/// #
/// #         impl ChildOf for $name {
/// #             type Parent = Top;
/// #         }
/// #
/// #         impl Actor for $name {
/// #             type Config = Nothing;
/// #             type Key = ();
/// #             type Handles = ();
/// #             type BeginSends = ();
/// #             type EndSends = ();
/// #
/// #             fn begin(_: Nothing, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
/// #                 Ok(Self)
/// #             }
/// #
/// #             fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
/// #                 Ok(())
/// #             }
/// #         }
/// #     };
/// # }
/// #
/// # root!(Windows, "example.windows");
/// # root!(Scene, "example.scene");
/// #
/// # #[derive(Clone)]
/// # struct Opened;
/// #
/// struct Feed;
///
/// impl<H: Host> Bootstrap<H> for Windows {
///     const SECTION: &'static str = "windows";
///     type Settings = ();
///     type Needs = ();
///     type Provides = ();
///
///     async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
///         cx.serve(Feed);
///         cx.begin(&(), Nothing)
///     }
/// }
/// ```
#[must_use]
pub struct Program<H: Host, R> {
    /// The engine's options, applied in the order they were set.
    options: Vec<EngineOption>,
    /// The program's defaults for the engine's section, beneath every layer: `EngineSettings`'s `Default` until the
    /// program gives its own (ADR-0028).
    engine: EngineSettings,
    /// The program's defaults for the host's own section, beneath every layer: its `Default` until the program gives
    /// its own (ADR-0028).
    running: H::Running,
    /// Each root's section, with the program's defaults for it, in the order the roots were listed (ADR-0029).
    sections: Vec<Section>,
    /// The layers of the configuration, the first winning, every one above the program's defaults (ADR-0028).
    layers: Vec<Overlay>,
    /// The program's changes to the flags its sections suggest, in the order it made them.
    flags: Vec<FlagChange>,
    listed: PhantomData<fn() -> (H, R)>,
}

impl<H: Host> Program<H, ()> {
    /// A program with no options, no layers, and no roots, its host's sections starting from their `Default`s.
    pub(super) fn new() -> Self {
        Self {
            options: Vec::new(),
            engine: EngineSettings::default(),
            running: H::Running::default(),
            sections: Vec::new(),
            layers: Vec::new(),
            flags: Vec::new(),
            listed: PhantomData,
        }
    }

    /// The program with its defaults for the engine's section `settings`, in place of `EngineSettings`'s `Default`:
    /// beneath every layer, so a person running it still sets any field (ADR-0028).
    ///
    /// `.with_engine(EngineSettings { workers, ..Default::default() })` gives the fields it has opinions on.
    pub fn with_engine(mut self, settings: EngineSettings) -> Self {
        self.engine = settings;
        self
    }

    /// The program with its defaults for the host's own section `settings`, in place of its `Default`, as
    /// [`with_engine`](Self::with_engine) gives the engine's.
    pub fn with_running(mut self, settings: H::Running) -> Self {
        self.running = settings;
        self
    }

    /// The program with the engine's option `option` added after those set already.
    fn with(mut self, option: EngineOption) -> Self {
        self.options.push(option);
        self
    }

    /// The engine, running the guests' code `guests` holds (ADR-0015), as [`Setup::with_guests`] does.
    pub fn with_guests(self, guests: Arc<dyn Guests>) -> Self {
        self.with(Box::new(move |setup| setup.with_guests(guests)))
    }

    /// The engine, keeping the native types `links` by name, for a guest to spawn, as [`Setup::with_links`] does.
    pub fn with_links(self, links: Links) -> Self {
        self.with(Box::new(move |setup| setup.with_links(links)))
    }

    /// The engine, letting its guests run the jobs of the types `J` lists, `.with_guest_jobs::<(Fetch, Decode)>()`, as
    /// [`Setup::with_guest_jobs`] does: a native turn's job carries its own code, so a program lists only what its
    /// guests run (ADR-0015, ADR-0018).
    pub fn with_guest_jobs<J: JobList>(self) -> Self {
        self.with(Box::new(|setup| setup.with_guest_jobs(J::types())))
    }

    /// The engine, running the cycle `C` between `Start` and `Stop`, as [`Setup::with_cycle`] does: a cycle out of its
    /// order does not build ([`Cycle`]).
    pub fn with_cycle<C: Cycle>(self) -> Self {
        let cycle: fn(Setup<()>) -> Setup<()> = Setup::with_cycle::<C>;

        self.with(Box::new(cycle))
    }

    /// The engine, limiting each `Tick`'s delta to `limit`, as [`Setup::with_delta_limit`] does.
    pub fn with_delta_limit(self, limit: Duration) -> Self {
        self.with(Box::new(move |setup| setup.with_delta_limit(limit)))
    }
}

impl<H: Host, R> Program<H, R> {
    /// The program with the root `A` listed after its roots so far, its settings starting from their kind's
    /// `Default`: it builds only when `H` bootstraps `A`, and only when `A`'s settings have a `Default`. A root whose
    /// settings have none is listed with [`root_with`](Self::root_with).
    ///
    /// ```compile_fail
    /// # use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Top};
    /// # use aethereal_data::Plain;
    /// # use aethereal_substrate::{Bootstrap, Builds, Engine, Host, ProgramError, Settled};
    /// #
    /// # struct Bench(Engine);
    /// #
    /// # #[derive(Plain, Default)]
    /// # #[kind(name = "example.bench.running")]
    /// # #[settings]
    /// # struct Running {}
    /// #
    /// # impl Host for Bench {
    /// #     type Running = Running;
    /// #     type Hosting = ();
    /// #     type Exit = Engine;
    /// #
    /// #     fn run(settled: Settled<Self>) -> Result<Engine, ProgramError> {
    /// #         settled.bootstrap(&mut ())
    /// #     }
    /// # }
    /// #
    /// # impl Builds for Bench {
    /// #     fn build(settled: Settled<Self>) -> Result<Self, ProgramError> {
    /// #         Ok(Self(settled.bootstrap(&mut ())?))
    /// #     }
    /// # }
    /// /// Where the show is played: it has no honest default.
    /// #[derive(Plain)]
    /// #[kind(name = "example.show")]
    /// #[settings]
    /// struct Show {
    ///     port: u16,
    /// }
    ///
    /// #[derive(Plain, Bootstrap)]
    /// #[kind(name = "example.scene")]
    /// struct Scene;
    /// #
    /// # impl ChildOf for Scene {
    /// #     type Parent = Top;
    /// # }
    /// #
    /// # impl Actor for Scene {
    /// #     type Config = Show;
    /// #     type Key = ();
    /// #     type Handles = ();
    /// #     type BeginSends = ();
    /// #     type EndSends = ();
    /// #
    /// #     fn begin(_: Show, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
    /// #         Ok(Self)
    /// #     }
    /// #
    /// #     fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
    /// #         Ok(())
    /// #     }
    /// # }
    ///
    /// // `Show` has no `Default`: the program must give it, `.root_with::<Scene>(Show { port: 7000 })`.
    /// let program = Bench::program().root::<Scene>();
    /// ```
    pub fn root<A>(self) -> Program<H, (A, R)>
    where
        A: Bootstrap<H>,
        A::Settings: Default,
    {
        self.root_with::<A>(A::Settings::default())
    }

    /// The program with the root `A` listed after its roots so far, its settings starting from `settings`, this
    /// program's defaults for `A`'s section, in place of its kind's `Default`: beneath every layer, so a person
    /// running it still sets any field (ADR-0028). It builds only when `H` bootstraps `A`.
    ///
    /// `.root_with::<Counter>(Counters { children, ..Default::default() })` gives the fields it has opinions on.
    pub fn root_with<A: Bootstrap<H>>(self, settings: A::Settings) -> Program<H, (A, R)> {
        let Self { options, engine, running, mut sections, layers, flags, listed: _ } = self;
        sections.push(Box::new(move |composing| composing.add(A::SECTION, Reader::Root(A::NAME), &settings)));

        Program { options, engine, running, sections, layers, flags, listed: PhantomData }
    }

    /// The program with the layer `overlay` beneath those given already: an earlier layer wins, and every layer wins
    /// over the program's defaults (ADR-0028).
    pub fn layer(mut self, overlay: Overlay) -> Self {
        self.layers.push(overlay);
        self
    }

    /// The program with the flags its sections suggest changed by `change`, after its changes so far: it renames,
    /// hides, or adds a flag for any field (ADR-0028).
    pub fn flags(mut self, change: impl for<'parts> Fn(Flags<'parts>) -> Flags<'parts> + 'static) -> Self {
        self.flags.push(Box::new(change));
        self
    }

    /// The sections of the program's configuration: the engine's, the host's, then each root's in order, each with
    /// the program's defaults for it, for reading layers against.
    ///
    /// # Errors
    ///
    /// [`ProgramError::Repeated`] when two sections share a name, naming what reads each.
    pub fn parts(&self) -> Result<Parts, ProgramError> {
        let host =
            Composing::new().add(ENGINE, Reader::Host, &self.engine)?.add(RUNNING, Reader::Host, &self.running)?;
        let composed = self.sections.iter().try_fold(host, |composing, section| section(composing))?;

        Ok(composed.into_parts())
    }

    /// The program's flags: what its sections suggest, as it changed them, for a host to read its command line or
    /// its page with (ADR-0028).
    ///
    /// # Errors
    ///
    /// [`ProgramError::Repeated`] as [`parts`](Self::parts) refuses, and [`ProgramError::Flags`] when a change names
    /// a flag that does not exist or two flags are left of one name.
    pub fn flag_table(&self) -> Result<FlagTable, ProgramError> {
        let parts = self.parts()?;
        let changed = self.flags.iter().fold(Flags::of(&parts), |flags, change| change(flags));

        Ok(changed.finish()?)
    }

    /// Bootstraps the program, then runs it on its host: it builds only when each root reaches at a typed path only
    /// roots listed before it (`Ordered`, ADR-0022), and each root's needs are provided by roots listed before it.
    ///
    /// # Errors
    ///
    /// Why the program did not start, as [`ProgramError`] says, or did not run to its end, as the host's error says.
    pub fn run<I: 'static, O>(self) -> Result<H::Exit, H::Error>
    where
        R: Bootstraps<H, I> + Ordered<O> + 'static,
    {
        H::run(self.settle()?)
    }

    /// Bootstraps the program, and its host holding the engine, not yet run: for a test to step. It builds as
    /// [`run`](Self::run) does.
    ///
    /// # Errors
    ///
    /// Why the program did not start, as [`ProgramError`] says.
    pub fn build<I: 'static, O>(self) -> Result<H, ProgramError>
    where
        H: Builds,
        R: Bootstraps<H, I> + Ordered<O> + 'static,
    {
        H::build(self.settle()?)
    }

    /// The first phase: every section composed, every layer decoded into them, and the host's sections taken.
    fn settle<I: 'static>(self) -> Result<Settled<H>, ProgramError>
    where
        R: Bootstraps<H, I> + 'static,
    {
        let mut budget = Budget::new(CONFIGURATION_BYTES);
        let mut configuration = self.parts()?.configure(&self.layers, &mut budget)?;
        let engine = EngineSettings::take(&mut configuration, ENGINE)?;
        let running = H::Running::take(&mut configuration, RUNNING)?;

        let roots = bootstrap_roots::<H, R, I>;

        Ok(Settled { engine, running, options: self.options, configuration, roots })
    }
}

/// Bootstrapping a program's roots on a stage: a future, awaited on a host whose setup awaits, and blocked on by one
/// that does not.
type Booting<'host, H> = Pin<Box<dyn Future<Output = Result<Stage<'host, H>, ProgramError>> + 'host>>;

/// Bootstraps the roots `R` on `stage`, dropping what they provided.
fn bootstrap_roots<H: Host, R: Bootstraps<H, I> + 'static, I: 'static>(stage: Stage<'_, H>) -> Booting<'_, H> {
    Box::pin(async move { Ok(R::bootstrap(stage).await?.0) })
}

/// A program whose settings are read, its engine and roots not yet made: what a host bootstraps when it is ready to,
/// on its hosting (ADR-0029).
pub struct Settled<H: Host> {
    engine: EngineSettings,
    running: H::Running,
    options: Vec<EngineOption>,
    configuration: Configuration,
    roots: for<'host> fn(Stage<'host, H>) -> Booting<'host, H>,
}

impl<H: Host> Settled<H> {
    /// The engine's settings.
    #[must_use]
    pub const fn engine(&self) -> &EngineSettings {
        &self.engine
    }

    /// The host's own settings: how the program is run.
    #[must_use]
    pub const fn running(&self) -> &H::Running {
        &self.running
    }

    /// The second phase: the engine made from its settings and options, then each root bootstrapped in order on
    /// `hosting`, each root's bootstrap awaited before the next begins, and the engine with the hosting, which holds
    /// the ends the roots gave it. A host whose setup awaits, as the browser's does to open its GPU, awaits it; a host
    /// that does not blocks on it (ADR-0029).
    ///
    /// # Errors
    ///
    /// [`ProgramError::Root`] naming the first root whose bootstrap failed; the engine and the roots begun before it
    /// are dropped.
    pub async fn bootstrap(self, hosting: H::Hosting<'_>) -> Result<(Engine, H::Hosting<'_>), ProgramError> {
        let mut setup = self.engine.setup();
        for option in self.options {
            setup = option(setup);
        }

        let stage = Stage::new(setup.ready(), hosting, self.configuration);

        Ok((self.roots)(stage).await?.into_made())
    }
}
