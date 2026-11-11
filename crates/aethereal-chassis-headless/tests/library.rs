//! The library on the native host (ADR-0054): `Code` a directory of the echo guest's builds, published as the program
//! starts; defaults asked of the library as a system would; and its tools, asked as a holder would, publishing the
//! echo's second build over running guests and listing who is still behind. What the library published is read in the
//! engine's heads, and what it answered in the typed mail it answered.
#![cfg(not(target_arch = "wasm32"))]

use aethereal_actor::{Actor, ActorRef, Answered, Bytes, Entry, Level, ProtocolRef, Root};
use aethereal_chassis_headless::guest::Runtime;
use aethereal_chassis_headless::waiting::{self, BUDGET};
use aethereal_chassis_headless::{FileRoots, Headless};
use aethereal_data::{Budget, FilePath, Kind, KindName, Plain, from_bytes, to_bytes};
use aethereal_files::Files;
use aethereal_guest_echo as echo;
use aethereal_library::{
    Defaulted, Defaults, GuestStanding, HeadRecord, KindMoves, KindRecord, Library, LibraryTool, LibraryToolConfig,
    ParkedGuest, Publish, PublishRecord, ReadUpgrades, Upgrading, WaitingGuest,
};
use aethereal_substrate::harness::{Harness, Unheard};
use aethereal_substrate::{Head, Host, Lifecycle, Program};
use aethereal_tools::{Call, Called, Tool, Tooling};
use std::path::PathBuf;
use std::sync::Arc;
use std::{env, fs, process};

/// The echo guest the build script built.
const ECHO: &[u8] = include_bytes!(env!("AETHEREAL_GUEST_ECHO"));

/// The echo guest's second build, with its `echo_v2` cfg.
const ECHO_V2: &[u8] = include_bytes!(env!("AETHEREAL_GUEST_ECHO_V2"));

/// The most bytes reading a tool's answer takes.
const ANSWER_BYTES: usize = 1 << 20;

/// A directory for a program's files, named for `test`, removed when dropped: `Code` is its `code`, made only once a
/// module is put there.
struct Dir(PathBuf);

impl Dir {
    /// A directory whose `Code` holds `modules`, each a name in it and its bytes: none for a program with no `Code`.
    fn new(test: &str, modules: &[(&str, &[u8])]) -> Self {
        let dir = Self(env::temp_dir().join(format!("aethereal-library-{}-{test}", process::id())));
        assert!(fs::create_dir_all(&dir.0).is_ok(), "the directory is made");
        for (name, bytes) in modules {
            dir.put(name, bytes);
        }

        dir
    }

    /// Writes `bytes` as the module `name` in `Code`.
    fn put(&self, name: &str, bytes: &[u8]) {
        let code = self.0.join("code");
        assert!(fs::create_dir_all(&code).is_ok(), "`Code` is made");
        assert!(fs::write(code.join(name), bytes).is_ok(), "the module is written");
    }

    /// Where the file capability finds each namespace.
    fn roots(&self) -> FileRoots {
        let at = |name: &str| self.0.join(name).display().to_string();

        FileRoots { assets: at("assets"), save: at("save"), code: at("code"), readers: 2 }
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A program that runs guests, with `Files` reading `dir`, and the library: its harness, started, before any step.
fn program(dir: &Dir) -> Result<Harness<Headless>, Unheard> {
    let program: Program<Headless, _> =
        Headless::program().with_guests(Arc::new(Runtime::default())).root_with::<Files>(dir.roots()).root::<Library>();

    Harness::of(program)
}

/// Defaults of `modules` asked of the library, each put as a blob: answered with what it took of them.
fn defaults(harness: &mut Harness<Headless>, modules: &[&[u8]]) -> Result<Answered<Defaulted>, Unheard> {
    let library = harness.root::<Library>();
    let modules = modules.iter().map(|module| harness.put(module.to_vec())).collect::<Result<Vec<Bytes>, _>>()?;
    let asked = harness.ask(library, &Defaults { modules })?;

    harness.answer(asked)
}

/// The bytes of the head of the kind `kind`, as of the last step.
fn head(harness: &mut Harness<Headless>, kind: &KindName) -> Option<Vec<u8>> {
    let engine = harness.host().engine();
    let blob = engine.head(kind)?.blob().clone();

    engine.blob(&blob).map(<[u8]>::to_vec)
}

/// The lines the library has logged at `level`, oldest first.
fn library_lines(harness: &mut Harness<Headless>, level: Level) -> Vec<String> {
    let library = harness.root::<Library>().seated();
    let logged = harness.host().engine().log(library);
    let lines = logged.filter_map(|entry| match entry {
        Entry::Line { level: at, text } if *at == level => Some(text.clone()),
        _ => None,
    });

    lines.collect()
}

#[test]
fn every_module_in_code_is_published_as_the_program_starts_and_defaults_asked_meanwhile_replace_none()
-> Result<(), Unheard> {
    // Catches `Code` not read as the program starts, a file in it that is no module taken for one (the guests' host
    // would refuse it, and the program stop), and defaults asked before `Code` was published taking the heads `Code`
    // was to make, as a library that took them as they came would.
    let dir = Dir::new("starts", &[("echo.wasm", ECHO_V2), ("notes.txt", b"not a module")]);
    let mut harness = program(&dir)?;

    let defaulted = defaults(&mut harness, &[ECHO])?;

    let Answered::Yes(Defaulted::Taken { published, kept }) = defaulted else {
        panic!("the defaults are taken: {defaulted:?}");
    };
    assert_eq!(published, 0, "`Code` made every kind's head first");
    assert!(kept > 0);
    assert_eq!(head(&mut harness, &echo::Mover::NAME).as_deref(), Some(ECHO_V2), "`Code`'s module is the head");
    Ok(())
}

#[test]
fn a_default_fills_only_the_kinds_that_have_no_head() -> Result<(), Unheard> {
    // Catches a program with no `Code` that never opens, a default that leaves a kind with no head as it is, and a
    // second default that replaces the first's head.
    let dir = Dir::new("defaults", &[]);
    let mut harness = program(&dir)?;

    let first = defaults(&mut harness, &[ECHO])?;
    let second = defaults(&mut harness, &[ECHO_V2])?;

    let Answered::Yes(Defaulted::Taken { published, kept: 0 }) = first else {
        panic!("the first defaults are taken, keeping no head: {first:?}");
    };
    assert!(published > 0);
    assert_eq!(second, Answered::Yes(Defaulted::Taken { published: 0, kept: published }), "every kind had a head");
    assert_eq!(head(&mut harness, &echo::Mover::NAME).as_deref(), Some(ECHO), "the first default's head stands");
    Ok(())
}

#[test]
fn two_modules_in_code_that_make_one_kind_stop_the_program_naming_both() -> Result<(), Unheard> {
    // Catches one of two modules that make a kind chosen over the other, and a program that runs on without the code
    // it was built with, logging nothing that says which modules clash.
    let dir = Dir::new("clash", &[("a.wasm", ECHO), ("b.wasm", ECHO_V2)]);
    let mut harness = program(&dir)?;

    let defaulted = defaults(&mut harness, &[])?;
    waiting::until(BUDGET, || {
        harness.step();
        harness.host().engine().lifecycle() == Lifecycle::Stopped
    });

    assert_eq!(defaulted, Answered::Yes(Defaulted::Stopped));
    assert_eq!(harness.host().engine().lifecycle(), Lifecycle::Stopped, "the program stops");
    let errors = library_lines(&mut harness, Level::Error);
    let names_both = errors.iter().any(|line| line.contains("`a.wasm` and `b.wasm` in `Code` both make"));
    assert!(names_both, "{errors:?}");
    Ok(())
}

#[test]
fn a_module_in_code_the_guests_host_refuses_stops_the_program() -> Result<(), Unheard> {
    // Catches a module that cannot run passed over, so its kinds run no code of `Code`'s and nothing says why.
    let dir = Dir::new("unfit", &[("broken.wasm", b"\0asm, but no module")]);
    let mut harness = program(&dir)?;

    let defaulted = defaults(&mut harness, &[])?;

    assert_eq!(defaulted, Answered::Yes(Defaulted::Stopped));
    let errors = library_lines(&mut harness, Level::Error);
    let names_it = errors.iter().any(|line| line.contains("`broken.wasm` in `Code` is no module this engine runs"));
    assert!(names_it, "{errors:?}");
    Ok(())
}

/// A program whose `Code` holds the echo's first build, with the library's tool, and three guests of its kinds made
/// from their heads: a mover that holds and answers `Later`, a keeper that moves, and a stubborn one whose move fails.
/// Then the echo's second build is put in `Code` as `echo2.wasm`.
struct Running {
    harness: Harness<Headless>,
    tool: ProtocolRef<Tooling>,
    mover: ActorRef<echo::Mover>,
    stubborn: ActorRef<echo::Stubborn>,
    _dir: Dir,
}

/// The [`Running`] program, named for `test`.
fn running(test: &str) -> Result<Running, Unheard> {
    let dir = Dir::new(test, &[("echo.wasm", ECHO)]);
    let mut harness = program(&dir)?;
    let opened = defaults(&mut harness, &[])?;
    assert_eq!(opened, Answered::Yes(Defaulted::Taken { published: 0, kept: 0 }));
    harness.add_root::<LibraryTool>(&(), LibraryToolConfig)?;
    let tool = LibraryTool::worker(harness.root::<LibraryTool>());
    let Some(code) = harness.host().engine().head(&echo::Mover::NAME).map(|head| head.blob().clone()) else {
        panic!("`Code` made the echo's kinds' heads");
    };
    let mover = guest::<echo::Mover>(&mut harness, &code, true)?;
    guest::<echo::Keeper>(&mut harness, &code, false)?;
    let stubborn = guest::<echo::Stubborn>(&mut harness, &code, false)?;
    harness.settle();
    dir.put("echo2.wasm", ECHO_V2);

    Ok(Running { harness, tool, mover, stubborn, _dir: dir })
}

/// The guest root `A`, made from the blob `code`, holding as `holds` says.
fn guest<A>(harness: &mut Harness<Headless>, code: &Bytes, holds: bool) -> Result<ActorRef<A>, Unheard>
where
    A: Root + Actor<Key = (), Config = echo::MoverConfig>,
{
    harness.host().engine_mut().add_root_from::<A>(code, &(), &echo::MoverConfig { holds })?;

    Ok(harness.root::<A>())
}

/// Calls the library's tool `tool` with `input`, and reads what it answered as `O`.
fn call<I: Plain, O: Plain>(running: &mut Running, tool: &str, input: &I) -> Result<O, Unheard> {
    let input = running.harness.put(to_bytes(input)?)?;
    let asked = running.harness.ask(running.tool, &Call { tool: String::from(tool), input })?;
    let Answered::Yes(Called::Output(output)) = running.harness.answer(asked)? else {
        panic!("`{tool}` answers an output");
    };
    let Some(bytes) = running.harness.read(&output) else {
        panic!("the output's blob is held");
    };
    let Ok(answer) = from_bytes(bytes, &mut Budget::new(ANSWER_BYTES)) else {
        panic!("the output reads as `{tool}`'s answer");
    };

    Ok(answer)
}

/// The record of the kind `kind` in `record`.
fn of_kind<'r>(record: &'r PublishRecord, kind: &KindName) -> &'r KindRecord {
    let Some(found) = record.kinds.iter().find(|found| found.kind == *kind) else {
        panic!("the module makes `{kind}`");
    };

    found
}

/// The path the tools name the root `actor` by.
fn spoken<A>(actor: ActorRef<A>, harness: &mut Harness<Headless>) -> String {
    let Some(path) = harness.host().engine().table().name(actor.seated().id) else {
        panic!("the guest is live");
    };

    path.to_string()
}

/// `echo2.wasm` published with a wait long past what a healthy run takes.
fn publish_second(running: &mut Running) -> Result<PublishRecord, Unheard> {
    let publish = Publish { path: FilePath::new("echo2.wasm"), wait_millis: Some(30_000) };

    call(running, "publish", &publish)
}

#[test]
fn publish_answers_who_moved_who_answered_later_and_who_is_parked() -> Result<(), Unheard> {
    // Catches a publish that answers before every running guest of its kinds has answered; a move not counted, or
    // the field its old state lacked lost; a guest that answered `Later` or failed its move left out, or put under
    // another kind; and a library whose publish numbers drift from the engine's, which would read another publish's
    // record as its own.
    let mut running = running("publish")?;

    let record = publish_second(&mut running)?;

    let (mover, stubborn) = (running.mover, running.stubborn);
    let (mover_path, stubborn_path) = (spoken(mover, &mut running.harness), spoken(stubborn, &mut running.harness));
    let waiting = HeadRecord {
        moved: 0,
        absent: Vec::new(),
        waiting: vec![WaitingGuest { actor: mover_path, batches: 1 }],
        parked: Vec::new(),
        unasked: Vec::new(),
    };
    assert_eq!(of_kind(&record, &echo::Mover::NAME).moves, KindMoves::Head(waiting));
    let onto_head =
        HeadRecord { moved: 1, absent: Vec::new(), waiting: Vec::new(), parked: Vec::new(), unasked: Vec::new() };
    assert_eq!(of_kind(&record, &echo::Keeper::NAME).moves, KindMoves::Head(onto_head));
    let reason = String::from("the old state has no `answered`");
    let parked = HeadRecord {
        moved: 0,
        absent: Vec::new(),
        waiting: Vec::new(),
        parked: vec![ParkedGuest { actor: stubborn_path, reason }],
        unasked: Vec::new(),
    };
    assert_eq!(of_kind(&record, &echo::Stubborn::NAME).moves, KindMoves::Head(parked));
    let number = running.harness.host().engine().head(&echo::Mover::NAME).map(Head::number);
    assert_eq!(Some(of_kind(&record, &echo::Mover::NAME).publish), number, "the engine's number for the publish");

    running.harness.tell(mover, &echo::LetGo)?;
    running.harness.settle();
    let probed = running.harness.ask(mover, &echo::Probe)?;
    let Answered::Yes(report) = running.harness.answer(probed)? else {
        panic!("the mover answers its probe");
    };
    running.harness.settle();
    let standings = running.harness.host().engine().moves();
    let mover_head = standings.head(&echo::Mover::NAME).map(|head| (head.moved, head.absent.clone()));

    assert_eq!(report.version, 2, "let go, the mover moved onto the second build");
    assert_eq!(mover_head, Some((1, vec![String::from("first_move")])), "its move counted, with what it lacked");
    Ok(())
}

#[test]
fn upgrades_lists_each_guest_behind_its_head_with_its_batches_or_why_it_is_parked() -> Result<(), Unheard> {
    // Catches a guest that moved still listed, one that answered `Later` or is parked left out, and the batches it
    // answered `Later` before not counted batch by batch.
    let mut running = running("upgrades")?;
    let record = publish_second(&mut running)?;
    let mover = running.mover;
    let probed = running.harness.ask(mover, &echo::Probe)?;
    let _ = running.harness.answer(probed)?;
    running.harness.settle();

    let upgrading: Upgrading = call(&mut running, "upgrades", &ReadUpgrades {})?;

    let (mover_path, stubborn_path) =
        (spoken(running.mover, &mut running.harness), spoken(running.stubborn, &mut running.harness));
    let listed: Vec<(String, KindName, u64, GuestStanding)> =
        upgrading.guests.into_iter().map(|guest| (guest.actor, guest.kind, guest.publish, guest.standing)).collect();
    let (mover_publish, stubborn_publish) =
        (of_kind(&record, &echo::Mover::NAME).publish, of_kind(&record, &echo::Stubborn::NAME).publish);
    let reason = String::from("the old state has no `answered`");
    let expected = vec![
        (mover_path, echo::Mover::NAME, mover_publish, GuestStanding::Later { batches: 2 }),
        (stubborn_path, echo::Stubborn::NAME, stubborn_publish, GuestStanding::Parked { reason }),
    ];
    assert_eq!(listed, expected, "asked once with no mail as the head moved, and once before the probe's batch");
    Ok(())
}
