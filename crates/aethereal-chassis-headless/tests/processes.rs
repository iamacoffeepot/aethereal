//! The process capability on the native host (ADR-0040, ADR-0036): a real program, the root asked through the harness,
//! and its launcher the real one, starting real programs. Each program is this test binary itself, run again with the
//! one test, `program`, picked, and told what to do by a [`Behaviour`], which it reads back from its arguments.
#![cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]

use aethereal_actor::{Actor, ActorRef, Answered, Beginning, ChildOf, Ending, Fault, Handles, ProtocolRef, Top, Turn};
use aethereal_chassis_headless::Headless;
use aethereal_chassis_headless::waiting::{self, BUDGET};
use aethereal_data::{Bound, Plain};
use aethereal_process::{ExitStatus, Exited, Exits, Process, Processes, ProcessesSettings, Start, Started, Stop};
use aethereal_substrate::Host;
use aethereal_substrate::harness::{Harness, Unheard};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::{env, fs, process, thread};

/// What a program this test starts does: one of a few fixed behaviours, each named by a token among its arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Behaviour {
    /// Exits with code 3.
    ExitsThree,
    /// Writes [`OLDER`] bytes of `a`, then [`NEWER`] bytes of `b`, to its stderr, and exits with code 0.
    FloodsStderr,
    /// Runs until something ends it.
    Lingers,
    /// Ignores SIGTERM, writes the file its arguments name to say it does, and runs until something ends it.
    IgnoresTerm,
}

/// How many bytes of `a` a program that floods its stderr writes first.
const OLDER: usize = 8192;
/// How many bytes of `b` it writes after them.
const NEWER: usize = 512;

/// What a token among a program's arguments begins with.
const BEHAVES: &str = "aethereal-behaves=";
/// What the argument naming the file a program writes once it ignores SIGTERM begins with.
const READY: &str = "aethereal-ready=";

impl Behaviour {
    const ALL: [Self; 4] = [Self::ExitsThree, Self::FloodsStderr, Self::Lingers, Self::IgnoresTerm];

    /// The name its token carries.
    const fn name(self) -> &'static str {
        match self {
            Self::ExitsThree => "exits-three",
            Self::FloodsStderr => "floods-stderr",
            Self::Lingers => "lingers",
            Self::IgnoresTerm => "ignores-term",
        }
    }

    /// The token that picks it among a program's arguments.
    fn token(self) -> String {
        format!("{BEHAVES}{}", self.name())
    }

    /// The behaviour whose token is among `args`, when one is.
    fn among(args: &[String]) -> Option<Self> {
        Self::ALL.into_iter().find(|behaviour| args.contains(&behaviour.token()))
    }
}

/// The test the program runs when started: a program told how to behave does it and never returns, and in a run of the
/// suite, where no behaviour is named, it does nothing.
#[test]
fn program() {
    let args: Vec<String> = env::args().collect();
    let Some(behaviour) = Behaviour::among(&args) else {
        return;
    };
    match behaviour {
        Behaviour::ExitsThree => process::exit(3),
        Behaviour::FloodsStderr => {
            let mut flood = vec![b'a'; OLDER];
            flood.extend(vec![b'b'; NEWER]);
            let written = io::stderr().write_all(&flood);
            process::exit(i32::from(written.is_err()));
        }
        Behaviour::Lingers => linger(),
        Behaviour::IgnoresTerm => {
            // SAFETY: setting SIGTERM's disposition to ignored installs no handler, so no code runs on the signal; the
            // program is single-minded from here, and nothing else in it depends on SIGTERM.
            let _ = unsafe { libc::signal(libc::SIGTERM, libc::SIG_IGN) };
            let ready = args.iter().find_map(|arg| arg.strip_prefix(READY));
            if let Some(ready) = ready {
                let _ = fs::write(ready, b"");
            }
            linger();
        }
    }
}

/// Runs until something ends the program.
fn linger() -> ! {
    loop {
        thread::park();
    }
}

/// A directory for one test, named for it, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    /// An empty directory for `test`.
    fn new(test: &str) -> Self {
        let root = env::temp_dir().join(format!("aethereal-processes-{}-{test}", process::id()));
        let _ = fs::remove_dir_all(&root);
        assert!(fs::create_dir_all(&root).is_ok(), "the test's directory is made");

        Self(root)
    }

    /// The path of `name` within it.
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// This test binary, as a program to start.
fn this_binary() -> String {
    let Ok(binary) = env::current_exe() else {
        panic!("the test knows its own binary");
    };

    binary.display().to_string()
}

/// The arguments that run this binary's `program` test alone, behaving as `behaviour`, and `extra` after.
fn args(behaviour: Behaviour, extra: &[String]) -> Vec<String> {
    let mut args: Vec<String> = ["--exact", "program", "--nocapture", "--test-threads=1"].map(String::from).into();
    args.push(behaviour.token());
    args.extend_from_slice(extra);

    args
}

/// A program holding the process capability as `settings` say, built and started.
fn processes(settings: ProcessesSettings) -> Result<Harness<Headless>, Unheard> {
    Harness::of(Headless::program().root_with::<Processes>(settings))
}

/// The settings that give a program asked to stop `stop_within_millis`, keeping `most_stderr_bytes` of its stderr.
const fn settings(stop_within_millis: u32, most_stderr_bytes: u32) -> ProcessesSettings {
    ProcessesSettings { stop_within_millis, most_stderr_bytes }
}

/// What starting `program` with `args` comes to, its exit told to `starter`.
fn start(
    harness: &mut Harness<Headless>,
    program: String,
    args: Vec<String>,
    starter: ProtocolRef<Exits>,
) -> Result<Started, Unheard> {
    let root = harness.root::<Processes>();
    let asked = harness.ask(root, &Start { program, args, starter })?;
    let Answered::Yes(answer) = harness.answer(asked)? else {
        panic!("the root answers a start");
    };

    Ok(answer)
}

/// The process running this binary as `behaviour`, `extra` among its arguments, its exit told to the outside.
fn running(
    harness: &mut Harness<Headless>,
    behaviour: Behaviour,
    extra: &[String],
) -> Result<ActorRef<Process>, Unheard> {
    let starter = harness.protocol::<Exits>();
    let Started::Running { process } = start(harness, this_binary(), args(behaviour, extra), starter)? else {
        panic!("this binary starts, as {behaviour:?}");
    };

    Ok(process)
}

#[test]
fn a_program_that_exits_three_is_told_as_code_three_and_its_process_ends() -> Result<(), Unheard> {
    // Catches an exit read as a signal or its code lost, `Exited` told to anyone but the starter, and a process that
    // stays once it has told the exit.
    let mut harness = processes(settings(60_000, 4096))?;
    let process = running(&mut harness, Behaviour::ExitsThree, &[])?;

    let exited = harness.heard::<Exited>()?;
    harness.settle();

    assert_eq!(exited.status, ExitStatus::Code(3));
    assert!(harness.state(process).is_err(), "the process has ended");

    Ok(())
}

#[test]
fn the_last_bytes_of_stderr_are_kept_and_older_ones_dropped() -> Result<(), Unheard> {
    // Catches a tail that keeps the first bytes instead of the last, one that keeps more than the setting says or one
    // byte off it, stderr not piped or not read to its end before the exit is told, and stdout taken for stderr.
    let mut harness = processes(settings(60_000, 1024))?;
    running(&mut harness, Behaviour::FloodsStderr, &[])?;

    let exited = harness.heard::<Exited>()?;

    let mut expected = "a".repeat(1024 - NEWER);
    expected.push_str(&"b".repeat(NEWER));
    assert_eq!(exited.status, ExitStatus::Code(0));
    assert_eq!(exited.stderr, expected);

    Ok(())
}

#[test]
fn stop_ends_a_program_with_sigterm() -> Result<(), Unheard> {
    // Catches a stop that kills at once (signal 9), one that sends nothing until its time has passed (the minute it is
    // given outlasts the harness's budget), and SIGTERM sent to anything but the program's group.
    let mut harness = processes(settings(60_000, 4096))?;
    let process = running(&mut harness, Behaviour::Lingers, &[])?;

    harness.tell(process, &Stop)?;
    let exited = harness.heard::<Exited>()?;

    assert_eq!(exited.status, ExitStatus::Signal(libc::SIGTERM));

    Ok(())
}

#[test]
fn a_program_that_ignores_sigterm_is_killed_once_its_time_has_passed() -> Result<(), Unheard> {
    // Catches a stop that never kills a program SIGTERM does not end, the kill sent before the time given has passed,
    // and a kill passed over as though the program had exited.
    let dir = Dir::new("ignores");
    let ready = dir.path("ready");
    let stop_within = Duration::from_millis(200);
    let mut harness = processes(settings(200, 4096))?;
    let process = running(&mut harness, Behaviour::IgnoresTerm, &[format!("{READY}{}", ready.display())])?;
    waiting::until(BUDGET, || Path::exists(&ready));
    assert!(ready.exists(), "the program ignores SIGTERM before it is stopped");

    #[allow(clippy::disallowed_methods)] // The program is killed on real time, which only bounds the wait from below.
    let stopped = Instant::now();
    harness.tell(process, &Stop)?;
    let exited = harness.heard::<Exited>()?;

    assert_eq!(exited.status, ExitStatus::Signal(libc::SIGKILL));
    assert!(stopped.elapsed() >= stop_within, "it was given its time before it was killed");

    Ok(())
}

/// What the test's starter is made from: nothing.
#[derive(Debug, Plain)]
#[kind(name = "aether.test.starter_config")]
struct StarterConfig;

/// End: told to the test's starter.
#[derive(Debug, Plain)]
#[kind(name = "aether.test.leave")]
struct Leave;

/// An actor a program is started for, as the hub's spawn will be, which ends when told.
#[derive(Bound)]
#[kind(name = "aether.test.starter")]
struct Starter;

impl ChildOf for Starter {
    type Parent = Top;
}

impl Actor for Starter {
    type Config = StarterConfig;
    type Key = ();
    type Handles = (Exited, Leave);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: StarterConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Exited> for Starter {
    type Sends = ();

    fn handle(&mut self, _: Exited, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Leave> for Starter {
    type Sends = ();

    fn handle(&mut self, _: Leave, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        turn.close();

        Ok(())
    }
}

#[test]
fn a_starter_that_ends_stops_its_program() -> Result<(), Unheard> {
    // Catches a process that does not watch its starter, or watches it and does nothing when it ends, so the program
    // outlives the actor that wanted it: it lingers, and only a stop ends it within the harness's budget.
    let mut harness = processes(settings(60_000, 4096))?;
    harness.add_root::<Starter>(&(), StarterConfig)?;
    let starter = harness.root::<Starter>();
    let Started::Running { process } =
        start(&mut harness, this_binary(), args(Behaviour::Lingers, &[]), starter.protocol())?
    else {
        panic!("this binary starts");
    };

    harness.tell(starter, &Leave)?;
    waiting::until(BUDGET, || {
        harness.step();
        harness.state(process).is_err()
    });

    assert!(harness.state(process).is_err(), "the process ended, so its program exited");

    Ok(())
}

#[test]
fn a_missing_program_is_refused() -> Result<(), Unheard> {
    // Catches a start answered running before the host has started anything, and a failed start never answered.
    let dir = Dir::new("missing");
    let mut harness = processes(settings(60_000, 4096))?;
    let outside = harness.protocol::<Exits>();

    let started = start(&mut harness, dir.path("no-such-program").display().to_string(), Vec::new(), outside)?;

    assert!(matches!(started, Started::Refused), "a missing program is refused: {started:?}");

    Ok(())
}
