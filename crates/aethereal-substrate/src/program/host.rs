//! A host: what runs a program, and what it serves the program's roots (ADR-0029).

use super::{Program, ProgramError, Settled};
use aethereal_data::config::Settings;

/// What runs a program: a chassis, which owns its I/O and how the engine is stepped (ADR-0029).
///
/// A host brings its own section of the configuration, `running`, beside the engine's, and keeps its hosting as its
/// program bootstraps: the ends its roots give it, and its own setup a bootstrap reaches, such as a screen.
pub trait Host: Sized {
    /// The host's own section: how the program is run.
    type Running: Settings;
    /// What the host keeps as its program bootstraps, which each root's bootstrap reaches through
    /// [`Bootstrapping::host`](super::Bootstrapping::host) and gives its ends to.
    type Hosting;
    /// What running a program to its end comes to.
    type Exit;

    /// The program this host runs, with no options and no roots yet.
    fn program() -> Program<Self, ()> {
        Program::new()
    }

    /// Bootstraps `settled`, a program whose settings are read, its engine and roots on its own hosting, then runs
    /// it to its end.
    ///
    /// # Errors
    ///
    /// Why bootstrapping it failed, as [`Settled::bootstrap`] says.
    fn run(settled: Settled<Self>) -> Result<Self::Exit, ProgramError>;
}

/// A host that can hand back its program bootstrapped and not yet run, for a test to step (ADR-0029).
///
/// A host whose setup happens inside its event loop, as the desktop's does, has only [`Host::run`].
pub trait Builds: Host {
    /// Bootstraps `settled`, and the host holding the engine it made, not yet run.
    ///
    /// # Errors
    ///
    /// Why bootstrapping it failed, as [`Settled::bootstrap`] says.
    fn build(settled: Settled<Self>) -> Result<Self, ProgramError>;
}

/// A host that serves the end `E` a root's bootstrap made: the half of a pair that is the host's to keep (ADR-0029).
#[diagnostic::on_unimplemented(
    message = "`{Self}` does not serve `{E}`",
    label = "a root's bootstrap gives this host an end it does not serve",
    note = "bootstrap the root only on hosts that implement `Serves<{E}>`"
)]
pub trait Serves<E>: Host {
    /// Keeps `end` in `hosting`.
    fn serve(hosting: &mut Self::Hosting, end: E);
}
