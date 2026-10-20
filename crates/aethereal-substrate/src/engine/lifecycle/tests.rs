//! The lifecycle the engine runs: each stage sent to its subscribers, and the next only once it has finished
//! (ADR-0020).

use super::super::*;
use crate::stages::key;
use aethereal_actor::{Ending, Fault, Handles, Start, Stop, Subscribes, Time, To, ToType, ToWithoutMail, Turn};
use aethereal_data::{Bound, Kind, KindName, Plain};
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
