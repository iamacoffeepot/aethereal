//! A desk: a host of a test's own that bootstraps a program as a chassis does, keeps what its roots served, and runs
//! nothing until the test steps it (ADR-0036).

use super::{Bench, Budget, Steps};
use crate::Engine;
use crate::program::{EngineSettings, Host, ProgramError, Settled};
use aethereal_actor::Time;
use aethereal_data::Plain;
use aethereal_data::config::Settings;

/// A host whose engine is a [`Bench`]'s: stepped on the test's thread at a time the test moves, so
/// [`Harness::advance`](super::Harness::advance) moves it.
pub trait Benched: Steps {
    /// The bench it steps.
    fn bench(&mut self) -> &mut Bench;
}

impl Benched for Bench {
    fn bench(&mut self) -> &mut Bench {
        self
    }
}

/// What a desk keeps of a program it bootstrapped: the engine on a bench, the hosting `K` its roots' bootstraps kept
/// their ends on, which a test reads back, and the settings it read, the engine's and its own section `R`.
///
/// A test names a desk of its own with [`desk!`](super::desk), which makes it a host, so its bootstraps of roots of
/// other crates, a window root or a renderer, are its own to write; the host derefs to this.
pub struct Desk<K, R> {
    bench: Bench,
    kept: K,
    running: R,
    settings: EngineSettings,
}

/// A desk's own section, which sets nothing: the section of a desk whose test reads none.
#[derive(Plain, Debug, Clone, Default)]
#[kind(name = "aether.harness.desk.running")]
#[settings]
pub struct DeskRunning {}

impl<K: Default, R: Settings + Clone> Desk<K, R> {
    /// Bootstraps `settled` as a host that does not await does, blocking on it, on a hosting of `K`'s `Default`, and
    /// the desk holding what came of it, its engine not yet started: for the host [`desk!`](super::desk) names.
    ///
    /// # Errors
    ///
    /// Why bootstrapping it failed, as [`Settled::bootstrap`] says.
    pub fn bootstrap<H>(settled: Settled<H>) -> Result<Self, ProgramError>
    where
        H: for<'host> Host<Running = R, Hosting<'host> = &'host mut K>,
    {
        let settings = settled.engine().clone();
        let running = settled.running().clone();
        let mut kept = K::default();
        let (engine, _) = pollster::block_on(settled.bootstrap(&mut kept))?;

        Ok(Self { bench: Bench { engine, now: Time::ZERO }, kept, running, settings })
    }
}

impl<K, R> Desk<K, R> {
    /// What the roots' bootstraps kept on the desk.
    #[must_use]
    pub const fn kept(&self) -> &K {
        &self.kept
    }

    /// The desk's own section, as the program read it.
    #[must_use]
    pub const fn running(&self) -> &R {
        &self.running
    }

    /// The engine's section, as the program read it.
    #[must_use]
    pub const fn settings(&self) -> &EngineSettings {
        &self.settings
    }
}

impl<K, R> Steps for Desk<K, R> {
    const BUDGET: Budget = Bench::BUDGET;

    fn engine(&mut self) -> &mut Engine {
        self.bench.engine()
    }

    fn run_step(&mut self) {
        self.bench.run_step();
    }
}

impl<K, R> Benched for Desk<K, R> {
    fn bench(&mut self) -> &mut Bench {
        &mut self.bench
    }
}

/// Names a desk of a test's own, `struct Name(Kept)` or `struct Name(Kept, Running)`: a host that bootstraps a
/// program as a chassis does, on a hosting of `&mut Kept` its roots' bootstraps keep their ends on, reads its own
/// section `Running` ([`DeskRunning`] when none is named), and runs nothing until the test steps it through a
/// [`Harness`](super::Harness), to the end the test chooses (ADR-0036). It derefs to its [`Desk`].
///
/// The host is the test's own type, not the [`Desk`] it holds, so that the test may write a bootstrap on it for a
/// root of another crate, as the overlay's does for the window root and the renderer: a `Bootstrap<Desk<Kept>>` for
/// a root it does not own is a foreign trait for foreign types, which the orphan rule refuses. Which ends the desk
/// serves, the test says as a host does, `impl Serves<Lit> for Name`.
///
/// ```
/// # use aethereal_actor::{Fault, actor};
/// # use aethereal_data::Plain;
/// # use aethereal_substrate::{Bootstrap, Host};
/// use aethereal_substrate::harness::{Harness, Unheard, desk};
/// #
/// # #[derive(Plain, Default)]
/// # #[kind(name = "example.show")]
/// # #[settings]
/// # struct Show {}
///
/// #[derive(Plain, Bootstrap)]
/// #[kind(name = "example.scene")]
/// struct Scene;
/// #
/// # #[actor]
/// # impl Scene {
/// #     fn begin(_: Show) -> Result<Self, Fault> {
/// #         Ok(Self)
/// #     }
/// # }
///
/// desk!(
///     /// A desk that keeps nothing.
///     struct Bench(());
/// );
///
/// let mut harness = Harness::of(Bench::program().root::<Scene>())?;
/// let scene = harness.root::<Scene>();
/// harness.settle();
/// assert!(harness.state(scene).is_ok());
/// # Ok::<(), Unheard>(())
/// ```
#[macro_export]
#[doc(hidden)]
macro_rules! __desk {
    ($(#[$attr:meta])* $vis:vis struct $name:ident($kept:ty);) => {
        $crate::__desk!($(#[$attr])* $vis struct $name($kept, $crate::harness::DeskRunning););
    };
    ($(#[$attr:meta])* $vis:vis struct $name:ident($kept:ty, $running:ty);) => {
        $(#[$attr])*
        $vis struct $name($crate::harness::Desk<$kept, $running>);

        impl ::core::ops::Deref for $name {
            type Target = $crate::harness::Desk<$kept, $running>;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl $crate::Host for $name {
            type Running = $running;
            type Hosting<'host> = &'host mut $kept;
            type Exit = Self;
            type Error = $crate::ProgramError;

            fn run(settled: $crate::Settled<Self>) -> ::core::result::Result<Self, $crate::ProgramError> {
                <Self as $crate::Builds>::build(settled)
            }
        }

        impl $crate::Builds for $name {
            fn build(settled: $crate::Settled<Self>) -> ::core::result::Result<Self, $crate::ProgramError> {
                $crate::harness::Desk::bootstrap(settled).map(Self)
            }
        }

        impl $crate::harness::Steps for $name {
            const BUDGET: $crate::harness::Budget =
                <$crate::harness::Desk<$kept, $running> as $crate::harness::Steps>::BUDGET;

            fn engine(&mut self) -> &mut $crate::Engine {
                $crate::harness::Steps::engine(&mut self.0)
            }

            fn run_step(&mut self) {
                $crate::harness::Steps::run_step(&mut self.0);
            }
        }

        impl $crate::harness::Benched for $name {
            fn bench(&mut self) -> &mut $crate::harness::Bench {
                $crate::harness::Benched::bench(&mut self.0)
            }
        }
    };
}

pub use crate::__desk as desk;
