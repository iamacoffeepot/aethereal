//! Guests moving onto their kind's new code while the program runs (ADR-0054): the echo guest, and its second build,
//! `v2`, which a library root publishes as the head of a kind the echo makes. Each guest answers a probe with the
//! version of the code that handled it and what its state holds.
#![cfg(not(target_arch = "wasm32"))]

use aethereal_actor::{
    Actor, ActorRef, Answered, Beginning, Bytes, ChildOf, Ended, Ending, Entry, Fault, Given, Handle, Handles, How,
    Level, Publisher, Root, Top, Turn, Unanswered,
};
use aethereal_chassis_headless::guest::Runtime;
use aethereal_data::{ActorPath, Bound, Kind, KindName, PathSegment, Plain};
use aethereal_guest_echo as echo;
use aethereal_substrate::Engine;
use aethereal_substrate::harness::{Askable, Bench, Harness, Steps, Unheard};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

/// The echo guest the build script built.
const ECHO: &[u8] = include_bytes!(env!("AETHEREAL_GUEST_ECHO"));

/// The echo guest's second build, with its `v2` feature: the second version of the guests the tests move.
const ECHO_V2: &[u8] = include_bytes!(env!("AETHEREAL_GUEST_ECHO_V2"));

/// What the library is made from: the engine's one publisher.
#[derive(Bound)]
#[kind(name = "aether.test.shelves")]
struct Shelves {
    publisher: Given<Publisher>,
}

/// A word to the library to publish `blob` as the head of `kind`.
#[derive(Bound)]
#[kind(name = "aether.test.publish")]
struct Publish {
    kind: KindName,
    blob: Bytes,
}

/// Why the library did not publish: the engine refused it.
#[derive(Plain)]
#[kind(name = "aether.test.unpublished")]
struct Unpublished;

/// A native root that holds the engine's publisher and publishes what it is told to.
#[derive(Bound)]
#[kind(name = "aether.test.library")]
struct Library {
    publisher: Handle<Publisher>,
}

impl ChildOf for Library {
    type Parent = Top;
}

impl Actor for Library {
    type Config = Shelves;
    type Key = ();
    type Handles = (Publish,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(shelves: Shelves, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self { publisher: turn.take(shelves.publisher)? })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Publish> for Library {
    type Sends = ();

    fn handle(&mut self, publish: Publish, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let published = turn.publish(&self.publisher, &publish.kind, &publish.blob);
        let Ok(()) = published else {
            return Err(Fault::new(&Unpublished)?);
        };

        Ok(())
    }
}

/// What a lookout is made from: the path of the actor it watches.
#[derive(Plain)]
#[kind(name = "aether.test.looks")]
struct Looks {
    at: ActorPath,
}

/// How each actor the lookout watches ended, as it was shown: locked because the test reads it from its own thread,
/// between the steps whose turns write it.
static SEEN: Mutex<Vec<How>> = Mutex::new(Vec::new());

/// A native root that watches the actor at its config's path, and keeps how it ended in [`SEEN`].
#[derive(Bound)]
#[kind(name = "aether.test.lookout")]
struct Lookout;

impl ChildOf for Lookout {
    type Parent = Top;
}

impl Actor for Lookout {
    type Config = Looks;
    type Key = ();
    type Handles = (Ended,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(looks: Looks, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        if let Some(found) = turn.resolve(&looks.at) {
            turn.watch(found);
        }

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ended> for Lookout {
    type Sends = ();

    fn handle(&mut self, ended: Ended, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        if let Ok(mut seen) = SEEN.lock() {
            seen.push(ended.how().clone());
        }

        Ok(())
    }
}

/// A program a test runs: its harness, its guest root, the library, and the blobs of the echo's second build.
struct Program<A> {
    harness: Harness<Bench>,
    guest: ActorRef<A>,
    library: ActorRef<Library>,
    second: Vec<Bytes>,
}

/// A program on two parts that runs guests with wasmtime: the echo put as code, the guest root `A` made from it with
/// `config`, the library holding the engine's publisher, and `seconds` blobs of the echo's second build put as code,
/// each its own blob, for the library to publish. Its harness, the root, the library, and the second build's blobs.
fn program<A: Root + Actor<Key = ()>>(config: &A::Config, seconds: usize) -> Result<Program<A>, Unheard> {
    let Some(parts) = NonZeroUsize::new(2) else {
        panic!("two is not zero");
    };
    let mut engine = Engine::new(parts, 1024, 1 << 20, 1 << 23, 64).with_guests(Arc::new(Runtime::default())).ready();
    let Ok(first) = engine.put_code(ECHO.to_vec()) else {
        panic!("the echo guest is fit to run");
    };
    let mut second = Vec::new();
    for _ in 0..seconds {
        let Ok(code) = engine.put_code(ECHO_V2.to_vec()) else {
            panic!("the echo guest's second build is fit to run");
        };
        second.push(code);
    }
    engine.add_root_from::<A>(&first, &(), config)?;
    let Ok(publisher) = engine.publisher() else {
        panic!("the engine hands out its publisher once");
    };
    engine.add_root::<Library>(&(), Shelves { publisher })?;
    let mut harness = Harness::bare(engine)?;
    let guest = harness.root::<A>();
    let library = harness.root::<Library>();

    Ok(Program { harness, guest, library, second })
}

/// What became of a probe: its report, or why none came.
fn report(answered: Answered<echo::Report>) -> Result<echo::Report, Unanswered> {
    match answered {
        Answered::Yes(report) => Ok(report),
        Answered::No(why) => Err(why),
    }
}

/// Probes `guest`, and answers with what became of the probe once it has.
fn probe<A>(harness: &mut Harness<Bench>, guest: ActorRef<A>) -> Result<Result<echo::Report, Unanswered>, Unheard>
where
    ActorRef<A>: Askable<echo::Probe, Answer = echo::Report>,
{
    let asked = harness.ask(guest, &echo::Probe)?;

    harness.answer(asked).map(report)
}

/// The lines `actor` has logged at `level`, oldest first.
fn lines<A>(harness: &mut Harness<Bench>, actor: ActorRef<A>, level: Level) -> Vec<String> {
    let logged = harness.host().engine().log(actor.seated());
    let lines = logged.filter_map(|entry| match entry {
        Entry::Line { level: at, text } if *at == level => Some(text.clone()),
        _ => None,
    });

    lines.collect()
}

#[test]
fn a_guest_that_answers_later_runs_its_turn_on_its_old_code_and_is_asked_again_before_the_next() -> Result<(), Unheard>
{
    // Catches a turn run on the new code though its actor answered `Later`, an actor never asked again once it has
    // answered `Later`, and a move that loses what the old code counted.
    let Program { mut harness, guest: mover, library, second } =
        program::<echo::Mover>(&echo::MoverConfig { holds: true }, 1)?;
    let blob = second[0].clone();
    harness.tell(library, &Publish { kind: echo::Mover::NAME, blob })?;
    harness.settle();

    let held = probe(&mut harness, mover)?.map(|report| (report.version, report.count));
    harness.tell(mover, &echo::LetGo)?;
    harness.settle();
    let after = probe(&mut harness, mover)?.map(|report| (report.version, report.count));

    assert_eq!(held, Ok((1, 1)), "the turn it answered `Later` before ran on its old code");
    assert_eq!(after, Ok((2, 2)), "asked again, it moved, and its count came with it");
    Ok(())
}

#[test]
fn a_guest_that_answers_now_moves_before_its_turn_and_the_new_code_handles_its_mail() -> Result<(), Unheard> {
    // Catches a batch that runs the turn an actor answered `Now` before on its old code, mail dropped where the batch
    // stopped, and a move that waits for a step with no mail.
    let Program { mut harness, guest: mover, library, second } =
        program::<echo::Mover>(&echo::MoverConfig { holds: true }, 1)?;
    let blob = second[0].clone();
    harness.tell(library, &Publish { kind: echo::Mover::NAME, blob })?;
    harness.settle();

    harness.tell(mover, &echo::LetGo)?;
    let asked = harness.ask(mover, &echo::Probe)?;
    let turned = harness.step_turns();
    let ran = turned.iter().filter(|turn| turn.actor == mover.seated()).count();
    let answered = harness.answer(asked).map(report)?.map(|report| (report.version, report.count));

    assert_eq!(ran, 1, "the letting go ran, and the probe waited for the new code");
    assert_eq!(answered, Ok((2, 1)), "the new code handled the probe that waited");
    Ok(())
}

#[test]
fn a_guest_whose_state_keeps_its_layout_is_taken_in_as_it_is() -> Result<(), Unheard> {
    // Catches a state of the same kind handed to `take_over`, which the keeper does not write, so its move would fail,
    // and a same-layout move that loses its state.
    let Program { mut harness, guest: keeper, library, second } =
        program::<echo::Keeper>(&echo::MoverConfig { holds: false }, 1)?;
    let before = probe(&mut harness, keeper)?.map(|report| (report.version, report.count));
    let blob = second[0].clone();
    harness.tell(library, &Publish { kind: echo::Keeper::NAME, blob })?;
    harness.settle();

    let after = probe(&mut harness, keeper)?.map(|report| (report.version, report.count));

    assert_eq!(before, Ok((1, 1)));
    assert_eq!(after, Ok((2, 2)), "it moved with its count, through no `take_over`");
    assert_eq!(lines(&mut harness, keeper, Level::Info), ["moved to publish 1; absent: none"]);
    Ok(())
}

#[test]
fn a_changed_layout_is_read_by_field_name_and_a_field_the_old_state_lacks_is_absent() -> Result<(), Unheard> {
    // Catches a field read by its place instead of its name, after the second version reordered them; a field the old
    // state lacks read as present; and a move that does not log what was absent.
    let Program { mut harness, guest: mover, library, second } =
        program::<echo::Mover>(&echo::MoverConfig { holds: false }, 1)?;
    let before = probe(&mut harness, mover)?.map(|report| (report.version, report.count, report.first_move));
    let blob = second[0].clone();
    harness.tell(library, &Publish { kind: echo::Mover::NAME, blob })?;
    harness.settle();

    let after = probe(&mut harness, mover)?.map(|report| (report.version, report.count, report.first_move));

    assert_eq!(before, Ok((1, 1, false)));
    assert_eq!(after, Ok((2, 2, true)), "its count read by name, and the field the first version lacks absent");
    assert_eq!(lines(&mut harness, mover, Level::Info), ["moved to publish 1; absent: first_move"]);
    Ok(())
}

#[test]
fn a_failing_take_over_leaves_the_guest_on_its_old_code_parked_until_the_next_publish() -> Result<(), Unheard> {
    // Catches a failed move that loses the state or leaves the actor on no code, an actor asked again every turn
    // after it failed, and one never asked again once its kind's head moves on.
    let Program { mut harness, guest: stubborn, library, second } =
        program::<echo::Stubborn>(&echo::MoverConfig { holds: false }, 2)?;
    let first = second[0].clone();
    harness.tell(library, &Publish { kind: echo::Stubborn::NAME, blob: first })?;
    harness.settle();

    let seen = |report: echo::Report| (report.version, report.count, report.asked);
    let parked = probe(&mut harness, stubborn)?.map(seen);
    let again = probe(&mut harness, stubborn)?.map(seen);
    let next = second[1].clone();
    harness.tell(library, &Publish { kind: echo::Stubborn::NAME, blob: next })?;
    harness.settle();
    let unparked = probe(&mut harness, stubborn)?.map(seen);

    assert_eq!(parked, Ok((1, 1, 1)), "on its old code with its state, asked once");
    assert_eq!(again, Ok((1, 2, 1)), "parked: not asked before its turns");
    assert_eq!(unparked, Ok((1, 3, 2)), "asked again once its kind's head moved on");
    let errors = lines(&mut harness, stubborn, Level::Error);
    let expected = [
        "did not move to publish 1: the old state has no `answered`; parked",
        "did not move to publish 2: the old state has no `answered`; parked",
    ];
    assert_eq!(errors, expected);
    Ok(())
}

#[test]
fn an_idle_guest_behind_its_head_is_asked_once() -> Result<(), Unheard> {
    // Catches an idle actor never asked, so a publish would never settle, and one asked again every step while it has
    // no mail.
    let Program { mut harness, guest: mover, library, second } =
        program::<echo::Mover>(&echo::MoverConfig { holds: true }, 1)?;
    let blob = second[0].clone();
    harness.tell(library, &Publish { kind: echo::Mover::NAME, blob })?;
    harness.settle();
    for _ in 0..4 {
        harness.step();
    }

    let report = probe(&mut harness, mover)?.map(|report| (report.version, report.asked));

    assert_eq!(report, Ok((1, 2)), "asked once idle, and once before this turn");
    Ok(())
}

#[test]
fn a_moved_guest_keeps_its_path_its_watchers_and_the_references_it_holds() -> Result<(), Unheard> {
    // Catches a move that ends the actor and begins another, which its watchers would be shown; one that gives it
    // another path; and one that drops the references in its state, whose blobs it could then not read.
    let Program { mut harness, guest: mover, library, second } =
        program::<echo::Mover>(&echo::MoverConfig { holds: false }, 1)?;
    let path = ActorPath::from_root(&PathSegment::new(echo::Mover::NAME, &()));
    harness.add_root::<Lookout>(&(), Looks { at: path.clone() })?;
    let kept = harness.put(vec![7; 300])?;
    harness.tell(mover, &echo::Keep { blob: kept })?;
    let blob = second[0].clone();
    harness.tell(library, &Publish { kind: echo::Mover::NAME, blob })?;
    harness.settle();

    let report = probe(&mut harness, mover)?.map(|report| (report.version, report.kept));
    let resolved = harness.host().engine().table().resolve(&path);
    let seen_moving = SEEN.lock().map(|seen| seen.clone()).unwrap_or_default();
    harness.tell(mover, &echo::Shut)?;
    harness.settle();
    let seen_closing = SEEN.lock().map(|seen| seen.clone()).unwrap_or_default();

    assert_eq!(report, Ok((2, 300)), "the new code reads the blob its state refers to");
    assert_eq!(resolved, Some(mover.seated().id), "its path still names it");
    assert_eq!(seen_moving, [], "moving is not ending");
    assert_eq!(seen_closing, [How::Closed], "the watch made on its old code sees it close on its new");
    Ok(())
}
