//! The hub spawning and terminating its engines on the native host (ADR-0040, ADR-0036): a real hub, its store, its
//! processes, and `Engines`, asked through the harness. Each engine spawned is this test binary itself, checked into
//! the store and started from its launch copy with the one test, `program`, picked, and told what to do by a
//! [`Behaviour`], which it reads back from its arguments: one that links runs a real `Link` with the arguments the hub
//! appended. An engine started by hand is a real `Link` in a harness of its own.
#![cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]

use aethereal_actor::{ActorRef, Answered, ProtocolRef};
use aethereal_chassis_headless::Headless;
use aethereal_chassis_headless::native::{Invocation, Invoked, NativeLayers};
use aethereal_chassis_headless::waiting::{self, BUDGET};
use aethereal_data::{Budget, Plain, from_bytes, to_bytes};
use aethereal_link::{
    Down, EngineList, EngineListing, EngineTools, EngineToolsConfig, Engines, EnginesSettings, ExitedEngine, Link,
    LinkSettings, ListEngines, MintToken, Minted, Origin, REVISION, Refusal, Spawn, SpawnEngine, SpawnedEngine,
    TerminateEngine, Terminated, Token, Up,
};
use aethereal_process::{ExitStatus, Processes, ProcessesSettings};
use aethereal_store::{Binary, CheckIn, CheckedIn, Store, StoreHash, StoreName, StoreSettings, StoredBinary};
use aethereal_substrate::harness::{Harness, Unheard};
use aethereal_substrate::{EngineSettings, Host, Lifecycle, Program};
use aethereal_tcp::Tcp;
use aethereal_tools::{Call, Called, Done, Tooling};
use std::io::{self, Read as _, Write as _};
use std::net::TcpStream;
use std::path::PathBuf;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use std::{env, fs, iter, process, str};

/// The most bytes of a frame, each way, at both ends.
const MOST_BYTES: u32 = 64 * 1024;

/// What a spawned program this test starts does: one of a few fixed behaviours, each named by a token among its
/// arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Behaviour {
    /// Runs a real `Link` with the link's arguments the hub appended, so it links, and ends when its link does.
    Links,
    /// Writes [`GAVE_UP`] to its stderr, and exits with code 3, never dialing the hub.
    ExitsEarly,
    /// Runs until something ends it, never dialing the hub.
    Lingers,
}

/// What a program that exits early writes to its stderr.
const GAVE_UP: &str = "the engine gave up before it dialed";

/// What a token among a program's arguments begins with.
const BEHAVES: &str = "aethereal-behaves=";
/// What the argument naming the file a program writes its `--hub-token` to begins with.
const TOKEN_FILE: &str = "aethereal-token-file=";
/// The name the tests check this binary into the store under.
const SPAWNEE: &str = "spawnee";

impl Behaviour {
    const ALL: [Self; 3] = [Self::Links, Self::ExitsEarly, Self::Lingers];

    /// The name its token carries.
    const fn name(self) -> &'static str {
        match self {
            Self::Links => "links",
            Self::ExitsEarly => "exits-early",
            Self::Lingers => "lingers",
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

/// The test the program runs when spawned: it writes the token the hub started it with to the file its arguments
/// name, then does as its behaviour says and exits, or lingers, never returning to the test harness; in a run of the
/// suite, where no behaviour is named, it does nothing. The hub's own arguments come after `--`, which the test
/// harness takes as filters that match no test.
#[test]
fn program() {
    let args: Vec<String> = env::args().collect();
    let Some(behaviour) = Behaviour::among(&args) else {
        return;
    };
    let link: Vec<String> = args.iter().skip_while(|arg| arg.as_str() != "--").skip(1).cloned().collect();
    let token = link.iter().skip_while(|arg| arg.as_str() != "--hub-token").nth(1);
    let file = args.iter().find_map(|arg| arg.strip_prefix(TOKEN_FILE));
    if let (Some(token), Some(file)) = (token, file) {
        let _ = fs::write(file, token);
    }
    match behaviour {
        Behaviour::Links => {
            let ran = link_program(&link).map(|program| program.run().is_ok());
            let failed = ran != Some(true);
            process::exit(i32::from(failed));
        }
        Behaviour::ExitsEarly => {
            let _ = io::stderr().write_all(GAVE_UP.as_bytes());
            process::exit(3);
        }
        Behaviour::Lingers => loop {
            thread::park();
        },
    }
}

/// The engine both the hub and each engine run on: it reads a frame whole.
fn engine() -> EngineSettings {
    EngineSettings { read_limit: MOST_BYTES, mail_bytes: 1 << 24, ..EngineSettings::default() }
}

/// A program of a `Link` alone, over `Tcp`.
type LinkProgram = Program<Headless, (Link, (Tcp, ()))>;

/// An engine of a `Link` alone, read from `args` as a person would type them after its name: `None` when they do not
/// read.
fn link_program(args: &[String]) -> Option<LinkProgram> {
    let link = LinkSettings { most_bytes: MOST_BYTES, ..LinkSettings::default() };
    let program = Headless::program().with_engine(engine()).root::<Tcp>().root_with::<Link>(link);
    let invocation =
        Invocation::new(iter::once("engine").chain(args.iter().map(String::as_str)), Vec::<(String, String)>::new());
    let Ok(Invoked::Run(program)) = program.read("engine", invocation) else {
        return None;
    };

    Some(program)
}

/// A directory for one test, named for it, removed when dropped: the store kept beneath `store/`, and the file a
/// spawned program writes its token to.
struct Dir(PathBuf);

impl Dir {
    /// An empty directory for `test`.
    fn new(test: &str) -> Self {
        let root = env::temp_dir().join(format!("aethereal-spawn-{}-{test}", process::id()));
        let _ = fs::remove_dir_all(&root);
        assert!(fs::create_dir_all(&root).is_ok(), "the test's directory is made");

        Self(root)
    }

    /// The store's settings, kept here, with room for this binary.
    fn store(&self) -> StoreSettings {
        StoreSettings {
            root: self.0.join("store").display().to_string(),
            most_bytes: 1 << 36,
            most_file_bytes: 1 << 34,
        }
    }

    /// The file a spawned program writes its token to.
    fn token_file(&self) -> PathBuf {
        self.0.join("token")
    }

    /// The token a spawned program wrote, once it has.
    fn token(&self) -> Token {
        let file = self.token_file();
        waiting::until(BUDGET, || fs::read_to_string(&file).is_ok_and(|text| text.len() == 32));
        let Ok(text) = fs::read_to_string(&file) else {
            panic!("the program wrote its token");
        };
        let mut bytes = [0; 16];
        for (byte, pair) in bytes.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
            let digits = str::from_utf8(pair).ok().and_then(|pair| u8::from_str_radix(pair, 16).ok());
            let Some(digits) = digits else {
                panic!("the token is written in hexadecimal: {text}");
            };
            *byte = digits;
        }

        Token(bytes)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// This test binary, as a file to check in.
fn this_binary() -> String {
    let Ok(binary) = env::current_exe() else {
        panic!("the test knows its own binary");
    };

    binary.display().to_string()
}

/// The binary the tests check in, by its name.
fn spawnee() -> Binary {
    let Ok(name) = StoreName::parse(SPAWNEE) else {
        panic!("the tests' name conforms");
    };

    Binary::Named(name)
}

/// The arguments that start this binary's `program` test alone, behaving as `behaviour`, writing its token into `dir`,
/// and ending in `--`, so the arguments the hub appends after them are filters that match no test.
fn args(behaviour: Behaviour, dir: &Dir) -> Vec<String> {
    let mut args: Vec<String> = ["--exact", "program", "--nocapture", "--test-threads=1"].map(String::from).into();
    args.push(behaviour.token());
    args.push(format!("{TOKEN_FILE}{}", dir.token_file().display()));
    args.push(String::from("--"));

    args
}

/// A hub: `Tcp`, the store kept in `dir`, `Processes`, `Engines` on a port of its choosing giving each engine it
/// spawns `link_within_millis` to link, and `EngineTools`; this binary checked in under [`SPAWNEE`]. Its port, and the
/// hash this binary was stored by.
fn hub(dir: &Dir, link_within_millis: u64) -> Result<(Harness<Headless>, u16, StoreHash), Unheard> {
    let engines = EnginesSettings { port: 0, most_bytes: MOST_BYTES, link_within_millis, ..EnginesSettings::default() };
    let processes = ProcessesSettings { kill_after_millis: 60_000, most_stderr_bytes: 4096 };
    let program = Headless::program()
        .with_engine(engine())
        .root::<Tcp>()
        .root_with::<Store>(dir.store())
        .root_with::<Processes>(processes)
        .root_with::<Engines>(engines);
    let mut harness = Harness::of(program)?;
    harness.add_root::<EngineTools>(&(), EngineToolsConfig)?;
    let engines = harness.root::<Engines>();
    waiting::until(BUDGET, || {
        harness.step();
        harness.state(engines).is_ok_and(|engines| engines.port().is_some())
    });
    let Some(port) = harness.state(engines)?.port() else {
        panic!("`Engines` listens");
    };

    let store = harness.root::<Store>();
    let Ok(name) = StoreName::parse(SPAWNEE) else {
        panic!("the tests' name conforms");
    };
    let asked = harness.ask(store, &CheckIn { path: this_binary(), name })?;
    let Answered::Yes(CheckedIn::Stored(StoredBinary { hash, .. })) = harness.answer(asked)? else {
        panic!("this binary is checked in");
    };

    Ok((harness, port, hash))
}

/// What spawning `binary` with `args` comes to.
fn spawn(harness: &mut Harness<Headless>, binary: Binary, args: Vec<String>) -> Result<Spawn, Unheard> {
    let engines = harness.root::<Engines>();
    let asked = harness.ask(engines, &SpawnEngine { binary, args })?;
    let Answered::Yes(spawn) = harness.answer(asked)? else {
        panic!("`Engines` answers a spawn");
    };

    Ok(spawn)
}

/// What terminating engine `engine` comes to.
fn terminate(harness: &mut Harness<Headless>, engine: u32) -> Result<Terminated, Unheard> {
    let engines = harness.root::<Engines>();
    let asked = harness.ask(engines, &TerminateEngine { engine })?;
    let Answered::Yes(terminated) = harness.answer(asked)? else {
        panic!("`Engines` answers a termination");
    };

    Ok(terminated)
}

/// What `Engines` lists.
fn listed(harness: &mut Harness<Headless>) -> Result<EngineList, Unheard> {
    let engines = harness.root::<Engines>();
    let listing = harness.ask(engines, &ListEngines)?;
    let Answered::Yes(list) = harness.answer(listing)? else {
        panic!("`Engines` answers its list");
    };

    Ok(list)
}

/// What `Engines` lists once `done` holds of it.
fn listed_once(harness: &mut Harness<Headless>, done: impl Fn(&EngineList) -> bool) -> Result<EngineList, Unheard> {
    waiting::until(BUDGET, || listed(harness).is_ok_and(|list| done(&list)));

    listed(harness)
}

/// Whether `list` holds no engine linked, and `exited` engines that have ended.
fn holds_exited(list: &EngineList, exited: usize) -> bool {
    let none_linked = list.engines.is_empty();
    let has_exited = list.exited.len() == exited;

    none_linked && has_exited
}

/// `value` as bytes.
fn bytes_of<T: Plain>(value: &T) -> Vec<u8> {
    let Ok(bytes) = to_bytes(value) else {
        panic!("the value encodes");
    };

    bytes
}

/// Writes `frame` to `stream` as the link frames it: its length, big-endian, then its bytes.
fn send<T: Plain>(stream: &mut TcpStream, frame: &T) {
    let bytes = bytes_of(frame);
    let Ok(length) = u32::try_from(bytes.len()) else {
        panic!("the frame has a length");
    };
    assert!(stream.write_all(&length.to_be_bytes()).is_ok(), "the length writes");
    assert!(stream.write_all(&bytes).is_ok(), "the frame writes");
}

/// The next frame on `stream`, read as `T`.
fn receive<T: Plain>(stream: &mut TcpStream) -> T {
    let mut prefix = [0; 4];
    assert!(stream.read_exact(&mut prefix).is_ok(), "a frame comes within ten seconds");
    let mut bytes = vec![0; usize::try_from(u32::from_be_bytes(prefix)).unwrap_or(usize::MAX)];
    assert!(stream.read_exact(&mut bytes).is_ok(), "the frame comes whole");
    let Ok(frame) = from_bytes(&bytes, &mut Budget::new(1 << 24)) else {
        panic!("the frame reads as what was expected");
    };

    frame
}

/// What `peer` came to, the harness stepping the host until it has.
fn heard<T>(harness: &mut Harness<Headless>, peer: JoinHandle<T>) -> T {
    while !peer.is_finished() {
        harness.step();
    }
    let Ok(heard) = peer.join() else {
        panic!("the socket's side finished");
    };

    heard
}

/// What the hub at `port` answers a `Hello` bringing `token`, and whether it then hangs up, the harness stepping the
/// host until it has.
fn refusal_of(harness: &mut Harness<Headless>, port: u16, token: Token) -> (Down, bool) {
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else {
        panic!("the engine's side connects");
    };
    assert!(stream.set_read_timeout(Some(Duration::from_secs(10))).is_ok(), "the read timeout sets");
    send(&mut stream, &Up::Hello { revision: REVISION, name: String::from("copy"), token });
    #[allow(clippy::disallowed_methods)] // The engine's side is a native thread of its own, outside the engine.
    let peer = thread::spawn(move || {
        let down: Down = receive(&mut stream);
        let mut byte = [0];
        let closed = stream.read(&mut byte).is_ok_and(|count| count == 0);

        (down, closed)
    });

    heard(harness, peer)
}

/// Whether the program `harness` steps has stopped, once it has, `hub` stepped alongside it.
fn stops(harness: &mut Harness<Headless>, hub: &mut Harness<Headless>) -> bool {
    waiting::until(BUDGET, || {
        harness.step();
        hub.step();
        harness.host().engine().lifecycle() == Lifecycle::Stopped
    });

    harness.host().engine().lifecycle() == Lifecycle::Stopped
}

#[test]
fn a_spawn_that_links_answers_its_engine_and_lists_it_as_spawned_from_its_binary_and_build() -> Result<(), Unheard> {
    // Catches the link's arguments not appended or built unlike `Minted`'s, so the program could not link; a spawn
    // answered before its engine linked, or with another id; and an engine listed as started by hand, or without the
    // binary it was spawned as or the hash of the build it runs.
    let dir = Dir::new("links");
    let (mut harness, _, hash) = hub(&dir, 60_000)?;

    let spawned = spawn(&mut harness, spawnee(), args(Behaviour::Links, &dir))?;
    let list = listed_once(&mut harness, |list| list.engines.len() == 1)?;

    assert_eq!(spawned, Spawn::Linked { engine: 0 });
    let origin = Origin::Spawned { binary: spawnee(), hash };
    assert_eq!(list.engines, vec![EngineListing { engine: 0, name: String::from("engine"), tools: 0, origin }]);

    Ok(())
}

#[test]
fn a_program_that_exits_before_it_links_answers_exited_and_its_token_goes_with_its_spawn() -> Result<(), Unheard> {
    // Catches an early exit left unanswered until the time to link runs out, or answered without its status or the
    // tail of its stderr; an exit not kept among the exited; and a spawn's token kept once its spawn has ended, which
    // any process that read it from the process list could link with.
    let dir = Dir::new("exits");
    let (mut harness, port, _) = hub(&dir, 60_000)?;

    let spawned = spawn(&mut harness, spawnee(), args(Behaviour::ExitsEarly, &dir))?;
    let list = listed_once(&mut harness, |list| holds_exited(list, 1))?;
    let (down, closed) = refusal_of(&mut harness, port, dir.token());

    let status = ExitStatus::Code(3);
    assert_eq!(spawned, Spawn::Exited { status, stderr: String::from(GAVE_UP) });
    assert_eq!(list.exited, vec![ExitedEngine { binary: spawnee(), status, stderr: String::from(GAVE_UP) }]);
    assert_eq!(down, Down::Refused(Refusal::Token));
    assert!(closed, "the hub hangs up once its refusal is sent");

    Ok(())
}

#[test]
fn a_program_that_never_links_answers_unlinked_and_is_stopped() -> Result<(), Unheard> {
    // Catches a spawn that waits on its program for good, and one answered `Unlinked` that leaves its program running:
    // it lingers, and only a stop ends it within the budget. The hub gives its engines no time at all to link, so the
    // time runs out whatever the machine's speed.
    let dir = Dir::new("unlinked");
    let (mut harness, _, _) = hub(&dir, 0)?;

    let spawned = spawn(&mut harness, spawnee(), args(Behaviour::Lingers, &dir))?;
    let list = listed_once(&mut harness, |list| holds_exited(list, 1))?;

    assert_eq!(spawned, Spawn::Unlinked);
    let status = list.exited.first().map(|exited| exited.status);
    assert_eq!(status, Some(ExitStatus::Signal(libc::SIGTERM)));

    Ok(())
}

#[test]
fn a_binary_the_store_does_not_hold_answers_not_stored() -> Result<(), Unheard> {
    // Catches a spawn the store could not launch answered as refused, or never answered, and a program started anyway.
    let dir = Dir::new("not-stored");
    let (mut harness, _, _) = hub(&dir, 60_000)?;
    let Ok(name) = StoreName::parse("never-checked-in") else {
        panic!("the name conforms");
    };

    let by_name = spawn(&mut harness, Binary::Named(name), args(Behaviour::Links, &dir))?;
    let by_hash = spawn(&mut harness, Binary::Hash(StoreHash([7; 32])), args(Behaviour::Links, &dir))?;

    assert_eq!(by_name, Spawn::NotStored);
    assert_eq!(by_hash, Spawn::NotStored);
    assert_eq!(listed(&mut harness)?.exited, Vec::new());

    Ok(())
}

#[test]
fn terminating_a_spawned_engine_stops_its_program_and_lists_it_as_exited() -> Result<(), Unheard> {
    // Catches a termination that asks nothing of the spawn, or a spawn that does not stop its process, so the engine
    // stays linked; and an engine that ended after it linked dropped from the list without a trace of how.
    let dir = Dir::new("terminate-spawned");
    let (mut harness, _, _) = hub(&dir, 60_000)?;
    let Spawn::Linked { engine } = spawn(&mut harness, spawnee(), args(Behaviour::Links, &dir))? else {
        panic!("the engine links");
    };

    let terminated = terminate(&mut harness, engine)?;
    let list = listed_once(&mut harness, |list| holds_exited(list, 1))?;

    assert_eq!(terminated, Terminated::Stopping);
    let exited: Vec<(Binary, ExitStatus)> =
        list.exited.into_iter().map(|exited| (exited.binary, exited.status)).collect();
    assert_eq!(exited, vec![(spawnee(), ExitStatus::Signal(libc::SIGTERM))]);

    Ok(())
}

/// An engine of a `Link` alone, started with `args`, in a harness of its own.
fn started(args: &[String]) -> Result<Harness<Headless>, Unheard> {
    let Some(program) = link_program(args) else {
        panic!("the engine reads the arguments it was started with");
    };

    Harness::of(program)
}

#[test]
fn terminating_an_engine_started_by_hand_shuts_its_link_and_it_ends() -> Result<(), Unheard> {
    // Catches a termination of an engine the hub holds no process of answered as though nothing could be done, or
    // answered `Stopping` without its link shut, so the engine runs on.
    let dir = Dir::new("terminate-external");
    let (mut hub, _, _) = hub(&dir, 60_000)?;
    let engines = hub.root::<Engines>();
    let minting = hub.ask(engines, &MintToken)?;
    let Answered::Yes(Minted { args, .. }) = hub.answer(minting)? else {
        panic!("`Engines` mints a token");
    };
    let mut engine = started(&args)?;
    waiting::until(BUDGET, || {
        engine.step();
        listed(&mut hub).is_ok_and(|list| list.engines.len() == 1)
    });

    let terminated = terminate(&mut hub, 0)?;

    assert_eq!(terminated, Terminated::Stopping);
    assert!(stops(&mut engine, &mut hub), "the engine stops once its link is shut");
    assert_eq!(listed_once(&mut hub, |list| list.engines.is_empty())?.engines, Vec::new());

    Ok(())
}

#[test]
fn terminating_an_id_no_engine_is_linked_under_answers_not_linked() -> Result<(), Unheard> {
    // Catches a termination answered `Stopping` for an engine that is not there, and one of a connection that has not
    // proven itself with a token, which would let the agent shut connections the hub has not placed.
    let dir = Dir::new("not-linked");
    let (mut harness, port, _) = hub(&dir, 60_000)?;
    let _unproven = TcpStream::connect(("127.0.0.1", port));
    let engines = harness.root::<Engines>();
    waiting::until(BUDGET, || {
        harness.step();
        harness.state(engines).is_ok_and(|engines| engines.engine(0).is_some())
    });

    let unproven = terminate(&mut harness, 0)?;
    let unknown = terminate(&mut harness, 7)?;

    assert_eq!(unproven, Terminated::NotLinked);
    assert_eq!(unknown, Terminated::NotLinked);

    Ok(())
}

#[test]
fn a_spawns_token_links_its_engine_once_and_is_refused_after() -> Result<(), Unheard> {
    // Catches a spawn's token left in the table once its engine linked, which a process that read it from the process
    // list could link with in the engine's place.
    let dir = Dir::new("token-reused");
    let (mut harness, port, _) = hub(&dir, 60_000)?;
    let spawned = spawn(&mut harness, spawnee(), args(Behaviour::Links, &dir))?;

    let (down, closed) = refusal_of(&mut harness, port, dir.token());

    assert_eq!(spawned, Spawn::Linked { engine: 0 });
    assert_eq!(down, Down::Refused(Refusal::Token));
    assert!(closed, "the hub hangs up once its refusal is sent");
    assert_eq!(listed(&mut harness)?.engines.len(), 1);

    Ok(())
}

/// The hub's tools, as the worker a holder would ask.
fn tools(harness: &mut Harness<Headless>) -> ProtocolRef<Tooling> {
    let tools: ActorRef<EngineTools> = harness.root::<EngineTools>();

    tools.protocol()
}

/// What the hub's tools answer a call of `tool` with `input`.
fn called<T: Plain>(harness: &mut Harness<Headless>, tool: &str, input: &T) -> Result<Answered<Called>, Unheard> {
    let worker = tools(harness);
    let asked = harness.ask(worker, &Call { tool: String::from(tool), input: bytes_of(input) })?;

    harness.answer(asked)
}

/// The output `called` holds, read as `T`.
fn output<T: Plain>(called: Answered<Called>) -> T {
    let Answered::Yes(Called::Output(bytes)) = called else {
        panic!("the tool answers an output: {called:?}");
    };
    let Ok(output) = from_bytes(&bytes, &mut Budget::new(1 << 20)) else {
        panic!("the output reads as the tool's output kind");
    };

    output
}

/// Whether `called` is a refusal whose reason says `says`.
fn refuses(called: &Answered<Called>, says: &str) -> bool {
    matches!(called, Answered::Yes(Called::Refused(why)) if why.contains(says))
}

#[test]
fn spawn_engine_answers_the_linked_engine_and_terminate_engine_answers_done_or_refuses() -> Result<(), Unheard> {
    // Catches the tools asking `Engines` nothing or something else, a linked spawn's id lost or answered as a refusal,
    // a failed spawn answered as an output or with a reason that does not say how it failed, and a termination of no
    // engine answered as done.
    let dir = Dir::new("tools");
    let (mut harness, _, _) = hub(&dir, 60_000)?;
    let Ok(missing) = StoreName::parse("never-checked-in") else {
        panic!("the name conforms");
    };

    let linked: SpawnedEngine = output(called(
        &mut harness,
        "spawn_engine",
        &SpawnEngine { binary: spawnee(), args: args(Behaviour::Links, &dir) },
    )?);
    let not_stored =
        called(&mut harness, "spawn_engine", &SpawnEngine { binary: Binary::Named(missing), args: Vec::new() })?;
    let exited = called(
        &mut harness,
        "spawn_engine",
        &SpawnEngine { binary: spawnee(), args: args(Behaviour::ExitsEarly, &dir) },
    )?;
    let unknown = called(&mut harness, "terminate_engine", &TerminateEngine { engine: 7 })?;
    let done: Done = output(called(&mut harness, "terminate_engine", &TerminateEngine { engine: linked.engine })?);

    assert_eq!(linked, SpawnedEngine { engine: 0 });
    assert!(refuses(&not_stored, "no binary"), "{not_stored:?}");
    assert!(refuses(&exited, "with code 3"), "{exited:?}");
    assert!(refuses(&exited, GAVE_UP), "{exited:?}");
    assert!(refuses(&unknown, "no engine 7"), "{unknown:?}");
    assert_eq!(done, Done {});

    Ok(())
}
