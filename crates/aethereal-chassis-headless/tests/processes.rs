//! The process capability on the native host (ADR-0040, ADR-0036): a real program, the root asked through the harness,
//! and its launcher the real one, starting real programs. Each program is this test binary itself, run again with the
//! one test, `program`, picked, and told what to do by a [`Behaviour`], which it reads back from its arguments. A
//! program whose stdin or stdout is piped is a system one, `cat` or `sh`, read by a [`Reader`].
#![cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]

use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anyone, Beginning, ChildOf, Ending, Fault, Handles, Number, Part, ProtocolRef,
    Receives, Streams, Top, Turn,
};
use aethereal_chassis_headless::Headless;
use aethereal_chassis_headless::waiting::{self, BUDGET};
use aethereal_data::{Bound, Plain};
use aethereal_process::{
    Environment, ExitStatus, Exited, Exits, Pipe, Piped, Process, Processes, ProcessesSettings, Start, Started, Stdio,
    Stop, Variable,
};
use aethereal_substrate::harness::{Harness, Unheard};
use aethereal_substrate::{Bootstrap, Bootstrapping, EngineSettings, Host, Unbuilt};
use aethereal_tcp::{Finish, HungUp, Read, Reading, Write, Writing, Wrote};
use std::io::{self, Write as _};
use std::num::NonZeroU32;
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

/// A start of `program` with `args`, its environment the host's and its stdin and stdout closed, its exit told to
/// `starter`.
fn closed(program: String, args: Vec<String>, starter: ProtocolRef<Exits>) -> Start {
    Start { program, args, environment: Environment::Inherited, stdin: Stdio::Closed, stdout: Stdio::Closed, starter }
}

/// What `start` comes to.
fn start(harness: &mut Harness<Headless>, start: &Start) -> Result<Started, Unheard> {
    let root = harness.root::<Processes>();
    let asked = harness.ask(root, start)?;
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
    let Started::Running { process, .. } = start(harness, &closed(this_binary(), args(behaviour, extra), starter))?
    else {
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
    type Depends = ();
    type Spawns = ();

    fn begin(_: StarterConfig, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Exited> for Starter {
    type From = Anyone;
    type Returns = ();

    fn handle(&mut self, _: Exited, _: &mut Turn<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Leave> for Starter {
    type From = Anyone;
    type Returns = ();

    fn handle(&mut self, _: Leave, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
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
    let Started::Running { process, .. } =
        start(&mut harness, &closed(this_binary(), args(Behaviour::Lingers, &[]), starter.protocol()))?
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

    let started = start(&mut harness, &closed(dir.path("no-such-program").display().to_string(), Vec::new(), outside))?;

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

/// How many parts of a pipe's stream a reader is kept ahead of it.
const WINDOW: u32 = 4;

/// What the test tells its reader: read this pipe.
#[derive(Bound)]
#[kind(name = "aether.test.processes.follow")]
struct Follow(ActorRef<Pipe>);

/// A root that reads the pipe the test hands it, and keeps every byte and how the pipe hung up: a stream is opened and
/// its parts taken only by [`Streams`], which only an actor holds.
#[derive(Bound)]
#[kind(name = "aether.test.processes.reader")]
struct Reader {
    reading: Streams<Reading, ()>,
    bytes: Vec<u8>,
    hung_up: Vec<HungUp>,
}

impl ChildOf for Reader {
    type Parent = Top;
}

impl Actor for Reader {
    type Config = StarterConfig;
    type Key = ();
    type Handles = (Follow, Part<Reading>, Answer<HungUp, Number>);
    type Depends = ();
    type Spawns = ();

    fn begin(_: StarterConfig, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let Some(window) = NonZeroU32::new(WINDOW) else {
            return Err(Fault::new(&StarterConfig)?);
        };

        Ok(Self { reading: Streams::new(window), bytes: Vec::new(), hung_up: Vec::new() })
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Bootstrap<Headless> for Reader {
    const SECTION: &'static str = "reader";
    type Settings = ();
    type Needs = ();
    type Provides = ();

    async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, Headless, Self>) -> Result<(), Unbuilt> {
        cx.begin(&(), StarterConfig)?;

        Ok(())
    }
}

impl Handles<Follow> for Reader {
    type From = Anyone;
    type Returns = ();

    fn handle(&mut self, Follow(pipe): Follow, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.reading.open(turn, pipe.protocol(), Read, ())?;

        Ok(())
    }
}

impl Handles<Part<Reading>> for Reader {
    type From = Anyone;
    type Returns = ();

    fn handle(&mut self, part: Part<Reading>, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Some(taken) = self.reading.part(turn, part)? else {
            return Ok(());
        };
        self.bytes.extend_from_slice(turn.read(&taken.value.bytes));

        Ok(())
    }
}

impl Receives<Answer<HungUp, Number>> for Reader {
    fn handle(&mut self, (answered, number): (Answered<HungUp>, Number), _: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.reading.answered(number);
        if let Answered::Yes(hung_up) = answered {
            self.hung_up.push(hung_up);
        }

        Ok(())
    }
}

/// A program holding the process capability and a reader of pipes.
fn piping() -> Result<Harness<Headless>, Unheard> {
    let engine = EngineSettings { read_limit: 64 * 1024, ..EngineSettings::default() };

    Harness::of(Headless::program().with_engine(engine).root_with::<Processes>(settings(60_000)).root::<Reader>())
}

/// The process and the pipe of `start`, which connects the program's stdin, its stdout, or both.
fn piped(harness: &mut Harness<Headless>, start: &Start) -> Result<(ActorRef<Process>, ActorRef<Pipe>), Unheard> {
    let started = self::start(harness, start)?;
    let Started::Running { process, pipe: Piped::Connected(pipe) } = started else {
        panic!("{} starts with a pipe: {started:?}", start.program);
    };

    Ok((process, pipe))
}

/// What the reader has read once its pipe has hung up, and how it did.
fn read_to_the_end(harness: &mut Harness<Headless>) -> Result<(Vec<u8>, Vec<HungUp>), Unheard> {
    let reader = harness.root::<Reader>();
    waiting::until(BUDGET, || {
        harness.step();
        harness.state(reader).is_ok_and(|state| !state.hung_up.is_empty())
    });
    let state = harness.state(reader)?;

    Ok((state.bytes.clone(), state.hung_up.clone()))
}

/// What `sh` prints of the variable `AETHEREAL_GIVEN` and of `CARGO_PKG_NAME`, which the test's runner sets in the
/// test's own environment, started with `environment`.
fn printed(environment: Environment) -> Result<Vec<u8>, Unheard> {
    let mut harness = piping()?;
    let starter = harness.protocol::<Exits>();
    let args = ["-c", "printf %s \"$AETHEREAL_GIVEN/$CARGO_PKG_NAME\""].map(String::from).into();
    let start = Start {
        program: String::from("/bin/sh"),
        args,
        environment,
        stdin: Stdio::Closed,
        stdout: Stdio::Connected,
        starter,
    };
    let (_, pipe) = piped(&mut harness, &start)?;
    let reader = harness.root::<Reader>();
    harness.tell(reader, &Follow(pipe))?;

    let exited = harness.heard::<Exited>()?;
    let (printed, hung_up) = read_to_the_end(&mut harness)?;

    assert_eq!(exited.status, ExitStatus::Code(0), "sh printed: {}", exited.stderr);
    assert_eq!(hung_up, [HungUp::Peer]);

    Ok(printed)
}

#[test]
fn bytes_written_to_cat_come_back_and_finish_ends_its_stdin_before_the_pipe_is_read() -> Result<(), Unheard> {
    // Catches a stdin or stdout not wired to the pipe, so nothing comes back; a `Finish` that never reaches the program
    // as the end of its stdin, so `cat` never exits within the harness's budget; and a pipe that ends with its
    // program's process, so what the program wrote before it exited is lost to a reader that comes after.
    let mut harness = piping()?;
    let starter = harness.protocol::<Exits>();
    let start = Start {
        program: String::from("/bin/cat"),
        args: Vec::new(),
        environment: Environment::Inherited,
        stdin: Stdio::Connected,
        stdout: Stdio::Connected,
        starter,
    };
    let (process, pipe) = piped(&mut harness, &start)?;

    let parts = vec![harness.put(b"through ".to_vec())?, harness.put(b"cat".to_vec())?];
    let wrote = harness.ask(pipe.protocol::<Writing>(), &Write { parts })?;
    assert_eq!(harness.answer(wrote)?, Answered::Yes(Wrote::Sent));
    harness.tell(pipe.protocol::<Writing>(), &Finish)?;
    let exited = harness.heard::<Exited>()?;
    harness.settle();
    assert_eq!(exited.status, ExitStatus::Code(0), "cat read its stdin to its end: {}", exited.stderr);
    assert!(harness.state(process).is_err(), "the process has ended");

    let reader = harness.root::<Reader>();
    harness.tell(reader, &Follow(pipe))?;
    let (bytes, hung_up) = read_to_the_end(&mut harness)?;

    assert_eq!(bytes, b"through cat");
    assert_eq!(hung_up, [HungUp::Peer], "the pipe reads to its end once the program has exited");

    Ok(())
}

#[test]
fn an_environment_of_only_given_variables_leaves_out_the_hosts_own() -> Result<(), Unheard> {
    // Catches `Only` that adds its variables to the host's rather than replacing them, one whose variables are never
    // set, and `Inherited` that passes the host's environment on no longer, as every engine the hub spawns needs it.
    let given = Variable { name: String::from("AETHEREAL_GIVEN"), value: String::from("given") };

    let only = printed(Environment::Only(vec![given]))?;
    let inherited = printed(Environment::Inherited)?;

    assert_eq!(only, b"given/", "only the given variable is set");
    assert_eq!(inherited, b"/aethereal-chassis-headless", "the host's own environment is passed on");

    Ok(())
}

#[test]
fn a_closed_stdin_reads_to_its_end_at_once_while_stdout_is_piped() -> Result<(), Unheard> {
    // Catches a closed stdin wired to the pipe all the same, or left as the engine's own, so `cat` waits on it and
    // never exits within the harness's budget; and a pipe that never hangs up once its program has exited.
    let mut harness = piping()?;
    let starter = harness.protocol::<Exits>();
    let start = Start {
        program: String::from("/bin/cat"),
        args: Vec::new(),
        environment: Environment::Inherited,
        stdin: Stdio::Closed,
        stdout: Stdio::Connected,
        starter,
    };
    let (_, pipe) = piped(&mut harness, &start)?;
    let reader = harness.root::<Reader>();
    harness.tell(reader, &Follow(pipe))?;

    let exited = harness.heard::<Exited>()?;
    let (bytes, hung_up) = read_to_the_end(&mut harness)?;

    assert_eq!(exited.status, ExitStatus::Code(0), "cat read an empty stdin: {}", exited.stderr);
    assert!(bytes.is_empty(), "cat had nothing to write: {bytes:?}");
    assert_eq!(hung_up, [HungUp::Peer]);

    Ok(())
}
