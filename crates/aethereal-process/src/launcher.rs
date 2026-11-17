//! The host's launcher, which starts, waits on, and signals the capability's programs: a service the host serves its
//! jobs (ADR-0034, ADR-0040).

use crate::{Exited, Invocation};
use aethereal_data::Plain;
use aethereal_tcp::Socket;
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

/// The launcher's end of the pipe a program was started with.
pub enum PipeEnd {
    /// Its start connected neither its stdin nor its stdout: there is no pipe.
    Closed,
    /// One end of a socket pair, set not to block, whose other end is the program's stdin, its stdout, or both, as its
    /// start connected them.
    Connected(Socket),
}

/// A start, as its launcher does it: the program running, with the launcher's end of its pipe, or why it is not.
pub type Starting<'a> = Pin<Box<dyn Future<Output = Result<PipeEnd, Unstarted>> + 'a>>;

/// A wait, as its launcher does it: how the program exited, once it has, and the head and tail of its stderr.
pub type Exiting<'a> = Pin<Box<dyn Future<Output = Result<Exited, Unwaited>> + 'a>>;

/// The host's launcher (ADR-0040): it starts each program in a process group of its own, with the environment its
/// invocation states, its stdin and its stdout each closed or connected to one socket pair whose other end it answers,
/// keeps the head and the tail of its stderr, each bounded, and signals its group. A host serves it into the services
/// its runners hand every job (ADR-0034); a host that serves none, as a browser, refuses every start.
///
/// Each program is named by the number the root gave its process. Nothing it does blocks the thread that polls it: a
/// launcher whose waits block does them on threads of its own, and wakes the start or the wait once it is done.
pub trait Launcher: Send + Sync {
    /// Starts the program `invocation` names, as the program numbered `process`.
    fn start(&self, process: u64, invocation: Invocation) -> Starting<'_>;

    /// Waits for the program numbered `process` to exit, and answers with how it did and the head and tail of its stderr. The
    /// exit is told once: the launcher then forgets the program.
    fn wait(&self, process: u64) -> Exiting<'_>;

    /// Sends `signal` to the group of the program numbered `process`, while it runs. One that has exited is not
    /// signalled, so a signal never reaches a group that has since taken its number.
    fn signal(&self, process: u64, signal: Signal);
}
