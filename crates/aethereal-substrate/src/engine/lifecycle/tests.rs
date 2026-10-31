//! The lifecycle the engine runs: each stage sent to its subscribers, and the next only once it has finished
//! (ADR-0020).

use super::super::outside::{Handed, Turned};
use super::super::*;
use crate::stages::key;
use aethereal_actor::{
    Actor, Anywhere, ChildOf, Cycle, Drained, Ending, Fault, Handles, Held, On, Own, Path, Present, Render, Stage,
    Start, Stop, Tells, Tick, Time, ToSender, ToType, Top, Turn, path,
};
use aethereal_data::{Bound, Kind, KindId, KindName, Plain};
use core::time::Duration;

/// What a test reads of one turn from the engine's record (ADR-0036). A number is the key of the actor, and an actor
/// with no key reads as nought. In order by actor first, so a step's turns sort as these tests read them.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Seen {
    /// The actor was handed this note.
    Got(u64, String),
    /// The actor was sent `Start`.
    Started(u64),
    /// The actor was sent `Stop`.
    Stopping(u64),
    /// A cycler was sent its chassis's own stage.
    Received,
    /// A cycler was sent `Tick`, with its delta and the time elapsed.
    Ticked(u64, u64),
    /// A cycler was sent `Render`.
    Rendered,
    /// A cycler was sent `Present`.
    Presented,
}

impl Seen {
    /// What `turned`, which did not fail, shows: nothing for a turn over no stage and no note.
    fn of(turned: &Turned) -> Option<Self> {
        assert!(!turned.failed(), "no turn of these tests fails");
        let Handed::Mail(_) = turned.handed else {
            return None;
        };
        let last = turned.path.last();
        let number = last.key().and_then(|key| key.to_string().parse().ok()).unwrap_or_default();

        if let Ok(Note { text }) = turned.read() {
            return Some(Self::Got(number, text));
        }
        if turned.read::<Start>().is_ok() {
            return Some(Self::Started(number));
        }
        if turned.read::<Stop>().is_ok() {
            return Some(Self::Stopping(number));
        }
        if turned.read::<Receive>().is_ok() {
            return Some(Self::Received);
        }
        if let Ok(tick) = turned.read::<Tick>() {
            return Some(Self::Ticked(tick.delta_nanos, tick.elapsed_nanos));
        }
        if turned.read::<Render>().is_ok() {
            return Some(Self::Rendered);
        }
        if turned.read::<Present>().is_ok() {
            return Some(Self::Presented);
        }

        None
    }
}

/// The actor `number` was handed the note `text`.
fn got(number: u64, text: &str) -> Seen {
    Seen::Got(number, String::from(text))
}

/// What the turns recorded since this was last asked show.
fn seen(engine: &mut Engine) -> Vec<Seen> {
    engine.take_turned().iter().filter_map(Seen::of).collect()
}

/// A note an actor sends itself.
#[derive(Plain)]
#[kind(name = "aether.test.stage_note")]
struct Note {
    text: String,
}

/// What a staged actor is told as it begins: its number, and the notes it sends itself.
#[derive(Plain)]
#[kind(name = "aether.test.staged_config")]
struct StagedConfig {
    number: u64,
    to_self: Vec<String>,
}

/// An actor that subscribes to `Start` and `Stop` as it begins. On `Start` it sends itself
/// `pass 2`, and the actor `2` sends itself `quit` after it. It acts on the notes it is sent:
///
/// - `pass N` sends itself `pass N-1` until `N` is zero, so a stage's chain runs on for a few steps;
/// - `quit` asks the program to stop, `close` closes it, and `leave` unsubscribes it from `Start`.
#[derive(Bound)]
#[kind(name = "aether.test.staged")]
struct Staged {
    number: u64,
}

impl ChildOf for Staged {
    type Parent = Top;
}

impl Actor for Staged {
    type Config = StagedConfig;
    type Key = u64;
    type Handles = (On<Start>, On<Stop>, Note);
    type BeginSends = (ToType<Self, Note>,);
    type EndSends = ();

    fn begin(config: StagedConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Start>();
        turn.subscribe::<Stop>();
        for text in config.to_self {
            turn.to(turn.me()).send(&Note { text })?;
        }

        Ok(Self { number: config.number })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<On<Start>> for Staged {
    type Sends = (ToType<Self, Note>,);

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.to(turn.me()).send(&Note { text: String::from("pass 2") })?;
        // The second asks to stop in a turn of `Start`'s chain, before the chain has finished.
        let quits = self.number == 2;
        if quits {
            turn.to(turn.me()).send(&Note { text: String::from("quit") })?;
        }

        Ok(())
    }
}

impl Handles<On<Stop>> for Staged {
    type Sends = ();

    fn handle(&mut self, _: Stop, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Note> for Staged {
    type Sends = (ToType<Self, Note>,);

    fn handle(&mut self, note: Note, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Note { text } = note;
        let words: Vec<&str> = text.split(' ').collect();
        match words[..] {
            ["pass", left] => {
                let left: u64 = left.parse().unwrap_or_default();
                if left > 0 {
                    turn.to(turn.me()).send(&Note { text: format!("pass {}", left - 1) })?;
                }
            }
            ["quit"] => turn.quit(),
            ["close"] => turn.close(),
            ["leave"] => turn.unsubscribe::<Start>(),
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
    let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0).ready();
    engine.record_turns();
    for (number, to_self) in roots {
        let to_self = to_self.iter().map(|text| String::from(*text)).collect();
        let Ok(_) = engine.add_root::<Staged>(number, StagedConfig { number: *number, to_self }) else {
            panic!("the root begins");
        };
    }

    engine
}

/// Runs one step and answers with what its turns show, sorted: turns of one step run in no order these tests read.
fn step(engine: &mut Engine) -> Vec<Seen> {
    engine.step(Time::ZERO, &Stopped);
    let mut shown = seen(engine);
    shown.sort();

    shown
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

    assert_eq!(step(&mut engine), [Seen::Started(1)]);
    assert_eq!(step(&mut engine), [got(1, "pass 2")]);
    assert_eq!(step(&mut engine), [got(1, "pass 1")]);
    assert_eq!(step(&mut engine), [got(1, "pass 0")]);
    assert_eq!(engine.lifecycle(), Lifecycle::Stopping);
    assert_eq!(step(&mut engine), [Seen::Stopping(1)]);
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
    assert_eq!(step(&mut early), [got(1, "quit")]);
    assert_eq!(early.lifecycle(), Lifecycle::Stopping, "asked before the host started it");

    let mut engine = engine(&[(1, &[]), (2, &[])]);
    engine.start();
    assert_eq!(step(&mut engine), [Seen::Started(1), Seen::Started(2)]);
    assert_eq!(step(&mut engine), [got(1, "pass 2"), got(2, "pass 2"), got(2, "quit")]);
    assert_eq!(engine.lifecycle(), Lifecycle::Stopping);
    assert_eq!(step(&mut engine), [got(1, "pass 1"), got(2, "pass 1")]);
    assert_eq!(step(&mut engine), [got(1, "pass 0"), got(2, "pass 0")]);
    assert_eq!(step(&mut engine), [Seen::Stopping(1), Seen::Stopping(2)]);
    assert!(step(&mut engine).is_empty());
    assert_eq!(engine.lifecycle(), Lifecycle::Stopped);
}

#[test]
fn a_stage_goes_only_to_the_live_actors_subscribed_to_it() {
    // Catches a stage sent to an actor that unsubscribed from it, or to one that has gone, a subscription asked for
    // in a `begin` that never takes, `Stop` sent once a stage has finished when nothing asked for it, and one that has
    // gone kept among the subscribers for good.
    let mut engine = engine(&[(1, &[]), (2, &["close"]), (3, &["leave"])]);
    assert_eq!(step(&mut engine), [got(2, "close"), got(3, "leave")]);
    step(&mut engine);
    engine.start();

    assert_eq!(step(&mut engine), [Seen::Started(1)]);
    let start = KindId::of::<Start>();
    let subscribed = engine.stages.shard(key(start)).subscribers(start).count();
    assert_eq!(subscribed, 1, "the actor that had gone is taken out as the stage passes it over");
    let mut heard = Vec::new();
    while !engine.is_idle() {
        heard.extend(step(&mut engine));
    }
    assert_eq!(heard, [got(1, "pass 2"), got(1, "pass 1"), got(1, "pass 0")]);
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

    fn begin(_: Note, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
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
#[kind(name = "aether.test.receive")]
struct Receive;

impl Stage for Receive {
    type Place = Own;
}

/// An actor that subscribes to every stage of a cycle and to `Stop` as it begins. On `Tick` it sends itself `pass 1`,
/// so the stage's chain runs a step longer.
#[derive(Bound)]
#[kind(name = "aether.test.cycler")]
struct Cycler;

impl ChildOf for Cycler {
    type Parent = Anywhere;
}

impl Actor for Cycler {
    type Config = Note;
    type Key = ();
    type Handles = (On<Receive>, On<Tick>, On<Render>, On<Present>, On<Stop>, Note);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Note, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Receive>();
        turn.subscribe::<Tick>();
        turn.subscribe::<Render>();
        turn.subscribe::<Present>();
        turn.subscribe::<Stop>();

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<On<Tick>> for Cycler {
    type Sends = (ToType<Self, Note>,);

    fn handle(&mut self, _: Tick, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.to(turn.me()).send(&Note { text: String::from("pass 1") })?;

        Ok(())
    }
}

impl Handles<Note> for Cycler {
    type Sends = ();

    fn handle(&mut self, _: Note, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// Takes the stage it is sent, which the record shows.
macro_rules! takes {
    ($($stage:ident),+) => {
        $(
            impl Handles<On<$stage>> for Cycler {
                type Sends = ();

                fn handle(&mut self, _: $stage, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
                    Ok(())
                }
            }
        )+
    };
}

takes!(Receive, Render, Present, Stop);

/// The cycle a cycler is made on: every stage it subscribes to, a chassis's own first.
type Full = (Receive, Tick, Render, Present);

/// An engine running the cycle `C`, with a cycler, started at the host's first moment, its `Start` finished.
fn cycling<C: Cycle>(limit: Duration) -> Engine {
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0).with_delta_limit(limit).with_cycle::<C>().ready();
    engine.record_turns();
    let Ok(_) = engine.add_root::<Cycler>(&(), Note { text: String::new() }) else {
        panic!("the cycler begins");
    };
    engine.start();

    engine
}

/// Runs one step at `nanos` and answers with what its turns show.
fn step_at(engine: &mut Engine, nanos: u64) -> Vec<Seen> {
    engine.step(Time::from_nanos(nanos), &Stopped);

    seen(engine)
}

#[test]
fn a_cycle_runs_its_stages_in_order_each_once_the_last_has_finished() {
    // Catches a stage of a cycle sent before every turn of the one before it has run, a cycle that does not end
    // with its last stage, cycles said to be due while one runs queued up, and a `Tick` whose delta is not the host's
    // time since the cycle before: the steps' time, which drifts with how long each cycle's work took, or the time
    // of the first cycle said due while one ran rather than the last.
    let mut engine = cycling::<Full>(Duration::from_secs(1));
    engine.cycle(Time::ZERO);
    assert_eq!(step_at(&mut engine, 10), [Seen::Received]);
    engine.cycle(Time::from_nanos(15));
    engine.cycle(Time::from_nanos(25));
    assert_eq!(step_at(&mut engine, 20), [Seen::Ticked(0, 0)]);
    assert_eq!(step_at(&mut engine, 30), [got(0, "pass 1")]);
    assert_eq!(step_at(&mut engine, 40), [Seen::Rendered]);
    assert_eq!(step_at(&mut engine, 50), [Seen::Presented]);
    assert_eq!(step_at(&mut engine, 60), [Seen::Received], "the cycles due while one ran are one cycle");
    assert_eq!(step_at(&mut engine, 70), [Seen::Ticked(25, 25)]);
    assert_eq!(step_at(&mut engine, 80), [got(0, "pass 1")]);
    assert_eq!(step_at(&mut engine, 90), [Seen::Rendered]);
    assert_eq!(step_at(&mut engine, 100), [Seen::Presented]);
    assert!(engine.is_idle(), "no cycle is due");
}

#[test]
fn a_tick_s_delta_is_limited_and_its_stage_of_a_chassis_s_own_comes_first() {
    // Catches a delta past the host's limit, a time elapsed that is not the sum of the deltas, and a `Tick` that is
    // not its cycle's first stage given no time at all.
    let mut engine = cycling::<Full>(Duration::from_nanos(25));
    engine.cycle(Time::ZERO);
    assert_eq!(step_at(&mut engine, 10), [Seen::Received]);
    assert_eq!(step_at(&mut engine, 20), [Seen::Ticked(0, 0)]);
    assert_eq!(step_at(&mut engine, 30), [got(0, "pass 1")]);
    assert_eq!(step_at(&mut engine, 40), [Seen::Rendered]);
    assert_eq!(step_at(&mut engine, 50), [Seen::Presented]);

    // 50 since the last cycle began, limited to 25.
    engine.cycle(Time::from_nanos(50));
    assert_eq!(step_at(&mut engine, 60), [Seen::Received]);
    assert_eq!(step_at(&mut engine, 70), [Seen::Ticked(25, 25)]);
    assert_eq!(step_at(&mut engine, 80), [got(0, "pass 1")]);
    assert_eq!(step_at(&mut engine, 90), [Seen::Rendered]);
    assert_eq!(step_at(&mut engine, 100), [Seen::Presented]);
    engine.cycle(Time::from_nanos(100));
    assert_eq!(step_at(&mut engine, 110), [Seen::Received]);
    assert_eq!(step_at(&mut engine, 120), [Seen::Ticked(25, 50)]);
}

#[test]
fn a_program_asked_to_stop_mid_cycle_finishes_the_cycle_first() {
    // Catches `Stop` sent in the middle of a cycle, so an actor stops between drawing and showing.
    let mut engine = cycling::<Full>(Duration::from_secs(1));
    engine.cycle(Time::ZERO);
    assert_eq!(step_at(&mut engine, 10), [Seen::Received]);
    engine.quit();
    engine.cycle(Time::from_nanos(15));
    assert_eq!(step_at(&mut engine, 20), [Seen::Ticked(0, 0)]);
    assert_eq!(step_at(&mut engine, 30), [got(0, "pass 1")]);
    assert_eq!(step_at(&mut engine, 40), [Seen::Rendered]);
    assert_eq!(step_at(&mut engine, 50), [Seen::Presented]);
    assert_eq!(step_at(&mut engine, 60), [Seen::Stopping(0)], "no cycle begins once asked to stop");
    // The cycler neither answered `Stop` nor held it, and what the engine says of that comes back to it, and is
    // dropped, in the next step.
    assert!(step_at(&mut engine, 70).is_empty());
    assert_eq!(engine.lifecycle(), Lifecycle::Stopped);
}

/// What became of a spawn, as a nest can keep it: [`Refused`] does not encode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Spawned {
    /// It stood up.
    Stood,
    /// It was refused for subscribing to a stage of this kind, which its engine does not run.
    Unstaged(KindId),
    /// It was refused for anything else, which these tests never expect.
    Other,
}

impl Spawned {
    /// What `spawned` came to.
    fn of<T>(spawned: &Result<T, Refused>) -> Self {
        match spawned {
            Ok(_) => Self::Stood,
            Err(Refused::Unstaged(kind)) => Self::Unstaged(*kind),
            Err(_) => Self::Other,
        }
    }
}

/// An actor that spawns a cycler beneath it as it begins, and a latecomer in a step, on the note it sends itself, and
/// keeps what became of each.
#[derive(Bound)]
#[kind(name = "aether.test.nest")]
struct Nest {
    /// What became of each spawn, in order.
    spawned: Vec<Spawned>,
}

impl ChildOf for Nest {
    type Parent = Top;
}

impl Actor for Nest {
    type Config = Note;
    type Key = ();
    type Handles = (Note,);
    type BeginSends = (ToType<Self, Note>,);
    type EndSends = ();

    fn begin(_: Note, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let spawned = turn.spawn::<Cycler>(&(), Note { text: String::new() });
        turn.to(turn.me()).send(&Note { text: String::new() })?;

        Ok(Self { spawned: vec![Spawned::of(&spawned)] })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Note> for Nest {
    type Sends = ();

    fn handle(&mut self, _: Note, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let spawned = turn.spawn::<Latecomer>(&(), Note { text: String::new() });
        self.spawned.push(Spawned::of(&spawned));

        Ok(())
    }
}

/// An engine of four parts running the cycle `C`.
fn running<C: Cycle>() -> Engine {
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    Engine::new(parts, 1024, 0, 1 << 20, 0).with_cycle::<C>().ready()
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
    let mut spawned = Vec::new();
    for engine in [&mut full, &mut drawing] {
        let Ok(id) = engine.add_root::<Nest>(&(), Note { text: String::new() }) else {
            panic!("the nest begins");
        };
        engine.step(Time::ZERO, &Stopped);
        let Some(nest) = engine.table().seat(id).and_then(|seat| engine.state::<Nest>(Seated { id, seat })) else {
            panic!("the nest is live");
        };
        spawned.push(nest.spawned.clone());
    }
    let refused = Spawned::Unstaged(KindId::of::<Receive>());
    assert_eq!(spawned, [[Spawned::Stood, Spawned::Stood], [refused, refused]]);
    let late = drawing.add_root::<Latecomer>(&(), Note { text: String::new() });
    assert_eq!(late, Err(unstaged), "a subscription its handlers declare counts as its `begin`'s does");
}

/// An actor that declares its subscription to `Receive` in a handler, not as it begins.
#[derive(Bound)]
#[kind(name = "aether.test.latecomer")]
struct Latecomer;

impl ChildOf for Latecomer {
    type Parent = Anywhere;
}

impl Actor for Latecomer {
    type Config = Note;
    type Key = ();
    type Handles = (On<Receive>, Note);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Note, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<On<Receive>> for Latecomer {
    type Sends = ();

    fn handle(&mut self, _: Receive, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Note> for Latecomer {
    type Sends = ();

    fn handle(&mut self, _: Note, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.subscribe::<Receive>();

        Ok(())
    }
}

/// An actor that subscribes to `Stop` as it begins, holds it when it is sent, and acts on the notes it is sent: `answer`
/// answers it, `let go` drops it unanswered, and `close` closes the actor holding it.
#[derive(Bound)]
#[kind(name = "aether.test.drainer")]
struct Drainer {
    /// The `Stop` it holds: none until it is sent, and none once answered or let go.
    held: Vec<Held<Drained>>,
}

impl ChildOf for Drainer {
    type Parent = Top;
}

impl Actor for Drainer {
    type Config = Note;
    type Key = ();
    type Handles = (On<Stop>, Note);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Note, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Stop>();

        Ok(Self { held: Vec::new() })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<On<Stop>> for Drainer {
    type Sends = (ToSender<Drained>,);

    fn handle(&mut self, _: Stop, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.held.push(turn.hold()?);

        Ok(())
    }
}

impl Handles<Note> for Drainer {
    type Sends = ();

    fn handle(&mut self, note: Note, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
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
#[kind(name = "aether.test.waker")]
struct Waker;

impl ChildOf for Waker {
    type Parent = Top;
}

impl Actor for Waker {
    type Config = Note;
    type Key = ();
    type Handles = ();
    type BeginSends = (Tells<Path<Drainer>, Note>,);
    type EndSends = ();

    fn begin(note: Note, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.to(path::<Drainer>(())).send(&note)?;

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// Stops an engine whose drainer holds `Stop`, then has a waker send it `told`, and answers with whether the program
/// had stopped after five steps of holding, and after five more once the drainer was told.
fn drained(told: &str) -> (Lifecycle, Lifecycle) {
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0).ready();
    engine.record_turns();
    let Ok(_) = engine.add_root::<Drainer>(&(), Note { text: String::new() }) else {
        panic!("the drainer begins");
    };
    engine.start();
    engine.quit();
    assert_eq!(step(&mut engine), [Seen::Stopping(0)]);
    for _ in 0..4 {
        step(&mut engine);
    }
    let held = engine.lifecycle();

    let Ok(_) = engine.add_root::<Waker>(&(), Note { text: String::from(told) }) else {
        panic!("the waker begins");
    };
    assert_eq!(step(&mut engine), [got(0, told)]);
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
    let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0).ready();
    engine.start();
    engine.quit();

    assert_eq!(engine.lifecycle(), Lifecycle::Stopped);
    assert_eq!(engine.step_number(), 0);
}

/// A root, a singleton, that a castaway reaches at its typed path and is never made.
#[derive(Bound)]
#[kind(name = "aether.test.shore")]
struct Shore;

impl ChildOf for Shore {
    type Parent = Top;
}

impl Actor for Shore {
    type Config = Note;
    type Key = ();
    type Handles = (Note,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Note, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Note> for Shore {
    type Sends = ();

    fn handle(&mut self, _: Note, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// A root that subscribes to `Receive` as it begins and reaches the shore at its typed path: refused for both.
#[derive(Bound)]
#[kind(name = "aether.test.castaway")]
struct Castaway;

impl ChildOf for Castaway {
    type Parent = Top;
}

impl Actor for Castaway {
    type Config = Note;
    type Key = ();
    type Handles = (On<Receive>,);
    type BeginSends = (Tells<Path<Shore>, Note>,);
    type EndSends = ();

    fn begin(_: Note, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<On<Receive>> for Castaway {
    type Sends = ();

    fn handle(&mut self, _: Receive, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
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
