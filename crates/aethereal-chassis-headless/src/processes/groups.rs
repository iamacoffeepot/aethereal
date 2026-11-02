//! The native host's launcher: each program in a process group of its own, started and waited on by a thread of its
//! own, its stderr's tail read by another (ADR-0040).

use super::tail::Tail;
use aethereal_process::{
    ExitStatus, Exited, Exiting, Launcher, ProcessesSettings, Signal, Starting, Unstarted, Unwaited,
};
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::future::{self, Future};
use std::io::{self, ErrorKind};
use std::mem::{self, MaybeUninit};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::pin::Pin;
use std::process::{Child, Command, ExitStatus as WaitStatus, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Waker};
use std::thread;
use std::time::Duration;

/// How long the stderr of a program that has exited is read on for, at most: what it wrote is in the pipe and read at
/// once, so this is spent only when a program it started holds the pipe open.
const DRAIN_WITHIN: Duration = Duration::from_secs(1);

/// The native host's launcher (ADR-0040): each program started by a thread of its own in a process group of its own,
/// with no stdin and its stdout discarded, the thread then waiting for it to exit while another keeps the last bytes of
/// its stderr. A start and a wait never block the thread that polls them, and are woken once done.
///
/// Dropping it kills the group of every program still running, so no program outlives the engine that started it;
/// each program's threads end once it has exited.
pub struct ProcessGroups {
    shared: Arc<Shared>,
}

/// What the launcher and its programs' threads share.
struct Shared {
    /// Each program started whose exit has not been taken, by its number. A lock, as the jobs that start, wait on, and
    /// signal programs run on whichever runner's thread polls them, and each program's thread marks it reaped: a
    /// signal and a reaping each hold it, so a group is never signalled once its leader is reaped and its id is free to
    /// be taken.
    programs: Mutex<BTreeMap<u64, Program>>,
    /// The most bytes of each program's stderr kept.
    most_stderr_bytes: usize,
}

/// A program started, and its exit, which its thread delivers.
struct Program {
    group: Group,
    exit: Arc<Mutex<Delivery<Result<Exited, Unwaited>>>>,
}

/// Where a program's process group stands.
#[derive(Clone, Copy)]
enum Group {
    /// Its thread is starting it: there is nothing to signal.
    Starting,
    /// Running: the id of its leader, which is the group's id.
    Running(u32),
    /// Its leader has exited and been reaped: its id may name another process.
    Reaped,
}

/// Where a start or an exit stands, as the thread that delivers it and the job that polls it both see it. A lock, as
/// the two are on different threads and either may come first.
enum Delivery<T> {
    /// Asked, and not yet polled.
    Asked,
    /// Polled before it was delivered: woken once it is.
    Polled(Waker),
    /// Delivered.
    Done(T),
    /// Handed to the job that polled it.
    Handed,
}

/// What came of a delivery never delivered, or polled again once handed.
struct Lost;

impl ProcessGroups {
    /// The launcher keeping as much of each program's stderr as `settings` say.
    #[must_use]
    pub fn new(settings: &ProcessesSettings) -> Self {
        let most_stderr_bytes = usize::try_from(settings.most_stderr_bytes).unwrap_or(usize::MAX);

        Self { shared: Arc::new(Shared { programs: Mutex::new(BTreeMap::new()), most_stderr_bytes }) }
    }
}

impl Launcher for ProcessGroups {
    fn start(&self, process: u64, program: String, args: Vec<String>) -> Starting<'_> {
        let started = Arc::new(Mutex::new(Delivery::Asked));
        let exit = Arc::new(Mutex::new(Delivery::Asked));
        let is_new = self.shared.admit(process, Program { group: Group::Starting, exit: Arc::clone(&exit) });
        if !is_new {
            return Box::pin(future::ready(Err(Unstarted)));
        }

        let shared = Arc::clone(&self.shared);
        let starting = Arc::clone(&started);
        #[allow(clippy::disallowed_methods)] // The native host owns the real threads (ADR-0001).
        let spawned = thread::Builder::new()
            .name(format!("aethereal-process-{process}"))
            .spawn(move || keep(&shared, process, command(program, args), &starting, &exit));
        if spawned.is_err() {
            self.shared.programs().remove(&process);
            return Box::pin(future::ready(Err(Unstarted)));
        }

        Box::pin(async move { Awaited { delivery: started }.await.unwrap_or(Err(Unstarted)) })
    }

    fn wait(&self, process: u64) -> Exiting<'_> {
        let found = self.shared.programs().get(&process).map(|program| Arc::clone(&program.exit));
        let Some(exit) = found else {
            return Box::pin(future::ready(Err(Unwaited)));
        };

        Box::pin(async move {
            let exited = Awaited { delivery: exit }.await;
            self.shared.programs().remove(&process);
            exited.unwrap_or(Err(Unwaited))
        })
    }

    fn signal(&self, process: u64, signal: Signal) {
        let programs = self.shared.programs();
        if let Some(Program { group: Group::Running(leader), .. }) = programs.get(&process) {
            signal_group(*leader, signal);
        }
    }
}

impl Drop for ProcessGroups {
    fn drop(&mut self) {
        for program in self.shared.programs().values() {
            if let Group::Running(leader) = program.group {
                signal_group(leader, Signal::Kill);
            }
        }
    }
}

impl Shared {
    /// The programs, whatever a thread that panicked holding them left.
    fn programs(&self) -> MutexGuard<'_, BTreeMap<u64, Program>> {
        self.programs.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Keeps `program` as the program numbered `process`, unless one is kept under that number already: whether it was
    /// kept.
    fn admit(&self, process: u64, program: Program) -> bool {
        match self.programs().entry(process) {
            Entry::Vacant(vacant) => {
                vacant.insert(program);
                true
            }
            Entry::Occupied(_) => false,
        }
    }

    /// Notes where the group of the program numbered `process` stands.
    fn mark(programs: &mut BTreeMap<u64, Program>, process: u64, group: Group) {
        if let Some(program) = programs.get_mut(&process) {
            program.group = group;
        }
    }
}

/// The command that starts `program` with `args` in a process group of its own, its stdin empty, its stdout
/// discarded, and its stderr piped to the launcher.
fn command(program: String, args: Vec<String>) -> Command {
    let mut command = Command::new(program);
    command.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).process_group(0);

    command
}

/// Starts the program numbered `process` with `command`, delivers whether it started, then waits for it to exit,
/// reaping it with the programs locked, and delivers how it did with the tail of its stderr: a program's own thread.
fn keep(
    shared: &Shared,
    process: u64,
    mut command: Command,
    started: &Mutex<Delivery<Result<(), Unstarted>>>,
    exit: &Mutex<Delivery<Result<Exited, Unwaited>>>,
) {
    let tail = Arc::new(Tail::new(shared.most_stderr_bytes));
    let running = command.spawn().map_err(|_| Unstarted).and_then(|child| read_stderr(child, &tail));
    let mut child = match running {
        Ok(child) => child,
        Err(unstarted) => {
            shared.programs().remove(&process);
            deliver(started, Err(unstarted));
            return;
        }
    };
    let leader = child.id();
    Shared::mark(&mut shared.programs(), process, Group::Running(leader));
    deliver(started, Ok(()));

    let reaped = if has_exited(leader) {
        let mut programs = shared.programs();
        Shared::mark(&mut programs, process, Group::Reaped);
        child.wait()
    } else {
        // The wait that leaves the program to be reaped failed: it is reaped first, and marked after.
        let reaped = child.wait();
        Shared::mark(&mut shared.programs(), process, Group::Reaped);
        reaped
    };
    let exited = reaped.map(status).map_err(|_| Unwaited);
    deliver(exit, exited.map(|status| Exited { status, stderr: tail.text(DRAIN_WITHIN) }));
}

/// `child`, its stderr read into `tail` by a thread of its own: refused, the child killed and reaped, when the host
/// cannot make the thread, as a program whose stderr no one reads would stall on it.
fn read_stderr(mut child: Child, tail: &Arc<Tail>) -> Result<Child, Unstarted> {
    let Some(stderr) = child.stderr.take() else {
        return Ok(child);
    };
    let reading = Arc::clone(tail);
    #[allow(clippy::disallowed_methods)] // The native host owns the real threads (ADR-0001).
    let spawned =
        thread::Builder::new().name(format!("aethereal-stderr-{}", child.id())).spawn(move || reading.read(stderr));
    if spawned.is_err() {
        // The program never ran as the launcher's: it is ended before anyone could have signalled it.
        let _ = child.kill();
        let _ = child.wait();
        return Err(Unstarted);
    }

    Ok(child)
}

/// Whether the child `leader` has exited, waiting until it has, and leaving it to be reaped: false when the wait
/// failed.
fn has_exited(leader: u32) -> bool {
    loop {
        let mut info = MaybeUninit::<libc::siginfo_t>::zeroed();
        // SAFETY: `info` is a zeroed `siginfo_t` this frame owns, which `waitid` only writes through the pointer, and
        // which is never read. `WNOWAIT` leaves the child to be reaped, so its `Child` still owns it.
        let waited = unsafe { libc::waitid(libc::P_PID, leader, info.as_mut_ptr(), libc::WEXITED | libc::WNOWAIT) };
        if waited == 0 {
            return true;
        }
        let is_interrupted = io::Error::last_os_error().kind() == ErrorKind::Interrupted;
        if !is_interrupted {
            return false;
        }
    }
}

/// Sends `signal` to the process group `leader` leads.
fn signal_group(leader: u32, signal: Signal) {
    let Ok(group) = libc::pid_t::try_from(leader) else {
        return;
    };
    let number = match signal {
        Signal::Terminate => libc::SIGTERM,
        Signal::Kill => libc::SIGKILL,
    };
    // SAFETY: `kill` takes two integers and touches no memory of this process. The caller holds the programs locked
    // with the leader not yet reaped, so the id names its group and no other; a group already gone is answered with an
    // error, which says nothing the program's exit will not.
    let _ = unsafe { libc::kill(-group, number) };
}

/// How a program exited, as its wait status says.
fn status(status: WaitStatus) -> ExitStatus {
    let raw = status.into_raw();
    if libc::WIFEXITED(raw) {
        ExitStatus::Code(libc::WEXITSTATUS(raw))
    } else {
        ExitStatus::Signal(libc::WTERMSIG(raw))
    }
}

/// Hands `done` to what `delivery` tracks, waking the job that polled it.
fn deliver<T>(delivery: &Mutex<Delivery<T>>, done: T) {
    let was = mem::replace(&mut *delivered(delivery), Delivery::Done(done));
    if let Delivery::Polled(waker) = was {
        waker.wake();
    }
}

/// Where a delivery stands, whatever a thread that panicked holding it left.
fn delivered<T>(delivery: &Mutex<Delivery<T>>) -> MutexGuard<'_, Delivery<T>> {
    delivery.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A start or an exit, as the job that asked it polls it.
struct Awaited<T> {
    delivery: Arc<Mutex<Delivery<T>>>,
}

impl<T> Future for Awaited<T> {
    type Output = Result<T, Lost>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let mut delivery = delivered(&self.delivery);
        let was = mem::replace(&mut *delivery, Delivery::Handed);
        match was {
            Delivery::Done(done) => Poll::Ready(Ok(done)),
            Delivery::Asked | Delivery::Polled(_) => {
                *delivery = Delivery::Polled(context.waker().clone());
                Poll::Pending
            }
            Delivery::Handed => Poll::Ready(Err(Lost)),
        }
    }
}
