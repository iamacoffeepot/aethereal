//! The process capability on the native host (ADR-0040, ADR-0036): a real program, the root asked through the harness,
//! and its launcher the real one, starting real programs. Each program is this test binary itself, run again with the
//! one test, `program`, picked, and told what to do by a [`Behaviour`], which it reads back from its arguments.
#![cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]

use aethereal_actor::{Actor, ActorRef, Answered, Beginning, ChildOf, Ending, Fault, Handles, ProtocolRef, Top, Turn};
use aethereal_chassis_headless::Headless;
use aethereal_chassis_headless::waiting::{self, BUDGET};
use aethereal_data::{Bound, Plain};
use aethereal_process::{ExitStatus, Exited, Exits, Process, Processes, ProcessesSettings, Start, Started, Stop};
use aethereal_substrate::harness::{Harness, Unheard};
use aethereal_substrate::{EngineSettings, Host};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::{env, fs, process, thread};

/// What a program this test starts does: one of a few fixed behaviours, each named by a token among its arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Behaviour {
    /// Exits with code 3.
    ExitsThree,
    /// Writes [`FIRST`], then [`FLOOD`] lines, then [`LAST`], to its stderr, and exits with code 0.
    FloodsStderr,
    /// Runs until something ends it.
    Lingers,
    /// Ignores SIGTERM, writes the file its arguments name to say it does, and runs until something ends it.
    IgnoresTerm,
}

/// The first line a program that floods its stderr writes.
const FIRST: &str = "thread 'main' panicked at src/main.rs:1:1:";
/// How many lines it writes after it.
const FLOOD: usize = 1000;
/// The last line it writes.
const LAST: &str = "aborting";

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
            let flood = "  a frame\n".repeat(FLOOD);
            let written = io::stderr().write_all(format!("{FIRST}\n{flood}{LAST}\n").as_bytes());
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

/// A program holding the process capability as `settings` say, built and started, on an engine that reads an
/// `Exited` whole: a program's stderr is kept to a few kilobytes, past the engine's default read limit.
fn processes(settings: ProcessesSettings) -> Result<Harness<Headless>, Unheard> {
    let engine = EngineSettings { read_limit: 64 * 1024, ..EngineSettings::default() };

    Harness::of(Headless::program().with_engine(engine).root_with::<Processes>(settings))
}

/// The settings that give a program asked to stop `kill_after_millis`.
const fn settings(kill_after_millis: u32) -> ProcessesSettings {
    ProcessesSettings { kill_after_millis }
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
    let mut harness = processes(settings(60_000))?;
    let process = running(&mut harness, Behaviour::ExitsThree, &[])?;

    let exited = harness.heard::<Exited>()?;
    harness.settle();

    assert_eq!(exited.status, ExitStatus::Code(3));
    assert!(harness.state(process).is_err(), "the process has ended");

    Ok(())
}

#[test]
fn the_head_and_tail_of_stderr_are_kept_and_the_middle_left_out() -> Result<(), Unheard> {
    // Catches a launcher that keeps only the tail, so the first line is lost, stderr not piped or not read to its end
    // before the exit is told, and stdout taken for stderr. How the middle is cut is the kept stderr's own tests'.
    let mut harness = processes(settings(60_000))?;
    running(&mut harness, Behaviour::FloodsStderr, &[])?;

    let exited = harness.heard::<Exited>()?;

    let lines: Vec<&str> = exited.stderr.lines().collect();
    let left_out = lines.iter().any(|line| line.ends_with("left out …"));
    assert_eq!(exited.status, ExitStatus::Code(0));
    assert_eq!(lines.first(), Some(&FIRST));
    assert_eq!(lines.last(), Some(&LAST));
    assert!(left_out, "a line says what was left out: {}", exited.stderr);

    Ok(())
}

#[test]
fn stop_ends_a_program_with_sigterm() -> Result<(), Unheard> {
    // Catches a stop that kills at once (signal 9), one that sends nothing until its time has passed (the minute it is
    // given outlasts the harness's budget), and SIGTERM sent to anything but the program's group.
    let mut harness = processes(settings(60_000))?;
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
    let kill_after = Duration::from_millis(200);
    let mut harness = processes(settings(200))?;
    let process = running(&mut harness, Behaviour::IgnoresTerm, &[format!("{READY}{}", ready.display())])?;
    waiting::until(BUDGET, || Path::exists(&ready));
    assert!(ready.exists(), "the program ignores SIGTERM before it is stopped");

    #[allow(clippy::disallowed_methods)] // The program is killed on real time, which only bounds the wait from below.
    let stopped = Instant::now();
    harness.tell(process, &Stop)?;
    let exited = harness.heard::<Exited>()?;

    assert_eq!(exited.status, ExitStatus::Signal(libc::SIGKILL));
    assert!(stopped.elapsed() >= kill_after, "it was given its time before it was killed");

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
    let mut harness = processes(settings(60_000))?;
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
    let mut harness = processes(settings(60_000))?;
    let outside = harness.protocol::<Exits>();

    let started = start(&mut harness, dir.path("no-such-program").display().to_string(), Vec::new(), outside)?;

    assert!(matches!(started, Started::Refused), "a missing program is refused: {started:?}");

    Ok(())
}

#[test]
fn a_headless_program_listing_processes_names_each_flag_once() {
    // Catches a setting of the process capability whose flag the headless host's own section already names, as
    // `--stop-within-millis` once was: every program listing `Processes` would then stop before it started.
    let flags = Headless::program().root::<Processes>().flag_table();

    assert!(flags.is_ok(), "the program's flags are read: {flags:?}");
}
