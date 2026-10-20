//! The lifecycle the engine runs: each stage sent to its subscribers, and the next only once it has finished
//! (ADR-0020).

use super::super::*;
use crate::stages::key;
use aethereal_actor::{
    Actor, Anywhere, ChildOf, Cycle, Drained, Ending, Fault, Handles, Held, Path, Present, Render, Spawns, Stage,
    Start, Stop, Subscribes, Tells, Tick, Time, To, ToSender, ToType, ToWithoutMail, Top, Turn, Unordered,
};
use aethereal_data::{ActorPath, Bound, Kind, KindId, KindName, PathSegment, Plain};
use core::time::Duration;
use std::cell::RefCell;
use std::mem;

thread_local! {
    /// What this thread's actors have said they did, in order.
    static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Notes what an actor did.
fn say(line: String) {
    SAID.with_borrow_mut(|said| said.push(line));
}

/// What has been said since this was last asked.
fn said() -> Vec<String> {
    SAID.with_borrow_mut(mem::take)
}

/// A note an actor sends itself.
#[derive(Plain)]
struct Note {
    text: String,
}

impl Kind for Note {
    const NAME: KindName = KindName::new("aether.test.stage_note");
}

/// What a staged actor is told as it begins: its number, and the notes it sends itself.
#[derive(Plain)]
struct StagedConfig {
    number: u64,
    to_self: Vec<String>,
}

impl Kind for StagedConfig {
    const NAME: KindName = KindName::new("aether.test.staged_config");
}

/// An actor that subscribes to `Start` and `Stop` as it begins, and says each it is sent. On `Start` it sends itself
/// `pass 2`, and the actor `2` sends itself `quit` after it. It acts on the notes it is sent:
///
/// - `pass N` sends itself `pass N-1` until `N` is zero, so a stage's chain runs on for a few steps;
/// - `quit` asks the program to stop, `close` closes it, and `leave` unsubscribes it from `Start`.
#[derive(Bound)]
struct Staged {
    number: u64,
}

impl Kind for Staged {
    const NAME: KindName = KindName::new("aether.test.staged");
}

impl ChildOf for Staged {
    type Parent = Top;
}

impl Actor for Staged {
    type Config = StagedConfig;
    type Key = u64;
    type Handles = (Start, Stop, Note);
    type BeginSends = (Subscribes<Start>, Subscribes<Stop>, ToType<Self, Note>);
    type EndSends = ();

    fn begin(config: StagedConfig, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Start, _>();
        turn.subscribe::<Stop, _>();
        for text in config.to_self {
            let me = turn.me();
            turn.send(ToWithoutMail::Held(me), &Note { text })?;
        }

        Ok(Self { number: config.number })
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Start> for Staged {
    type Sends = (ToType<Self, Note>,);

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        say(format!("{} started", self.number));
        let me = turn.me();
        turn.send(To::Held(me), &Note { text: String::from("pass 2") })?;
        // The second asks to stop in a turn of `Start`'s chain, before the chain has finished.
        let quits = self.number == 2;
        if quits {
            turn.send(To::Held(me), &Note { text: String::from("quit") })?;
        }

        Ok(())
    }
}

impl Handles<Stop> for Staged {
    type Sends = ();

    fn handle(&mut self, _: Stop, _: &mut Turn<'_, ()>) -> Result<(), Fault> {
        say(format!("{} stopping", self.number));

        Ok(())
    }
}

impl Handles<Note> for Staged {
    type Sends = (ToType<Self, Note>, Subscribes<Start>);

    fn handle(&mut self, note: Note, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        let Note { text } = note;
        say(format!("{} got {text}", self.number));
        let words: Vec<&str> = text.split(' ').collect();
        match words[..] {
            ["pass", left] => {
                let left: u64 = left.parse().unwrap_or_default();
                if left > 0 {
                    let me = turn.me();
                    turn.send(To::Held(me), &Note { text: format!("pass {}", left - 1) })?;
                }
            }
            ["quit"] => turn.quit(),
            ["close"] => turn.close(),
            ["leave"] => turn.unsubscribe::<Start, _>(),
            _ => {}
        }

        Ok(())
    }
}

/// A clock that never moves: these tests are about what turns leave, not how long they take.
struct Stopped;

impl Clock for Stopped {
    fn nanos(&self) -> u64 {
        0
    }
}

/// An engine with a staged root for each of `roots`, its number and the notes it sends itself as it begins.
fn engine(roots: &[(u64, &[&str])]) -> Engine {
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0);
    for (number, to_self) in roots {
        let to_self = to_self.iter().map(|text| String::from(*text)).collect();
        let Ok(_) = engine.add_root::<Staged>(number, StagedConfig { number: *number, to_self }) else {
            panic!("the root begins");
        };
    }

    engine
}

/// Runs one step and answers with what its turns said, sorted: turns of one step run in no order these tests read.
fn step(engine: &mut Engine) -> Vec<String> {
    engine.step(Time::ZERO, &Stopped);
    let mut heard = said();
    heard.sort();

    heard
}

#[test]
fn stop_is_sent_only_once_every_turn_start_set_off_has_run() {
    // Catches a stage sent while the chain of the one before it is still running, `Stop` never sent once asked
    // for, and a stopped lifecycle that never says so.
    let mut engine = engine(&[(1, &[])]);
    assert_eq!(engine.lifecycle(), Lifecycle::Waiting);
    engine.start();
    engine.quit();
    assert_eq!(engine.lifecycle(), Lifecycle::Stopping);

    assert_eq!(step(&mut engine), ["1 started"]);
    assert_eq!(step(&mut engine), ["1 got pass 2"]);
    assert_eq!(step(&mut engine), ["1 got pass 1"]);
    assert_eq!(step(&mut engine), ["1 got pass 0"]);
    assert_eq!(engine.lifecycle(), Lifecycle::Stopping);
    assert_eq!(step(&mut engine), ["1 stopping"]);
    assert_eq!(engine.lifecycle(), Lifecycle::Stopping, "what the engine says of `Stop` left unanswered is to come");
    assert!(step(&mut engine).is_empty());
    assert_eq!(engine.lifecycle(), Lifecycle::Stopped);
    assert!(engine.is_idle());
}

#[test]
fn an_actor_that_asks_to_stop_stops_the_program_once_its_stage_has_finished() {
    // Catches an actor's request to stop that the engine never reads, and one that cuts the stage under way short:
    // the second root asks in a turn of `Start`'s chain, which runs on two steps more. One asked before the host
    // started the lifecycle stops it as well.
    let mut early = engine(&[(1, &["quit"])]);
    assert_eq!(step(&mut early), ["1 got quit"]);
    assert_eq!(early.lifecycle(), Lifecycle::Stopping, "asked before the host started it");

    let mut engine = engine(&[(1, &[]), (2, &[])]);
    engine.start();
    assert_eq!(step(&mut engine), ["1 started", "2 started"]);
    assert_eq!(step(&mut engine), ["1 got pass 2", "2 got pass 2", "2 got quit"]);
    assert_eq!(engine.lifecycle(), Lifecycle::Stopping);
    assert_eq!(step(&mut engine), ["1 got pass 1", "2 got pass 1"]);
    assert_eq!(step(&mut engine), ["1 got pass 0", "2 got pass 0"]);
    assert_eq!(step(&mut engine), ["1 stopping", "2 stopping"]);
    assert!(step(&mut engine).is_empty());
    assert_eq!(engine.lifecycle(), Lifecycle::Stopped);
}

#[test]
fn a_stage_goes_only_to_the_live_actors_subscribed_to_it() {
    // Catches a stage sent to an actor that unsubscribed from it, or to one that has gone, a subscription asked for
    // in a `begin` that never takes, `Stop` sent once a stage has finished when nothing asked for it, and one that has
    // gone kept among the subscribers for good.
    let mut engine = engine(&[(1, &[]), (2, &["close"]), (3, &["leave"])]);
    assert_eq!(step(&mut engine), ["2 got close", "3 got leave"]);
    step(&mut engine);
    engine.start();

    assert_eq!(step(&mut engine), ["1 started"]);
    let start = KindId::of::<Start>();
    let subscribed = engine.stages.shard(key(start)).subscribers(start).count();
    assert_eq!(subscribed, 1, "the actor that had gone is taken out as the stage passes it over");
    let mut heard = Vec::new();
    while !engine.is_idle() {
        heard.extend(step(&mut engine));
    }
    assert_eq!(heard, ["1 got pass 2", "1 got pass 1", "1 got pass 0"]);
    assert_eq!(engine.lifecycle(), Lifecycle::Running, "nothing asked to stop");
}

/// An actor at the name the engine keeps for its lifecycle.
#[derive(Bound)]
struct Impostor;

impl Kind for Impostor {
    const NAME: KindName = LIFECYCLE;
}

impl ChildOf for Impostor {
    type Parent = Top;
}

impl Actor for Impostor {
    type Config = Note;
    type Key = ();
    type Handles = ();
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Note, _: &mut Beginning<'_, ()>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

#[test]
fn no_root_is_made_at_the_lifecycle_s_name() {
    // Catches a root made at the id every stage's chain begins at, whose own mail would then be taken for a stage's.
    let mut engine = engine(&[]);

    let made = engine.add_root::<Impostor>(&(), Note { text: String::new() });
    assert!(matches!(made, Err(Refused::KeyTaken)));
}

/// A stage of a chassis's own, which a cycle may hold anywhere.
#[derive(Plain, Default)]
struct Receive;

impl Kind for Receive {
    const NAME: KindName = KindName::new("aether.test.receive");
}

impl Stage for Receive {}

/// An actor that subscribes to every stage of a cycle and to `Stop` as it begins, and says each it is sent, `Tick`
/// with its delta and the time elapsed. On `Tick` it sends itself `pass 1`, so the stage's chain runs a step longer.
#[derive(Bound)]
struct Cycler;

impl Kind for Cycler {
    const NAME: KindName = KindName::new("aether.test.cycler");
}

impl ChildOf for Cycler {
    type Parent = Anywhere;
}

impl Actor for Cycler {
    type Config = Note;
    type Key = ();
    type Handles = (Receive, Tick, Render, Present, Stop, Note);
    type BeginSends =
        (Subscribes<Receive>, Subscribes<Tick>, Subscribes<Render>, Subscribes<Present>, Subscribes<Stop>);
    type EndSends = ();

    fn begin(_: Note, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Receive, _>();
        turn.subscribe::<Tick, _>();
        turn.subscribe::<Render, _>();
        turn.subscribe::<Present, _>();
        turn.subscribe::<Stop, _>();

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Tick> for Cycler {
    type Sends = (ToType<Self, Note>,);

    fn handle(&mut self, tick: Tick, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        say(format!("tick {} {}", tick.delta_nanos, tick.elapsed_nanos));
        let me = turn.me();
        turn.send(To::Held(me), &Note { text: String::from("pass 1") })?;

        Ok(())
    }
}

impl Handles<Note> for Cycler {
    type Sends = ();

    fn handle(&mut self, note: Note, _: &mut Turn<'_, ()>) -> Result<(), Fault> {
        say(note.text);

        Ok(())
    }
}

/// Says the stage it is sent, by name.
macro_rules! says {
    ($($stage:ident => $said:literal),+) => {
        $(
            impl Handles<$stage> for Cycler {
                type Sends = ();

                fn handle(&mut self, _: $stage, _: &mut Turn<'_, ()>) -> Result<(), Fault> {
                    say(String::from($said));

                    Ok(())
                }
            }
        )+
    };
}

says!(Receive => "receive", Render => "render", Present => "present", Stop => "stopping");

/// The cycle a cycler is made on: every stage it subscribes to, a chassis's own first.
type Full = (Receive, Tick, Render, Present);

/// An engine running the cycle `C`, with a cycler, started at the host's first moment, its `Start` finished.
fn cycling<C: Cycle>(limit: Duration) -> Engine {
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let Ok(engine) = Engine::new(parts, 1024, 0, 1 << 20, 0).with_cycle::<C>() else {
        panic!("the cycle is in order");
    };
    let mut engine = engine.with_delta_limit(limit);
    let Ok(_) = engine.add_root::<Cycler>(&(), Note { text: String::new() }) else {
        panic!("the cycler begins");
    };
    engine.start();

    engine
}

/// Runs one step at `nanos` and answers with what its turns said.
fn step_at(engine: &mut Engine, nanos: u64) -> Vec<String> {
    engine.step(Time::from_nanos(nanos), &Stopped);

    said()
}

#[test]
fn a_cycle_runs_its_stages_in_order_each_once_the_last_has_finished() {
    // Catches a stage of a cycle sent before every turn of the one before it has run, a cycle that does not end
    // with its last stage, cycles said to be due while one runs queued up, and a `Tick` whose delta is not the time
    // since the cycle before.
    let mut engine = cycling::<Full>(Duration::from_secs(1));
    engine.cycle();
    assert_eq!(step_at(&mut engine, 10), ["receive"]);
    engine.cycle();
    engine.cycle();
    assert_eq!(step_at(&mut engine, 20), ["tick 0 0"]);
    assert_eq!(step_at(&mut engine, 30), ["pass 1"]);
    assert_eq!(step_at(&mut engine, 40), ["render"]);
    assert_eq!(step_at(&mut engine, 50), ["present"]);
    assert_eq!(step_at(&mut engine, 60), ["receive"], "the cycles due while one ran are one cycle");
    assert_eq!(step_at(&mut engine, 70), ["tick 50 50"]);
    assert_eq!(step_at(&mut engine, 80), ["pass 1"]);
    assert_eq!(step_at(&mut engine, 90), ["render"]);
    assert_eq!(step_at(&mut engine, 100), ["present"]);
    assert!(engine.is_idle(), "no cycle is due");
}

#[test]
fn a_tick_s_delta_is_limited_and_its_stage_of_a_chassis_s_own_comes_first() {
    // Catches a delta past the host's limit, a time elapsed that is not the sum of the deltas, and a `Tick` that is
    // not its cycle's first stage given no time at all.
    let mut engine = cycling::<Full>(Duration::from_nanos(25));
    engine.cycle();
    assert_eq!(step_at(&mut engine, 10), ["receive"]);
    assert_eq!(step_at(&mut engine, 20), ["tick 0 0"]);
    assert_eq!(step_at(&mut engine, 30), ["pass 1"]);
    assert_eq!(step_at(&mut engine, 40), ["render"]);
    assert_eq!(step_at(&mut engine, 50), ["present"]);

    // 50 since the last cycle began, limited to 25.
    engine.cycle();
    assert_eq!(step_at(&mut engine, 60), ["receive"]);
    assert_eq!(step_at(&mut engine, 70), ["tick 25 25"]);
    assert_eq!(step_at(&mut engine, 80), ["pass 1"]);
    assert_eq!(step_at(&mut engine, 90), ["render"]);
    assert_eq!(step_at(&mut engine, 100), ["present"]);
    engine.cycle();
    assert_eq!(step_at(&mut engine, 110), ["receive"]);
    assert_eq!(step_at(&mut engine, 120), ["tick 25 50"]);
}

#[test]
fn a_program_asked_to_stop_mid_cycle_finishes_the_cycle_first() {
    // Catches `Stop` sent in the middle of a cycle, so an actor stops between drawing and showing.
    let mut engine = cycling::<Full>(Duration::from_secs(1));
    engine.cycle();
    assert_eq!(step_at(&mut engine, 10), ["receive"]);
    engine.quit();
    engine.cycle();
    assert_eq!(step_at(&mut engine, 20), ["tick 0 0"]);
    assert_eq!(step_at(&mut engine, 30), ["pass 1"]);
    assert_eq!(step_at(&mut engine, 40), ["render"]);
    assert_eq!(step_at(&mut engine, 50), ["present"]);
    assert_eq!(step_at(&mut engine, 60), ["stopping"], "no cycle begins once asked to stop");
    // The cycler neither answered `Stop` nor held it, and what the engine says of that comes back to it, and is
    // dropped, in the next step.
    assert!(step_at(&mut engine, 70).is_empty());
    assert_eq!(engine.lifecycle(), Lifecycle::Stopped);
}

#[test]
fn a_cycle_out_of_order_or_holding_an_end_or_a_stage_twice_is_refused() {
    // Catches a cycle that turns the shared stages round, holds `Start`, or holds a stage twice made into an engine,
    // and a chassis's own stage refused anywhere.
    let Some(parts) = NonZeroUsize::new(1) else {
        panic!("one is not zero");
    };
    let made = |cycle: fn(Engine) -> Result<Engine, Unordered>| cycle(Engine::new(parts, 1024, 0, 1 << 20, 0)).err();

    assert_eq!(made(Engine::with_cycle::<(Render, Tick)>), Some(Unordered::OutOfOrder));
    assert_eq!(made(Engine::with_cycle::<(Tick, Present, Render)>), Some(Unordered::OutOfOrder));
    assert_eq!(made(Engine::with_cycle::<(Start, Tick)>), Some(Unordered::HoldsAnEnd));
    assert_eq!(made(Engine::with_cycle::<(Tick, Tick)>), Some(Unordered::Twice));
    assert_eq!(made(Engine::with_cycle::<(Receive, Tick, Receive)>), Some(Unordered::Twice));
    assert_eq!(made(Engine::with_cycle::<(Tick, Receive, Present)>), None);
    assert_eq!(made(Engine::with_cycle::<(Receive, Render)>), None);
}

/// An actor that spawns a cycler beneath it as it begins, and a latecomer in a step, on the note it sends itself, and
/// says why each was refused, if it was.
#[derive(Bound)]
struct Nest;

impl Kind for Nest {
    const NAME: KindName = KindName::new("aether.test.nest");
}

impl ChildOf for Nest {
    type Parent = Top;
}

impl Actor for Nest {
    type Config = Note;
    type Key = ();
    type Handles = (Note,);
    type BeginSends = (ToType<Self, Note>, Spawns<Cycler>);
    type EndSends = ();

    fn begin(_: Note, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        let spawned = turn.spawn(&(), Note { text: String::new() });
        say(format!("{:?}", spawned.err()));
        let me = turn.me();
        turn.send(ToWithoutMail::Held(me), &Note { text: String::new() })?;

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Note> for Nest {
    type Sends = (Spawns<Latecomer>,);

    fn handle(&mut self, _: Note, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        let spawned = turn.spawn(&(), Note { text: String::new() });
        say(format!("{:?}", spawned.err()));

        Ok(())
    }
}

/// An engine of four parts running the cycle `C`.
fn running<C: Cycle>() -> Engine {
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let Ok(engine) = Engine::new(parts, 1024, 0, 1 << 20, 0).with_cycle::<C>() else {
        panic!("the cycle is in order");
    };

    engine
}

#[test]
fn an_actor_that_subscribes_to_a_stage_its_engine_does_not_run_is_refused_where_it_is_made() {
    // Catches a root or a child made though its engine never sends a stage it subscribes to, a stage of the cycle,
    // or one of the ends, taken for one the engine does not run, and a refusal that does not name the stage.
    let mut full = running::<Full>();
    let mut drawing = running::<(Tick, Render, Present)>();
    let unstaged = Refused::Unstaged(KindId::of::<Receive>());

    assert!(full.add_root::<Cycler>(&(), Note { text: String::new() }).is_ok());
    assert_eq!(drawing.add_root::<Cycler>(&(), Note { text: String::new() }), Err(unstaged.clone()));
    for engine in [&mut full, &mut drawing] {
        let Ok(_) = engine.add_root::<Nest>(&(), Note { text: String::new() }) else {
            panic!("the nest begins");
        };
        engine.step(Time::ZERO, &Stopped);
    }
    let refused = format!("{:?}", Some(unstaged.clone()));
    assert_eq!(said(), [String::from("None"), String::from("None"), refused.clone(), refused]);
    let late = drawing.add_root::<Latecomer>(&(), Note { text: String::new() });
    assert_eq!(late, Err(unstaged), "a subscription its handlers declare counts as its `begin`'s does");
}

/// An actor that declares its subscription to `Receive` in a handler, not as it begins.
#[derive(Bound)]
struct Latecomer;

impl Kind for Latecomer {
    const NAME: KindName = KindName::new("aether.test.latecomer");
}

impl ChildOf for Latecomer {
    type Parent = Anywhere;
}

impl Actor for Latecomer {
    type Config = Note;
    type Key = ();
    type Handles = (Receive, Note);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Note, _: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Receive> for Latecomer {
    type Sends = ();

    fn handle(&mut self, _: Receive, _: &mut Turn<'_, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Note> for Latecomer {
    type Sends = (Subscribes<Receive>,);

    fn handle(&mut self, _: Note, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        turn.subscribe::<Receive, _>();

        Ok(())
    }
}

/// An actor that subscribes to `Stop` as it begins, holds it when it is sent, and acts on the notes it is sent: `answer`
/// answers it, `let go` drops it unanswered, and `close` closes the actor holding it.
#[derive(Bound)]
struct Drainer {
    /// The `Stop` it holds: none until it is sent, and none once answered or let go.
    held: Vec<Held<Drained>>,
}

impl Kind for Drainer {
    const NAME: KindName = KindName::new("aether.test.drainer");
}

impl ChildOf for Drainer {
    type Parent = Top;
}

impl Actor for Drainer {
    type Config = Note;
    type Key = ();
    type Handles = (Stop, Note);
    type BeginSends = (Subscribes<Stop>,);
    type EndSends = ();

    fn begin(_: Note, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Stop, _>();

        Ok(Self { held: Vec::new() })
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Stop> for Drainer {
    type Sends = (ToSender<Drained>,);

    fn handle(&mut self, _: Stop, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        say(String::from("holding"));
        self.held.push(turn.hold()?);

        Ok(())
    }
}

impl Handles<Note> for Drainer {
    type Sends = ();

    fn handle(&mut self, note: Note, turn: &mut Turn<'_, ()>) -> Result<(), Fault> {
        say(note.text.clone());
        match note.text.as_str() {
            "answer" => {
                for held in self.held.drain(..) {
                    turn.answer(held, &Drained)?;
                }
            }
            "let go" => self.held.clear(),
            "close" => turn.close(),
            _ => {}
        }

        Ok(())
    }
}

/// An actor that, as it begins, sends the drainer the note it is made with: from a chain of its own, not `Stop`'s.
#[derive(Bound)]
struct Waker;

impl Kind for Waker {
    const NAME: KindName = KindName::new("aether.test.waker");
}

impl ChildOf for Waker {
    type Parent = Top;
}

impl Actor for Waker {
    type Config = Note;
    type Key = ();
    type Handles = ();
    type BeginSends = (ToType<Drainer, Note>,);
    type EndSends = ();

    fn begin(note: Note, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        let path = ActorPath::from_root(&PathSegment::new(Drainer::NAME, &()));
        let Some(drainer) = turn.resolve(&path) else {
            return Ok(Self);
        };
        turn.send(ToWithoutMail::Held(drainer), &note)?;

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// Stops an engine whose drainer holds `Stop`, then has a waker send it `told`, and answers with whether the program
/// had stopped after five steps of holding, and after five more once the drainer was told.
fn drained(told: &str) -> (Lifecycle, Lifecycle) {
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0);
    let Ok(_) = engine.add_root::<Drainer>(&(), Note { text: String::new() }) else {
        panic!("the drainer begins");
    };
    engine.start();
    engine.quit();
    assert_eq!(step(&mut engine), ["holding"]);
    for _ in 0..4 {
        step(&mut engine);
    }
    let held = engine.lifecycle();

    let Ok(_) = engine.add_root::<Waker>(&(), Note { text: String::from(told) }) else {
        panic!("the waker begins");
    };
    assert_eq!(step(&mut engine), [told]);
    for _ in 0..4 {
        step(&mut engine);
    }

    (held, engine.lifecycle())
}

#[test]
fn a_subscriber_that_holds_stop_keeps_the_program_from_stopping_until_it_answers_or_lets_it_go() {
    // Catches `Stop` sent as no request, so it cannot be held, `Stop` finished once its chain has though a subscriber
    // still holds it, and a held `Stop` that keeps the program from stopping once it was answered, let go, or its
    // holder closed.
    assert_eq!(drained("answer"), (Lifecycle::Stopping, Lifecycle::Stopped));
    assert_eq!(drained("let go"), (Lifecycle::Stopping, Lifecycle::Stopped));
    assert_eq!(drained("close"), (Lifecycle::Stopping, Lifecycle::Stopped));
    assert_eq!(drained("nothing"), (Lifecycle::Stopping, Lifecycle::Stopping));
}

#[test]
fn a_stage_nothing_subscribes_to_is_passed_at_once() {
    // Catches a stage sent to no subscriber left under way until a step's fold looks again: an engine with nothing to
    // run never stops, as no step will run to move it on.
    let Some(parts) = NonZeroUsize::new(1) else {
        panic!("one is not zero");
    };
    let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0);
    engine.start();
    engine.quit();

    assert_eq!(engine.lifecycle(), Lifecycle::Stopped);
    assert_eq!(engine.step_number(), 0);
}

/// A root, a singleton, that a castaway reaches at its typed path and is never made.
#[derive(Bound)]
struct Shore;

impl Kind for Shore {
    const NAME: KindName = KindName::new("aether.test.shore");
}

impl ChildOf for Shore {
    type Parent = Top;
}

impl Actor for Shore {
    type Config = Note;
    type Key = ();
    type Handles = (Note,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Note, _: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Note> for Shore {
    type Sends = ();

    fn handle(&mut self, _: Note, _: &mut Turn<'_, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// A root that subscribes to `Receive` as it begins and reaches the shore at its typed path: refused for both.
#[derive(Bound)]
struct Castaway;

impl Kind for Castaway {
    const NAME: KindName = KindName::new("aether.test.castaway");
}

impl ChildOf for Castaway {
    type Parent = Top;
}

impl Actor for Castaway {
    type Config = Note;
    type Key = ();
    type Handles = (Receive,);
    type BeginSends = (Subscribes<Receive>, Tells<Path<Shore>, Note>);
    type EndSends = ();

    fn begin(_: Note, _: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Receive> for Castaway {
    type Sends = ();

    fn handle(&mut self, _: Receive, _: &mut Turn<'_, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

#[test]
fn a_root_refused_both_for_a_stage_and_for_a_typed_path_is_refused_for_the_stage() {
    // Catches the check of typed paths run before the check of stages, where the stage is the refusal (ADR-0022).
    let mut drawing = running::<(Tick, Render, Present)>();
    let mut full = running::<Full>();

    let unstaged = Err(Refused::Unstaged(KindId::of::<Receive>()));
    assert_eq!(drawing.add_root::<Castaway>(&(), Note { text: String::new() }).map(|_| ()), unstaged);
    assert_eq!(
        full.add_root::<Castaway>(&(), Note { text: String::new() }).map(|_| ()),
        Err(Refused::Missing(Shore::NAME))
    );
}
