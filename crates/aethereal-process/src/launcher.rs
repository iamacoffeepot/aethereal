//! The host's launcher, which starts, waits on, and signals the capability's programs: a service the host serves its
//! jobs (ADR-0034, ADR-0040).

use crate::Exited;
use aethereal_data::Plain;
use std::future::Future;
use std::pin::Pin;

/// A signal a process sends its program's group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.process.signal")]
pub enum Signal {
    /// Asks the group to end: SIGTERM.
    Terminate,
    /// Ends the group: SIGKILL.
    Kill,
}

/// Why a program was not started: nothing runnable is at its path, or the host could not make its process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unstarted;

/// Why a program's exit cannot be told: the launcher holds no program of that number, its exit was already taken, or
/// the thread that waited on it was lost. A process whose exit is lost fails with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.process.unwaited")]
pub struct Unwaited;

/// A start, as its launcher does it: the program running, or why it is not.
pub type Starting<'a> = Pin<Box<dyn Future<Output = Result<(), Unstarted>> + 'a>>;

/// A wait, as its launcher does it: how the program exited, once it has, and the tail of its stderr.
pub type Exiting<'a> = Pin<Box<dyn Future<Output = Result<Exited, Unwaited>> + 'a>>;

/// The host's launcher (ADR-0040): it starts each program in a process group of its own, with no stdin and its stdout
/// discarded, keeps the last bytes of its stderr, and signals its group. A host serves it into the services its
/// runners hand every job (ADR-0034); a host that serves none, as a browser, refuses every start.
///
/// Each program is named by the number the root gave its process. Nothing it does blocks the thread that polls it: a
/// launcher whose waits block does them on threads of its own, and wakes the start or the wait once it is done.
pub trait Launcher: Send + Sync {
    /// Starts `program`, a path on the host's machine, with `args`, as the program numbered `process`.
    fn start(&self, process: u64, program: String, args: Vec<String>) -> Starting<'_>;

    /// Waits for the program numbered `process` to exit, and answers with how it did and the tail of its stderr. The
    /// exit is told once: the launcher then forgets the program.
    fn wait(&self, process: u64) -> Exiting<'_>;

    /// Sends `signal` to the group of the program numbered `process`, while it runs. One that has exited is not
    /// signalled, so a signal never reaches a group that has since taken its number.
    fn signal(&self, process: u64, signal: Signal);
}
