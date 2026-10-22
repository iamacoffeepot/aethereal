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

use crate::{Engine, Jobs, Setup};
use aethereal_actor::{Cycle, Guests, Links, Ordered, Unordered, ordered};
use aethereal_data::Budget;
use aethereal_data::config::{Configuration, Overlay, Parts, RootSettings};
use core::marker::PhantomData;
use core::time::Duration;
use std::sync::Arc;

/// What decoding a program's layers may hold at once, in bytes (ADR-0028).
const CONFIGURATION_BYTES: usize = 1 << 24;

/// One of the engine's options, applied to its setup as it is made.
type EngineOption = Box<dyn FnOnce(Setup<()>) -> Result<Setup<()>, Unordered>>;

/// A program for the host `H`: the engine's options, the layers of its configuration, and its roots `R`, a list, `()`
/// for none and `(A, S)` for the root `A` listed after those of `S` (ADR-0029).
///
/// ```
/// # use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Top};
/// # use aethereal_data::config::Overlay;
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
/// let program = Bench::program().root::<Scene>();
/// let mut code = Overlay::new("code");
/// code.set(&program.parts()?, "scene.seconds=3")?;
/// let Bench(engine) = program.layer(code).build()?;
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
///     fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(Opened,), Unbuilt> {
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
///     fn bootstrap((): (), (_,): (Opened,), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
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
///     fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
///         cx.serve(Feed);
///         cx.begin(&(), Nothing)
///     }
/// }
/// ```
#[must_use]
pub struct Program<H, R> {
    /// The engine's options, applied in the order they were set.
    options: Vec<EngineOption>,
    /// The layers of the configuration, the first winning (ADR-0028).
    layers: Vec<Overlay>,
    listed: PhantomData<fn() -> (H, R)>,
}

impl<H: Host> Program<H, ()> {
    /// A program with no options, no layers, and no roots.
    pub(super) const fn new() -> Self {
        Self { options: Vec::new(), layers: Vec::new(), listed: PhantomData }
    }

    /// The program with the engine's option `option` added after those set already.
    fn with(mut self, option: EngineOption) -> Self {
        self.options.push(option);
        self
    }

    /// The engine, running the guests' code `guests` holds (ADR-0015), as [`Setup::with_guests`] does.
    pub fn with_guests(self, guests: Arc<dyn Guests>) -> Self {
        self.with(Box::new(move |setup| Ok(setup.with_guests(guests))))
    }

    /// The engine, keeping the native types `links` by name, for a guest to spawn, as [`Setup::with_links`] does.
    pub fn with_links(self, links: Links) -> Self {
        self.with(Box::new(move |setup| Ok(setup.with_links(links))))
    }

    /// The engine, its jobs run by `jobs`, as [`Setup::with_jobs`] does.
    pub fn with_jobs(self, jobs: Arc<dyn Jobs>) -> Self {
        self.with(Box::new(move |setup| Ok(setup.with_jobs(jobs))))
    }

    /// The engine, running the cycle `C` between `Start` and `Stop`, as [`Setup::with_cycle`] does.
    ///
    /// # Errors
    ///
    /// [`Unordered`] when the cycle holds `Start` or `Stop`, a stage twice, or the shared stages out of their order.
    pub fn with_cycle<C: Cycle>(self) -> Result<Self, Unordered> {
        ordered(&C::stages())?;
        let cycle: fn(Setup<()>) -> Result<Setup<()>, Unordered> = Setup::with_cycle::<C>;

        Ok(self.with(Box::new(cycle)))
    }

    /// The engine, limiting each `Tick`'s delta to `limit`, as [`Setup::with_delta_limit`] does.
    pub fn with_delta_limit(self, limit: Duration) -> Self {
        self.with(Box::new(move |setup| Ok(setup.with_delta_limit(limit))))
    }
}

impl<H: Host, R> Program<H, R> {
    /// The program with the root `A` listed after its roots so far: it builds only when `H` bootstraps `A`.
    pub fn root<A: Bootstrap<H>>(self) -> Program<H, (A, R)> {
        Program { options: self.options, layers: self.layers, listed: PhantomData }
    }

    /// The program with the layer `overlay` beneath those given already: an earlier layer wins (ADR-0028).
    pub fn layer(mut self, overlay: Overlay) -> Self {
        self.layers.push(overlay);
        self
    }

    /// The sections of the program's configuration: the engine's, the host's, then each root's in order, for
    /// reading layers against.
    ///
    /// # Errors
    ///
    /// [`ProgramError::Repeated`] when two sections share a name, naming what reads each.
    pub fn parts<I>(&self) -> Result<Parts, ProgramError>
    where
        R: Bootstraps<H, I>,
    {
        let composing =
            Composing::new().add::<EngineSettings>(ENGINE, Reader::Host)?.add::<H::Running>(RUNNING, Reader::Host)?;

        Ok(R::compose(composing)?.into_parts())
    }

    /// Bootstraps the program, then runs it on its host: it builds only when each root reaches at a typed path only
    /// roots listed before it (`Ordered`, ADR-0022), and each root's needs are provided by roots listed before it.
    ///
    /// # Errors
    ///
    /// Why the program did not start, as [`ProgramError`] says.
    pub fn run<I, O>(self) -> Result<H::Exit, ProgramError>
    where
        R: Bootstraps<H, I> + Ordered<O>,
    {
        H::run(self.settle()?)
    }

    /// Bootstraps the program, and its host holding the engine, not yet run: for a test to step. It builds as
    /// [`run`](Self::run) does.
    ///
    /// # Errors
    ///
    /// Why the program did not start, as [`ProgramError`] says.
    pub fn build<I, O>(self) -> Result<H, ProgramError>
    where
        H: Builds,
        R: Bootstraps<H, I> + Ordered<O>,
    {
        H::build(self.settle()?)
    }

    /// The first phase: every section composed, every layer decoded into them, and the host's sections taken.
    fn settle<I>(self) -> Result<Settled<H>, ProgramError>
    where
        R: Bootstraps<H, I>,
    {
        let mut budget = Budget::new(CONFIGURATION_BYTES);
        let mut configuration = self.parts()?.configure(&self.layers, &mut budget)?;
        let engine = EngineSettings::take(&mut configuration, ENGINE)?;
        let running = H::Running::take(&mut configuration, RUNNING)?;

        Ok(Settled { engine, running, options: self.options, configuration, roots: bootstrap_roots::<H, R, I> })
    }
}

/// Bootstraps the roots `R` on `stage`, dropping what they provided.
fn bootstrap_roots<H: Host, R: Bootstraps<H, I>, I>(stage: &mut Stage<'_, H>) -> Result<(), ProgramError> {
    R::bootstrap(stage)?;

    Ok(())
}

/// A program whose settings are read, its engine and roots not yet made: what a host bootstraps when it is ready to,
/// on its hosting (ADR-0029).
pub struct Settled<H: Host> {
    engine: EngineSettings,
    running: H::Running,
    options: Vec<EngineOption>,
    configuration: Configuration,
    roots: fn(&mut Stage<'_, H>) -> Result<(), ProgramError>,
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
    /// `hosting`, and the engine.
    ///
    /// # Errors
    ///
    /// [`ProgramError::Root`] naming the first root whose bootstrap failed; the engine and the roots begun before it
    /// are dropped.
    pub fn bootstrap(self, hosting: &mut H::Hosting) -> Result<Engine, ProgramError> {
        let mut setup = self.engine.setup();
        for option in self.options {
            setup = option(setup).map_err(ProgramError::Cycle)?;
        }

        let mut engine = setup.ready();
        (self.roots)(&mut Stage::new(&mut engine, hosting, self.configuration))?;

        Ok(engine)
    }
}
