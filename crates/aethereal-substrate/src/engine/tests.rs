//! The engine's tests: what a step runs, and what is there for the next one.

use super::*;
use crate::stream::Pending;
use crate::{Entry, Standing, View, child_id};
use aethereal_actor::{Ended, Fault, How, Item, Level, To, ToWithoutMail};
use aethereal_data::{Budget, Kind, KindName, Plain, TypedKey};
use std::cell::RefCell;
use std::mem;
use std::sync::atomic::{AtomicU64, Ordering};

thread_local! {
    /// What the actors of this thread's test have said they did, in order.
    static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Notes that an actor did something.
fn say(line: String) {
    SAID.with_borrow_mut(|said| said.push(line));
}

/// What has been said since this was last asked.
fn said() -> Vec<String> {
    SAID.with_borrow_mut(mem::take)
}

/// A line of text one node sends another. A node acts on the text it is sent.
#[derive(Plain)]
struct Note {
    text: String,
}

impl Kind for Note {
    const NAME: KindName = KindName::new("aether.test.note");
}

/// A node's number, and the notes it sends itself as it begins.
#[derive(Plain)]
struct NodeConfig {
    number: u64,
    to_self: Vec<String>,
}

impl Kind for NodeConfig {
    const NAME: KindName = KindName::new("aether.test.node_config");
}

/// An actor that says what it is sent, and acts on it:
///
/// - `close` closes it, and `fail` fails its turn, and `fail text` logs `text` and sends its parent `never`
///   first;
/// - `spawn N text` spawns the node `N` beneath it and sends that child `text`, and `watch N text` watches the
///   child as well;
/// - `tell N text` sends the node `N` beneath it `text`;
/// - `both` sends its parent `watchme` and itself `close`, and `watchme` has it watch the sender;
/// - `up text` sends its parent `text from <number>`, and `reply` answers the sender with `re`;
/// - the node `13` fails its closing turn, so what it sends there is dropped;
/// - `bye 5` has it watch the sender as well;
/// - `where` has it say how deep in its chain that note is;
/// - `bye N`, which a child sends as it ends, has it send that child `late`.
#[derive(Plain)]
struct Node {
    number: u64,
}

impl Kind for Node {
    const NAME: KindName = KindName::new("aether.test.node");
}

impl Node {
    /// The target that is the node `number` beneath this one.
    fn child(number: u64) -> To {
        To::ChildInstanced(Self::NAME, number.to_name())
    }
}

impl Actor for Node {
    type Config = NodeConfig;
    type Key = u64;

    fn begin(config: NodeConfig, turn: &mut Beginning<'_>) -> Result<Self, Fault> {
        for text in config.to_self {
            turn.send(ToWithoutMail::Held(turn.me()), &Note { text })?;
        }

        Ok(Self { number: config.number })
    }

    fn handle(&mut self, item: Item<'_>, turn: &mut Turn<'_>) -> Result<(), Fault> {
        let mail = match item {
            Item::Mail(mail) => mail,
            Item::Ended(ended) => {
                say(format!("{} saw {}", self.number, how(ended.how())));

                return Ok(());
            }
        };
        let Some(Note { text }) = mail.read()? else {
            return Ok(());
        };
        say(format!("{} got {text}", self.number));

        let words: Vec<&str> = text.splitn(3, ' ').collect();
        match words[..] {
            ["close"] => turn.close(),
            ["fail"] => return Err(Fault::new(&Note { text })?),
            ["fail", line] => {
                turn.log(Level::Warn, line);
                turn.send(To::Parent, &Note { text: String::from("never") })?;

                return Err(Fault::new(&Note { text: String::from(line) })?);
            }
            ["reply"] => turn.send(To::Sender, &Note { text: String::from("re") })?,
            ["up", said] => turn.send(To::Parent, &Note { text: format!("{said} from {}", self.number) })?,
            ["spawn" | "watch", number, text] => {
                let number = number.parse().unwrap_or_default();
                if let Ok(child) = turn.spawn::<Self>(&number, NodeConfig { number, to_self: Vec::new() }) {
                    let watches = words[0] == "watch";
                    if watches {
                        turn.watch(child);
                    }
                    turn.send(Self::child(number), &Note { text: String::from(text) })?;
                }
            }
            ["tell", number, text] => {
                turn.send(Self::child(number.parse().unwrap_or_default()), &Note { text: String::from(text) })?;
            }
            ["both"] => {
                turn.send(To::Parent, &Note { text: String::from("watchme") })?;
                turn.send(To::Held(turn.me()), &Note { text: String::from("close") })?;
            }
            ["watchme", ..] => turn.watch(mail.sender()),
            ["where", ..] => say(format!("{} at depth {}", self.number, mail.depth())),
            ["bye", number] => {
                let watches = number == "5";
                if watches {
                    turn.watch(mail.sender());
                }
                turn.send(Self::child(number.parse().unwrap_or_default()), &Note { text: String::from("late") })?;
            }
            _ => {}
        }

        Ok(())
    }

    fn end(self, turn: &mut Ending<'_>) -> Result<(), Fault> {
        say(format!("{} ended {:?}", self.number, turn.why()));
        turn.send(ToWithoutMail::Parent, &Note { text: format!("bye {}", self.number) })?;
        let fails = self.number == 13;
        if fails {
            return Err(Fault::new(&Note { text: String::from("unlucky") })?);
        }

        Ok(())
    }
}

/// How a watched node ended, in a word, with the note it failed over.
fn how(how: &How) -> String {
    match how {
        How::Closed => String::from("closed"),
        How::Earlier => String::from("earlier"),
        How::Failed(fault) => match fault.read::<Note>(&mut Budget::new(64)) {
            Ok(Some(Note { text })) => format!("failed over {text}"),
            _ => String::from("failed"),
        },
    }
}

/// A clock that never moves: these tests are about what turns leave, not how long they take.
struct Stopped;

impl Clock for Stopped {
    fn nanos(&self) -> u64 {
        0
    }
}

/// An engine with the root node `1`, which sends itself `to_self` as it begins.
fn engine(to_self: &[&str]) -> (Engine, ActorId) {
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let mut engine = Engine::new(parts, 1024, 0);
    let to_self = to_self.iter().map(|text| String::from(*text)).collect();
    let Ok(root) = engine.add_root::<Node>(&1, NodeConfig { number: 1, to_self }) else {
        panic!("the root begins");
    };

    (engine, root)
}

/// Runs one step and answers with what its turns said.
fn step(engine: &mut Engine) -> Vec<String> {
    engine.step(&Stopped);

    said()
}

/// The id of the node `number` beneath `parent`.
fn node(parent: ActorId, number: u64) -> ActorId {
    child_id(parent, &PathSegment::new(Node::NAME, &number))
}

#[test]
fn mail_is_handled_in_a_step_after_it_is_sent() {
    // Catches mail handled as it is sent, inside the turn or the step that sent it, and a reply sent to anyone
    // but the sender.
    let (mut engine, _) = engine(&["reply"]);
    assert_eq!(said(), [""; 0]);
    assert!(!engine.is_idle());

    assert_eq!(step(&mut engine), ["1 got reply"]);
    assert_eq!(step(&mut engine), ["1 got re"]);
    assert!(engine.is_idle());
    assert_eq!(engine.step_number(), 2);
}

#[test]
fn child_takes_mail_sent_in_the_step_that_spawned_it() {
    // Catches mail delivered before its step's spawns are folded: the child would not be there for it.
    let (mut engine, root) = engine(&["spawn 2 up hello"]);

    assert_eq!(step(&mut engine), ["1 got spawn 2 up hello"]);
    assert_eq!(engine.table().standing(node(root, 2)), Standing::Live);
    assert_eq!(step(&mut engine), ["2 got up hello"]);
    assert_eq!(step(&mut engine), ["1 got hello from 2"]);
}

#[test]
fn closed_actor_handles_no_more_mail_and_ends_in_the_next_step() {
    // Catches mail queued behind a close being handled, a closing turn run in the step that closed, and mail
    // delivered to an actor that has ended.
    let (mut engine, root) = engine(&["spawn 2 close", "spawn 3 up hi"]);
    assert_eq!(step(&mut engine), ["1 got spawn 2 close", "1 got spawn 3 up hi"]);

    // The two children run in the order of their ids, which nothing here depends on.
    let mut second = step(&mut engine);
    second.sort();
    assert_eq!(second, ["2 got close", "3 got up hi"]);
    assert_eq!(engine.table().standing(node(root, 2)), Standing::Ended);

    let mut third = step(&mut engine);
    third.sort();
    assert_eq!(third, ["1 got hi from 3", "2 ended Closed"]);
    assert_eq!(step(&mut engine), ["1 got bye 2"]);
    assert_eq!(step(&mut engine), [""; 0]);
    assert!(engine.is_idle());
}

#[test]
fn mail_queued_behind_a_close_is_dropped() {
    // Catches an ended actor handling what it was sent before it ended.
    let (mut engine, _) = engine(&["close", "reply"]);

    assert_eq!(step(&mut engine), ["1 got close"]);
    assert_eq!(step(&mut engine), ["1 ended Closed"]);
    assert_eq!(step(&mut engine), [""; 0]);
    assert!(engine.is_idle());
}

#[test]
fn failed_turn_ends_its_actor_and_sends_nothing() {
    // Catches a failed turn's asks being applied, a failed actor left live, and a closing turn that is not told
    // its actor failed.
    let (mut engine, root) = engine(&["spawn 2 fail"]);
    step(&mut engine);

    assert_eq!(step(&mut engine), ["2 got fail"]);
    assert_eq!(engine.table().standing(node(root, 2)), Standing::Ended);
    assert_eq!(step(&mut engine), ["2 ended Faulted"]);
    assert_eq!(step(&mut engine), ["1 got bye 2"]);
}

#[test]
fn mail_from_two_senders_arrives_in_the_order_of_their_ids() {
    // Catches a queue left in the order its senders happened to run: the same turns must leave the same queue.
    let (mut engine, root) = engine(&["spawn 2 up x", "spawn 3 up x", "spawn 4 up x"]);
    step(&mut engine);
    step(&mut engine);

    let mut numbers = [2, 3, 4];
    numbers.sort_by_key(|number| node(root, *number).bits());
    assert_eq!(step(&mut engine), numbers.map(|number| format!("1 got x from {number}")));
}

#[test]
fn key_spawned_earlier_in_the_step_is_taken() {
    // Catches an actor's second turn of a step reading its own key as free, which would make two actors under
    // one id.
    let (mut engine, _) = engine(&["spawn 2 up first", "spawn 2 up second"]);
    step(&mut engine);

    assert_eq!(step(&mut engine), ["2 got up first"]);
}

#[test]
fn watcher_reacts_to_an_ending_once_with_its_fault_after_its_last_mail() {
    // Catches a watcher never run for an ending, run without the fault, run before the ended actor's last mail
    // has reached it, and run for it again in a later step.
    let (mut engine, _) = engine(&["watch 2 fail"]);
    step(&mut engine);
    step(&mut engine);

    assert_eq!(step(&mut engine), ["2 ended Faulted"]);
    assert_eq!(step(&mut engine), ["1 got bye 2", "1 saw failed over fail"]);
    assert_eq!(step(&mut engine), [""; 0]);
    assert!(engine.is_idle());
}

#[test]
fn watcher_with_no_mail_is_run_for_an_ending() {
    // Catches an ending that waits for mail to arrive before its watcher runs.
    // The node 13 fails its closing turn, so its parent is sent nothing.
    let (mut engine, _) = engine(&["watch 13 close"]);
    step(&mut engine);
    step(&mut engine);
    assert_eq!(step(&mut engine), ["13 ended Closed"]);

    assert_eq!(step(&mut engine), ["1 saw closed"]);
}

#[test]
fn watch_asked_for_in_the_step_its_actor_ends_is_shown() {
    // Catches a watch that only counts once folded: its actor's ending is in the same step's effects, and would
    // be missed.
    let (mut engine, _) = engine(&["spawn 2 both"]);
    step(&mut engine);
    step(&mut engine);

    let mut third = step(&mut engine);
    third.sort();
    assert_eq!(third, ["1 got watchme", "2 got close"]);
    assert_eq!(step(&mut engine), ["2 ended Closed"]);
    assert_eq!(step(&mut engine), ["1 got bye 2", "1 saw closed"]);
}

#[test]
fn watch_asked_for_before_the_closing_turn_has_run_is_shown_how() {
    // Catches a watch that lands between an actor's close and its closing turn being shown only that the actor
    // ended earlier, when how it ended is still to come.
    let (mut engine, _) = engine(&["spawn 2 up watchme", "tell 2 close"]);
    step(&mut engine);
    assert_eq!(step(&mut engine), ["2 got up watchme", "2 got close"]);

    let mut third = step(&mut engine);
    third.sort();
    assert_eq!(third, ["1 got watchme from 2", "2 ended Closed"]);
    assert_eq!(step(&mut engine), ["1 got bye 2", "1 saw closed"]);
}

#[test]
fn watch_of_an_actor_whose_life_is_over_is_shown() {
    // Catches a watch on an actor whose closing turn has run that waits for an ending which has already been.
    let (mut engine, _) = engine(&["spawn 5 close"]);
    step(&mut engine);
    step(&mut engine);
    assert_eq!(step(&mut engine), ["5 ended Closed"]);

    assert_eq!(step(&mut engine), ["1 got bye 5"]);
    assert_eq!(step(&mut engine), ["1 saw earlier"]);
}

#[test]
fn failed_turn_keeps_its_log_lines_and_leaves_its_fault() {
    // Catches a failed turn's lines dropped with the rest of what it asked for, the rest kept with the lines,
    // and a failure that leaves nothing to read.
    let (mut engine, root) = engine(&["spawn 2 fail low"]);
    step(&mut engine);
    step(&mut engine);
    let Ok(fault) = Fault::new(&Note { text: String::from("low") }) else {
        panic!("a short note encodes");
    };

    let kept: Vec<&Entry> = engine.log(node(root, 2)).collect();
    assert_eq!(kept, [&Entry::Line { level: Level::Warn, text: String::from("low") }, &Entry::Failed(fault)]);
    assert_eq!(step(&mut engine), ["2 ended Faulted"]);
    assert_eq!(step(&mut engine), ["1 got bye 2"]);
}

#[test]
fn failed_closing_turn_leaves_its_fault_and_sends_nothing() {
    // Catches a closing turn whose failure leaves nothing to read, and one whose mail goes out all the same.
    let (mut engine, root) = engine(&["spawn 13 close"]);
    step(&mut engine);
    step(&mut engine);
    let Ok(fault) = Fault::new(&Note { text: String::from("unlucky") }) else {
        panic!("a short note encodes");
    };

    assert_eq!(step(&mut engine), ["13 ended Closed"]);
    assert_eq!(engine.log(node(root, 13)).collect::<Vec<_>>(), [&Entry::EndFailed(fault)]);
    assert_eq!(step(&mut engine), [""; 0]);
}

#[test]
fn an_actor_counts_what_it_sends_across_its_begin_and_its_steps() {
    // Catches a count of mail sent that is lost between steps, or not kept from a root's `begin`, so the actor
    // starts counting again and gives a later piece of mail the id of an earlier one.
    let (mut engine, root) = engine(&["spawn 2 reply"]);
    assert_eq!(step(&mut engine), ["1 got spawn 2 reply"]);
    assert_eq!(step(&mut engine), ["2 got reply"]);
    assert_eq!(step(&mut engine), ["1 got re"]);

    let sent = |actor: ActorId| {
        let seat = engine.table.seat(actor).unwrap_or(SeatNumber::new(u64::MAX));
        let me = Seated { id: actor, seat };

        engine.lives.shard(seat.number()).life(me).map(lives::Life::sent)
    };
    assert_eq!(sent(root), Some(2), "one piece from its begin, one from its first turn");
    assert_eq!(sent(node(root, 2)), Some(1));
}

#[test]
fn depth_counts_each_send_down_a_chain() {
    // Catches a depth that is not carried through a spawn and a reply, and one that starts from the wrong end:
    // a root's first mail is at depth one.
    let (mut engine, _) = engine(&["where", "spawn 2 up where"]);

    assert_eq!(step(&mut engine), ["1 got where", "1 at depth 1", "1 got spawn 2 up where"]);
    step(&mut engine);
    assert_eq!(step(&mut engine), ["1 got where from 2", "1 at depth 3"]);
}

#[test]
fn item_placed_under_a_seat_reaches_only_the_actor_it_was_placed_for() {
    // Catches an item placed under a seat handed to whichever actor sits there, when the actor it was placed for
    // has left the seat or never sat in it.
    let (mut engine, root) = engine(&[]);
    let seat = engine.table.seat(root).map_or(u64::MAX, SeatNumber::number);
    let context = Context { cause: None, root: MailId { sender: root, count: 0 }, depth: 1 };
    let place = |engine: &mut Engine, to: ActorId| {
        let ended = Ended::new(Seated { id: node(root, 2), seat: SeatNumber::new(7) }, How::Closed);
        let pending = Pending::Ended(Box::new((ended, context)));
        let record = stream::Record::Item { to: to.bits(), order: (2, u64::MAX), pending };
        Streams::fold(engine.streams.shard_mut(seat), seat, record);
    };

    place(&mut engine, node(root, 3));
    assert_eq!(step(&mut engine), [""; 0]);
    place(&mut engine, root);
    assert_eq!(step(&mut engine), ["1 saw closed"]);
}

/// A clock that moves a microsecond each time it is read, so every turn is measured to cost something.
#[derive(Default)]
struct Ticking(AtomicU64);

impl Clock for Ticking {
    fn nanos(&self) -> u64 {
        self.0.fetch_add(1_000, Ordering::Relaxed)
    }
}

#[test]
fn children_spawned_on_two_workers_each_take_a_seat_of_their_own() {
    // Catches two workers giving one seat to two children, a seat a part numbered or took back left neither taken nor
    // free, and a child spawned in an earlier turn of the step that a send by name cannot find.
    let (mut engine, root) = engine(&["spawn 2 spawn 4 up a", "spawn 3 spawn 5 up b", "tell 3 spawn 6 up c"]);
    let clock = Ticking::default();
    engine.step(&clock);
    said();

    let workers = Workers { active: 2, max: 2, wake_nanos: 0 };
    let Split::Shares(shares) = engine.plan(workers).into_split() else {
        panic!("two workers share the step");
    };
    assert_eq!(shares.len(), 2);
    let reading = engine.reading();
    let done = shares.into_iter().map(|share| reading.run(share, &clock)).collect();
    engine.apply(done);
    said();

    let mut third = step(&mut engine);
    third.sort();
    assert_eq!(third, ["4 got up a", "5 got up b", "6 got up c"]);
    let (two, three) = (node(root, 2), node(root, 3));
    let actors = [root, two, three, node(two, 4), node(three, 5), node(three, 6)];
    let taken: Vec<u64> = actors.iter().filter_map(|id| engine.table().seat(*id)).map(SeatNumber::number).collect();
    assert_eq!(taken.len(), actors.len());
    let free: Vec<u64> = engine.seating.iter().flat_map(|part| &part.free).map(|seat| seat.number()).collect();
    let numbered = engine.seating.iter().zip(0..).flat_map(|(part, first)| (first..part.next).step_by(4));
    let each_once = numbered.clone().all(|seat| taken.iter().chain(&free).filter(|held| **held == seat).count() == 1);
    assert!(each_once);
    assert_eq!(taken.len() + free.len(), numbered.count());
}
