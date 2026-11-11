//! Guests moving onto their kind's new code while the program runs (ADR-0054): the echo guest, and its second build,
//! `v2`, which a library root publishes as the head of a kind the echo makes. Each guest answers a probe with the
//! version of the code that handled it and what its state holds; what each answered when the engine asked it whether
//! to move, and about which publish, is read in the engine's record of its asks.
#![cfg(not(target_arch = "wasm32"))]

use aethereal_actor::{
    Actor, ActorName, ActorRef, Answered, Beginning, Bytes, Carries, ChildOf, Ended, Ending, Entry, Fault, Given,
    Handle, Handles, How, Level, Path, Publisher, Root, Standing, Tells, ToSender, Top, Turn, Unanswered, Upgrade,
    path,
};
use aethereal_chassis_headless::guest::Runtime;
use aethereal_data::{ActorPath, Bound, Kind, KindName, PathSegment, Plain};
use aethereal_guest_echo as echo;
use aethereal_substrate::Engine;
use aethereal_substrate::harness::{Askable, Bench, Harness, Steps, Turned, Unheard};
use std::num::NonZeroUsize;
use std::sync::Arc;

/// The echo guest the build script built.
const ECHO: &[u8] = include_bytes!(env!("AETHEREAL_GUEST_ECHO"));

/// The echo guest's second build, with its `echo_v2` cfg: the second version of the guests the tests move.
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

/// A word to the library to publish `blob` as the head of the mover's kind, and in the same turn to tell the mover to
/// let go: the mover has mail as the head moves.
#[derive(Bound)]
#[kind(name = "aether.test.publish_letting_go")]
struct PublishLettingGo {
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
        let Ok(_) = published else {
            return Err(Fault::new(&Unpublished)?);
        };

        Ok(())
    }
}

/// A native root that holds the engine's publisher, publishes the mover's kind's head, and tells the mover to let go
/// in the same turn: a library of its own, as one that reaches the mover's path stands only where a mover does.
#[derive(Bound)]
#[kind(name = "aether.test.letting_go")]
struct LettingGo {
    publisher: Handle<Publisher>,
}

impl ChildOf for LettingGo {
    type Parent = Top;
}

impl Actor for LettingGo {
    type Config = Shelves;
    type Key = ();
    type Handles = (PublishLettingGo,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(shelves: Shelves, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self { publisher: turn.take(shelves.publisher)? })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<PublishLettingGo> for LettingGo {
    type Sends = (Tells<Path<echo::Mover>, echo::LetGo>,);

    fn handle(&mut self, publish: PublishLettingGo, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let published = turn.publish(&self.publisher, &echo::Mover::NAME, &publish.blob);
        let Ok(_) = published else {
            return Err(Fault::new(&Unpublished)?);
        };
        turn.to(path::<echo::Mover>(())).send(&echo::LetGo)?;

        Ok(())
    }
}

/// What a lookout is made from: the path of the actor it watches.
#[derive(Plain)]
#[kind(name = "aether.test.looks")]
struct Looks {
    at: ActorPath,
}

/// How an actor the lookout watched ended, as it was shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Sighting {
    Closed,
    Failed,
    Earlier,
}

impl From<&How> for Sighting {
    fn from(how: &How) -> Self {
        match how {
            How::Closed => Self::Closed,
            How::Failed(_) => Self::Failed,
            How::Earlier => Self::Earlier,
        }
    }
}

/// A question to the lookout: how has each actor it watches ended so far?
#[derive(Plain)]
#[kind(name = "aether.test.recall")]
struct Recall;

/// What the lookout answers a recall with: each ending it was shown, oldest first.
#[derive(Plain)]
#[kind(name = "aether.test.sightings")]
struct Sightings {
    endings: Vec<Sighting>,
}

/// A native root that watches the actor at its config's path, keeps how it ended, and answers a recall with it.
#[derive(Bound)]
#[kind(name = "aether.test.lookout")]
struct Lookout {
    seen: Vec<Sighting>,
}

impl ChildOf for Lookout {
    type Parent = Top;
}

impl Actor for Lookout {
    type Config = Looks;
    type Key = ();
    type Handles = (Ended, Recall);
    type BeginSends = ();
    type EndSends = ();

    fn begin(looks: Looks, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        if let Some(found) = turn.resolve(&looks.at) {
            turn.watch(found);
        }

        Ok(Self { seen: Vec::new() })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ended> for Lookout {
    type Sends = ();

    fn handle(&mut self, ended: Ended, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.seen.push(Sighting::from(ended.how()));

        Ok(())
    }
}

impl Carries<Recall> for Lookout {
    type Answers = (Sightings,);
}

impl Handles<Recall> for Lookout {
    type Sends = (ToSender<Sightings>,);

    fn handle(&mut self, _: Recall, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.reply(&Sightings { endings: self.seen.clone() })?;

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

/// What became of a recall: the endings the lookout was shown, or why it did not answer.
fn endings(answered: Answered<Sightings>) -> Result<Vec<Sighting>, Unanswered> {
    match answered {
        Answered::Yes(sightings) => Ok(sightings.endings),
        Answered::No(why) => Err(why),
    }
}

/// Asks `lookout` how each actor it watches has ended so far, and answers with what became of the recall once it has.
fn recall(
    harness: &mut Harness<Bench>,
    lookout: ActorRef<Lookout>,
) -> Result<Result<Vec<Sighting>, Unanswered>, Unheard> {
    let asked = harness.ask(lookout, &Recall)?;

    harness.answer(asked).map(endings)
}

/// What `actor` answered each time the engine asked it whether to move onto its kind's head, and the number of the
/// publish that made the head it was asked about, since the record was last taken, oldest first: every other actor's
/// asks are taken with them.
fn asks<A>(harness: &mut Harness<Bench>, actor: ActorRef<A>) -> Vec<(Upgrade, u64)> {
    let asked = harness.host().engine().take_asked_to_move();

    asked.iter().filter(|ask| ask.actor == actor.seated()).map(|ask| (ask.answer, ask.publish)).collect()
}

/// How many turns of `actor` are among `turned`.
fn turns_of<A>(turned: &[Turned], actor: ActorRef<A>) -> usize {
    turned.iter().filter(|turn| turn.actor == actor.seated()).count()
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

/// What a probe's report, `report`, says of the code that handled it and the count it kept: its version and count, or
/// why no report came.
fn handled(report: Result<echo::Report, Unanswered>) -> Result<(u64, u64), Unanswered> {
    report.map(|report| (report.version, report.count))
}

#[test]
fn a_guest_that_answers_later_runs_its_whole_batch_on_its_old_code_and_is_asked_again_at_its_next()
-> Result<(), Unheard> {
    // Catches an actor asked before each turn instead of once a batch, so it moves in the middle of one; a batch run on
    // the new code though its actor answered `Later`; an actor never asked again once it has answered `Later`; and a
    // move that loses what the old code counted.
    let Program { mut harness, guest: mover, library, second } =
        program::<echo::Mover>(&echo::MoverConfig { holds: true }, 1)?;
    let blob = second[0].clone();
    harness.tell(library, &Publish { kind: echo::Mover::NAME, blob })?;
    harness.settle();
    let idle = asks(&mut harness, mover);

    harness.tell(mover, &echo::LetGo)?;
    let held = harness.ask(mover, &echo::Probe)?;
    let held = handled(report(harness.answer(held)?));
    let holding = asks(&mut harness, mover);
    let after = handled(probe(&mut harness, mover)?);
    let moving = asks(&mut harness, mover);
    let on_head = handled(probe(&mut harness, mover)?);
    let asked_on_head = asks(&mut harness, mover);

    assert_eq!(idle, [(Upgrade::Later, 1)], "asked once as the head moved, with no mail");
    assert_eq!(held, Ok((1, 1)), "the probe after the letting go ran on the old code, in the batch asked `Later`");
    assert_eq!(holding, [(Upgrade::Later, 1)], "asked once for the batch, before the letting go");
    assert_eq!(after, Ok((2, 2)), "asked again at its next batch, it moved, and its count came with it");
    assert_eq!(moving, [(Upgrade::Now, 1)]);
    assert_eq!(on_head, Ok((2, 3)));
    assert_eq!(asked_on_head, [], "on its kind's head, it is not asked");
    Ok(())
}

#[test]
fn a_guest_that_answers_now_moves_and_its_batch_runs_on_the_new_code_in_the_same_step() -> Result<(), Unheard> {
    // Catches a batch run on the old code though its actor answered `Now` before it; a move put off to between steps,
    // with the batch's mail waiting a step for it; and mail dropped where the move happened.
    let Program { mut harness, guest: mover, library, second } =
        program::<echo::Mover>(&echo::MoverConfig { holds: true }, 1)?;
    let blob = second[0].clone();
    harness.tell(library, &Publish { kind: echo::Mover::NAME, blob })?;
    harness.settle();
    harness.tell(mover, &echo::LetGo)?;
    harness.settle();
    let _ = asks(&mut harness, mover);

    let asked = harness.ask(mover, &echo::Probe)?;
    let turned = harness.step_turns();
    let asked_in_step = asks(&mut harness, mover);
    let answered = handled(report(harness.answer(asked)?));

    assert_eq!(asked_in_step, [(Upgrade::Now, 1)], "asked before the probe's batch, in the step it ran in");
    assert_eq!(turns_of(&turned, mover), 1, "the probe ran in the step it was sent for");
    assert_eq!(answered, Ok((2, 1)), "on the new code");
    assert_eq!(lines(&mut harness, mover, Level::Info), ["moved to publish 1; absent: first_move"]);
    Ok(())
}

#[test]
fn a_failed_move_runs_the_batch_on_the_old_code_and_parks_the_guest() -> Result<(), Unheard> {
    // Catches a failed move that loses the batch's mail, runs it on no code, or loses the state; and a parked actor
    // asked again before each batch, failing the same way each time.
    let Program { mut harness, guest: stubborn, library, second } =
        program::<echo::Stubborn>(&echo::MoverConfig { holds: true }, 1)?;
    let blob = second[0].clone();
    harness.tell(library, &Publish { kind: echo::Stubborn::NAME, blob })?;
    harness.settle();
    harness.tell(stubborn, &echo::LetGo)?;
    harness.settle();
    let _ = asks(&mut harness, stubborn);

    let asked = harness.ask(stubborn, &echo::Probe)?;
    let turned = harness.step_turns();
    let failed = handled(report(harness.answer(asked)?));
    let asked_failing = asks(&mut harness, stubborn);
    let parked = handled(probe(&mut harness, stubborn)?);
    let asked_parked = asks(&mut harness, stubborn);

    assert_eq!(asked_failing, [(Upgrade::Now, 1)]);
    assert_eq!(turns_of(&turned, stubborn), 1, "the probe ran in the step its move failed in");
    assert_eq!(failed, Ok((1, 1)), "on its old code, with its state");
    assert_eq!(parked, Ok((1, 2)));
    assert_eq!(asked_parked, [], "parked: not asked before its next batch");
    let errors = lines(&mut harness, stubborn, Level::Error);
    assert_eq!(errors, ["did not move to publish 1: the old state has no `answered`; parked"]);
    Ok(())
}

#[test]
fn a_parked_guest_is_not_asked_again_until_its_kinds_head_moves_on() -> Result<(), Unheard> {
    // Catches a parked actor asked again while the head it failed on stands, and one never asked again once its
    // kind's head moves on.
    let Program { mut harness, guest: stubborn, library, second } =
        program::<echo::Stubborn>(&echo::MoverConfig { holds: false }, 2)?;
    let first = second[0].clone();
    harness.tell(library, &Publish { kind: echo::Stubborn::NAME, blob: first })?;
    harness.settle();
    let failed = asks(&mut harness, stubborn);

    let parked = handled(probe(&mut harness, stubborn)?);
    let again = handled(probe(&mut harness, stubborn)?);
    let while_parked = asks(&mut harness, stubborn);
    let next = second[1].clone();
    harness.tell(library, &Publish { kind: echo::Stubborn::NAME, blob: next })?;
    harness.settle();
    let after_publish = asks(&mut harness, stubborn);
    let unparked = handled(probe(&mut harness, stubborn)?);

    assert_eq!(failed, [(Upgrade::Now, 1)], "asked once, with no mail, as the head moved");
    assert_eq!(parked, Ok((1, 1)), "on its old code with its state");
    assert_eq!(again, Ok((1, 2)));
    assert_eq!(while_parked, [], "parked: not asked before its batches");
    assert_eq!(after_publish, [(Upgrade::Now, 2)], "asked again once its kind's head moved on");
    assert_eq!(unparked, Ok((1, 3)), "parked again, on its old code with its state");
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
    let idle = asks(&mut harness, mover);

    let version = probe(&mut harness, mover)?.map(|report| report.version);
    let batch = asks(&mut harness, mover);

    assert_eq!(idle, [(Upgrade::Later, 1)], "asked once while it had no mail, however many steps passed");
    assert_eq!(version, Ok(1));
    assert_eq!(batch, [(Upgrade::Later, 1)], "and once before its next batch");
    Ok(())
}

#[test]
fn a_guest_whose_state_keeps_its_layout_is_taken_in_as_it_is() -> Result<(), Unheard> {
    // Catches a state of the same kind handed to `take_over`, which the keeper does not write, so its move would fail,
    // and a same-layout move that loses its state.
    let Program { mut harness, guest: keeper, library, second } =
        program::<echo::Keeper>(&echo::MoverConfig { holds: false }, 1)?;
    let before = handled(probe(&mut harness, keeper)?);
    let blob = second[0].clone();
    harness.tell(library, &Publish { kind: echo::Keeper::NAME, blob })?;
    harness.settle();

    let after = handled(probe(&mut harness, keeper)?);

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
fn a_moved_guest_keeps_its_path_its_watchers_and_the_references_it_holds() -> Result<(), Unheard> {
    // Catches a move that ends the actor and begins another, which its watchers would be shown; one that gives it
    // another path; and one that drops the references in its state, whose blobs it could then not read.
    let Program { mut harness, guest: mover, library, second } =
        program::<echo::Mover>(&echo::MoverConfig { holds: false }, 1)?;
    let path = ActorPath::from_root(&PathSegment::new(echo::Mover::NAME, &()));
    harness.add_root::<Lookout>(&(), Looks { at: path.clone() })?;
    let lookout = harness.root::<Lookout>();
    let kept = harness.put(vec![7; 300])?;
    harness.tell(mover, &echo::Keep { blob: kept })?;
    let blob = second[0].clone();
    harness.tell(library, &Publish { kind: echo::Mover::NAME, blob })?;
    harness.settle();

    let report = probe(&mut harness, mover)?.map(|report| (report.version, report.kept));
    let resolved = harness.host().engine().table().resolve(&path);
    let seen_moving = recall(&mut harness, lookout)?;
    harness.tell(mover, &echo::Shut)?;
    harness.settle();
    let seen_closing = recall(&mut harness, lookout)?;

    assert_eq!(report, Ok((2, 300)), "the new code reads the blob its state refers to");
    assert_eq!(resolved, Some(mover.seated().id), "its path still names it");
    assert_eq!(seen_moving, Ok(Vec::new()), "moving is not ending");
    assert_eq!(seen_closing, Ok(vec![Sighting::Closed]), "the watch made on its old code sees it close on its new");
    Ok(())
}

/// A program of one part that runs guests with wasmtime, and in it, from the echo, a breaker, a mover and a keeper,
/// seated in that order, each holding until it is told to let go, and the library: its harness, those, and a blob
/// of the echo's second build put as code.
struct Crowd {
    harness: Harness<Bench>,
    breaker: ActorRef<echo::Breaker>,
    mover: ActorRef<echo::Mover>,
    keeper: ActorRef<echo::Keeper>,
    library: ActorRef<Library>,
    second: Bytes,
}

/// The [`Crowd`].
fn crowd() -> Result<Crowd, Unheard> {
    let mut engine =
        Engine::new(NonZeroUsize::MIN, 1024, 1 << 20, 1 << 23, 64).with_guests(Arc::new(Runtime::default())).ready();
    let (Ok(first), Ok(second)) = (engine.put_code(ECHO.to_vec()), engine.put_code(ECHO_V2.to_vec())) else {
        panic!("the echo guest's builds are fit to run");
    };
    let holds = echo::MoverConfig { holds: true };
    engine.add_root_from::<echo::Breaker>(&first, &(), &holds)?;
    engine.add_root_from::<echo::Mover>(&first, &(), &holds)?;
    engine.add_root_from::<echo::Keeper>(&first, &(), &holds)?;
    let Ok(publisher) = engine.publisher() else {
        panic!("the engine hands out its publisher once");
    };
    engine.add_root::<Library>(&(), Shelves { publisher })?;
    let mut harness = Harness::bare(engine)?;
    let (breaker, mover, keeper) = (harness.root(), harness.root(), harness.root());
    let library = harness.root::<Library>();

    Ok(Crowd { harness, breaker, mover, keeper, library, second })
}

#[test]
fn a_guest_whose_instance_broke_as_another_moved_runs_nothing_more_in_that_instances_place() -> Result<(), Unheard> {
    // Catches an actor of an instance a trap in another's `take_over` dropped, mid-step, running later in the step on
    // the instance of the same code the next move made: on whichever actor that instance keeps under its handle,
    // answering for it and changing its state. The keeper moves first, into the second build's instance; the
    // breaker's move traps there, ending the keeper; the mover's then makes the instance again; the keeper's probe
    // runs after both.
    let Crowd { mut harness, breaker, mover, keeper, library, second } = crowd()?;
    for kind in [echo::Breaker::NAME, echo::Mover::NAME, echo::Keeper::NAME] {
        harness.tell(library, &Publish { kind, blob: second.clone() })?;
    }
    harness.settle();
    harness.tell(breaker, &echo::LetGo)?;
    harness.tell(mover, &echo::LetGo)?;
    harness.settle();

    let probes =
        (harness.ask(breaker, &echo::Probe)?, harness.ask(mover, &echo::Probe)?, harness.ask(keeper, &echo::Probe)?);
    let (broke, moved, lost) = harness.answers(probes)?;
    let again = handled(probe(&mut harness, mover)?);

    assert_eq!(handled(report(broke)), Ok((1, 1)), "the breaker's batch ran on its old code");
    assert_eq!(handled(report(moved)), Ok((2, 1)), "the mover's on its new code");
    assert_eq!(handled(report(lost)), Err(Unanswered::Failed), "the keeper ended with its instance");
    assert_eq!(again, Ok((2, 2)), "the mover's state is its own");
    let errors = lines(&mut harness, breaker, Level::Error);
    let broke_taking_over = errors.iter().any(|line| line.contains("broke taking it over"));
    assert!(broke_taking_over, "the breaker's move failed as its new code broke: {errors:?}");
    Ok(())
}

/// What the engine records of `actor`, behind its kind's head: the publish it was found behind, and where it stands.
fn standing<A>(harness: &mut Harness<Bench>, actor: ActorRef<A>) -> Option<(u64, Standing)> {
    let found = harness.host().engine().moves();
    let name = harness.host().engine().table().name(actor.seated().id)?;
    let behind = found.behind.into_iter().find(|behind| behind.actor == ActorName::Path(name.clone()))?;

    Some((behind.publish, behind.standing))
}

#[test]
fn a_guest_with_mail_as_its_head_moves_is_recorded_still_to_be_asked_until_its_batch_asks_it() -> Result<(), Unheard> {
    // Catches a guest that has mail as its kind's head moves left out of what the engine records, so `upgrades` would
    // not list it among the guests still to be asked; and one still recorded as unasked once its batch has asked it. With no hops, the library's word to the mover waits for the next step, so it is
    // waiting as the head moves.
    let mut engine =
        Engine::new(NonZeroUsize::MIN, 1024, 1 << 20, 1 << 23, 0).with_guests(Arc::new(Runtime::default())).ready();
    let (Ok(first), Ok(second)) = (engine.put_code(ECHO.to_vec()), engine.put_code(ECHO_V2.to_vec())) else {
        panic!("the echo guest's builds are fit to run");
    };
    engine.add_root_from::<echo::Mover>(&first, &(), &echo::MoverConfig { holds: true })?;
    let Ok(publisher) = engine.publisher() else {
        panic!("the engine hands out its publisher once");
    };
    engine.add_root::<LettingGo>(&(), Shelves { publisher })?;
    let mut harness = Harness::bare(engine)?;
    let (mover, library) = (harness.root::<echo::Mover>(), harness.root::<LettingGo>());

    harness.tell(library, &PublishLettingGo { blob: second })?;
    harness.step();
    let as_the_head_moved = standing(&mut harness, mover);
    harness.step();
    let once_asked = standing(&mut harness, mover);

    assert_eq!(as_the_head_moved, Some((1, Standing::Unasked)), "its word to let go waits as the head moves");
    assert_eq!(once_asked, Some((1, Standing::Later { batches: 1 })), "asked before the batch that let it go");
    Ok(())
}
