//! The engine's tests: what a step runs, and what is there for the next one.

use super::*;
use crate::watch::Notice;
use crate::{ByActor, Entry, child_id, path_id};
use aethereal_actor::Time;
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anywhere, Asks, Carries, ChildOf, Ended, Ending, Fault, Handles, How, Level,
    Path, Recursive, Table, Tells, To, ToAny, ToSender, ToType, ToWithoutMail, Top, Turn, path,
};
use aethereal_data::{Bound, Budget, Kind, KindName, Plain, TypedKey};
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

/// A node's number, the notes it sends itself as it begins, and the children it spawns as it begins, each of which
/// closes on its first note.
#[derive(Plain)]
struct NodeConfig {
    number: u64,
    to_self: Vec<String>,
    spawns: Vec<u64>,
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
/// - `nest N M` spawns the node `N`, which spawns the node `M` beneath itself as it begins;
/// - `tell N text` sends the node `N` beneath it `text`;
/// - `both` sends its parent `watchme` and itself `close`, and `watchme` has it watch the sender;
/// - `up text` sends its parent `text from <number>`, and `reply` answers the sender with `re`;
/// - the node `13` fails its closing turn, so what it sends there is dropped;
/// - `bye 5` has it watch the sender as well;
/// - `where` has it say how deep in its chain that note is;
/// - `bye N`, which a child sends as it ends, has it send that child `late`;
/// - `bounce N` answers the sender `bounce N-1` until `N` is zero, and `pair` answers it `bounce 2`, then `last`;
/// - `keep` sends the sender `late` through the reference to it, as an actor of its own type;
/// - `many N text` sends itself `textI` for each `I` below `N`, a single digit: notes of one length, back to back;
/// - `fan N,M,.. text` spawns each node `N`, `M` and so on beneath it, and sends them all `text` in one send;
/// - `sibling N` says whether it finds the node `N` beneath its parent.
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

impl ChildOf for Node {
    type Parent = Recursive;
}

impl Actor for Node {
    type Config = NodeConfig;
    type Key = u64;
    type Handles = (Note, Ended);
    type BeginSends = (ToType<Self, Note>,);
    type EndSends = (ToType<Self, Note>,);

    fn begin(config: NodeConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        for number in config.spawns {
            let to_self = vec![String::from("close")];
            let _ = turn.spawn::<Self>(&number, NodeConfig { number, to_self, spawns: Vec::new() });
        }
        for text in config.to_self {
            let me = turn.me();
            turn.send(ToWithoutMail::Held(me), &Note { text })?;
        }

        Ok(Self { number: config.number })
    }

    fn end(self, turn: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        say(format!("{} ended {:?}", self.number, turn.why()));
        turn.send(ToWithoutMail::Parent, &Note { text: format!("bye {}", self.number) })?;
        let fails = self.number == 13;
        if fails {
            return Err(Fault::new(&Note { text: String::from("unlucky") })?);
        }

        Ok(())
    }
}

impl Handles<Note> for Node {
    type Sends = (ToType<Self, Note>, ToSender<Note>);

    fn handle(&mut self, note: Note, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Note { text } = note;
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
            ["reply"] => turn.reply(&Note { text: String::from("re") })?,
            ["pair", ..] => {
                turn.reply(&Note { text: String::from("bounce 2") })?;
                turn.reply(&Note { text: String::from("last") })?;
            }
            ["bounce", left] => {
                let left: u64 = left.parse().unwrap_or_default();
                if left > 0 {
                    turn.reply(&Note { text: format!("bounce {}", left - 1) })?;
                }
            }
            ["up", said] => turn.send(To::Parent, &Note { text: format!("{said} from {}", self.number) })?,
            ["spawn" | "watch", number, text] => {
                let number = number.parse().unwrap_or_default();
                let config = NodeConfig { number, to_self: Vec::new(), spawns: Vec::new() };
                if let Ok(child) = turn.spawn::<Self>(&number, config) {
                    let watches = words[0] == "watch";
                    if watches {
                        turn.watch(child);
                    }
                    turn.send(Self::child(number), &Note { text: String::from(text) })?;
                }
            }
            ["nest", number, beneath] => {
                let number = number.parse().unwrap_or_default();
                let spawns = vec![beneath.parse().unwrap_or_default()];
                let _ = turn.spawn::<Self>(&number, NodeConfig { number, to_self: Vec::new(), spawns });
            }
            ["tell", number, text] => {
                let child = Self::child(number.parse().unwrap_or_default());
                turn.send(child, &Note { text: String::from(text) })?;
            }
            ["both"] => {
                turn.send(To::Parent, &Note { text: String::from("watchme") })?;
                let me = turn.me();
                turn.send(To::Held(me), &Note { text: String::from("close") })?;
            }
            ["watchme", ..] => {
                let sender = turn.sender();
                turn.watch(sender);
            }
            ["self", ..] => {
                let (_, rest) = text.split_once(' ').unwrap_or_default();
                let me = turn.me();
                turn.send(To::Held(me), &Note { text: String::from(rest) })?;
            }
            ["keep", ..] => {
                let sender = turn.sender();
                turn.send(To::Held(sender), &Note { text: String::from("late") })?;
            }
            ["many", count, text] => {
                let me = turn.me();
                for index in 0..count.parse().unwrap_or(0u64) {
                    turn.send(To::Held(me), &Note { text: format!("{text}{index}") })?;
                }
            }
            ["fan", numbers, text] => {
                let children: Vec<ActorRef<Self>> = numbers
                    .split(',')
                    .filter_map(|number| {
                        let number = number.parse().unwrap_or_default();
                        turn.spawn(&number, NodeConfig { number, to_self: Vec::new(), spawns: Vec::new() }).ok()
                    })
                    .collect();
                turn.send_to_all(&children, &Note { text: String::from(text) })?;
            }
            ["where", ..] => say(format!("{} at depth {}", self.number, turn.chain().depth)),
            ["sibling", number] => {
                let number: u64 = number.parse().unwrap_or_default();
                let found = turn.parent().and_then(|parent| turn.child::<Self>(parent, &number));
                say(format!("{} {} {number}", self.number, finding(found)));
            }
            ["bye", number] => {
                let watches = number == "5";
                if watches {
                    let sender = turn.sender();
                    turn.watch(sender);
                }
                let child = Self::child(number.parse().unwrap_or_default());
                turn.send(child, &Note { text: String::from("late") })?;
            }
            _ => {}
        }

        Ok(())
    }
}

impl Handles<Ended> for Node {
    type Sends = ();

    fn handle(&mut self, ended: Ended, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        say(format!("{} saw {}", self.number, how(ended.how())));

        Ok(())
    }
}

/// Whether a node looked for was found, in words.
fn finding(found: Option<ActorRef<Node>>) -> &'static str {
    match found {
        Some(_) => "found",
        None => "found no",
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

/// An engine with the root node `1`, which sends itself `to_self` as it begins, and fuses no mail.
fn engine(to_self: &[&str]) -> (Engine, ActorId) {
    fusing(0, to_self)
}

/// An engine with the root node `1`, which sends itself `to_self` as it begins, whose parts each run at most `hops`
/// fused hops a step.
fn fusing(hops: usize, to_self: &[&str]) -> (Engine, ActorId) {
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let mut engine = Engine::new(parts, 1024, 0, 1 << 20, hops).ready();
    let to_self = to_self.iter().map(|text| String::from(*text)).collect();
    let Ok(root) = engine.add_root::<Node>(&1, NodeConfig { number: 1, to_self, spawns: Vec::new() }) else {
        panic!("the root begins");
    };

    (engine, root)
}

/// Runs one step and answers with what its turns said.
fn step(engine: &mut Engine) -> Vec<String> {
    engine.step(Time::ZERO, &Stopped);

    said()
}

/// The live actor `id` stands for, in its seat.
fn seated(engine: &Engine, id: ActorId) -> Seated {
    let Some(seat) = engine.table().seat(id) else {
        panic!("the actor is live");
    };

    Seated { id, seat }
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
    assert!(engine.table().is_live(node(root, 2)));
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
    assert!(!engine.table().is_live(node(root, 2)));

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
    assert!(!engine.table().is_live(node(root, 2)));
    assert_eq!(step(&mut engine), ["2 ended Faulted"]);
    assert_eq!(step(&mut engine), ["1 got bye 2"]);
}

#[test]
fn send_through_a_reference_to_an_actor_that_has_ended_is_dropped() {
    // Catches a send checked against the type of an actor the table no longer holds, which would stop the engine
    // over mail that is only to be dropped, and mail handed to an actor that has ended.
    let (mut engine, _) = engine(&["spawn 2 up keep", "tell 2 close"]);
    step(&mut engine);
    assert_eq!(step(&mut engine), ["2 got up keep", "2 got close"]);

    let mut third = step(&mut engine);
    third.sort();
    assert_eq!(third, ["1 got keep from 2", "2 ended Closed"]);
    assert_eq!(step(&mut engine), ["1 got bye 2"]);
    assert_eq!(step(&mut engine), [""; 0]);
}

#[test]
fn a_child_is_found_beneath_a_reference_held_while_it_is_live() {
    // Catches `turn.child` looking beneath the turn's own actor rather than the reference it is handed, finding a key
    // no child holds, and finding a child that has ended (ADR-0022). Node 2 looks for its siblings beneath its parent.
    let (mut engine, _) = engine(&[
        "spawn 3 x",
        "spawn 2 sibling 3",
        "self tell 2 sibling 5",
        "self tell 3 close",
        "self self self tell 2 sibling 3",
    ]);
    let found: Vec<String> = (0..5).flat_map(|_| step(&mut engine)).filter(|line| line.contains(" found ")).collect();

    assert_eq!(found, ["2 found 3", "2 found no 5", "2 found no 3"]);
}

#[test]
fn a_child_is_found_in_the_table_only_of_its_kind() {
    // Catches the table's lookup of a child beneath a reference answering an actor of another kind at the place, or a
    // place no child holds (ADR-0022).
    let (mut engine, root) = engine(&["spawn 2 x"]);
    step(&mut engine);
    let two = PathSegment::new(Node::NAME, &2u64);
    let three = PathSegment::new(Node::NAME, &3u64);
    let child = seated(&engine, child_id(root, &two));

    assert_eq!(Table::reach_child(engine.table(), root, &two, KindId::of::<Node>()), Some(child));
    assert_eq!(Table::reach_child(engine.table(), root, &two, KindId::of::<Note>()), None);
    assert_eq!(Table::reach_child(engine.table(), root, &three, KindId::of::<Node>()), None);
}

#[test]
fn key_stays_taken_after_its_child_has_ended() {
    // Catches a parent's key freed once its child has ended and left the table, which would bring a closed path
    // back to life.
    let (mut engine, _) = engine(&["spawn 2 close", "self self spawn 2 up again"]);
    step(&mut engine);
    step(&mut engine);

    let mut third = step(&mut engine);
    third.sort();
    assert_eq!(third, ["1 got spawn 2 up again", "2 ended Closed"]);
    assert_eq!(step(&mut engine), ["1 got bye 2"]);
    assert_eq!(step(&mut engine), [""; 0]);
}

#[test]
fn key_a_child_used_as_it_began_stays_taken() {
    // Catches the children a child stands up in its `begin` going unnoted, as when a child is seated before its
    // parent has a seat to note it in.
    let (mut engine, _) = engine(&["nest 2 3", "self self tell 2 spawn 3 up again"]);
    step(&mut engine);
    let mut second = step(&mut engine);
    second.sort();
    assert_eq!(second, ["1 got self tell 2 spawn 3 up again", "3 got close"]);

    let mut third = step(&mut engine);
    third.sort();
    assert_eq!(third, ["1 got tell 2 spawn 3 up again", "3 ended Closed"]);
    let mut fourth = step(&mut engine);
    fourth.sort();
    assert_eq!(fourth, ["2 got bye 3", "2 got spawn 3 up again"]);
    assert_eq!(step(&mut engine), [""; 0]);
}

#[test]
fn root_key_stays_taken_after_its_root_has_ended() {
    // Catches a root's key freed once the table no longer holds its ended root, which would bring a closed path
    // back to life.
    let (mut engine, _) = engine(&["close"]);
    step(&mut engine);
    step(&mut engine);

    let again = engine.add_root::<Node>(&1, NodeConfig { number: 1, to_self: Vec::new(), spawns: Vec::new() });
    assert!(matches!(again, Err(Refused::KeyTaken)));
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
    let child = seated(&engine, node(root, 2));
    step(&mut engine);
    let Ok(fault) = Fault::new(&Note { text: String::from("low") }) else {
        panic!("a short note encodes");
    };

    let kept: Vec<&Entry> = engine.log(child).collect();
    assert_eq!(kept, [&Entry::Line { level: Level::Warn, text: String::from("low") }, &Entry::Failed(fault)]);
    assert_eq!(step(&mut engine), ["2 ended Faulted"]);
    assert_eq!(step(&mut engine), ["1 got bye 2"]);
}

#[test]
fn failed_closing_turn_leaves_its_fault_and_sends_nothing() {
    // Catches a closing turn whose failure leaves nothing to read, and one whose mail goes out all the same.
    let (mut engine, root) = engine(&["spawn 13 close"]);
    step(&mut engine);
    let child = seated(&engine, node(root, 13));
    step(&mut engine);
    let Ok(fault) = Fault::new(&Note { text: String::from("unlucky") }) else {
        panic!("a short note encodes");
    };

    assert_eq!(step(&mut engine), ["13 ended Closed"]);
    assert_eq!(engine.log(child).collect::<Vec<_>>(), [&Entry::EndFailed(fault)]);
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
    let context = Context { root: MailId { sender: root, count: 0 }, depth: 1 };
    let place = |engine: &mut Engine, to: ActorId| {
        let ended = Ended::new(Seated { id: node(root, 2), seat: SeatNumber::new(7) }, How::Closed);
        let to = Seated { id: to, seat: SeatNumber::new(seat) };
        engine.streams.shard_mut(seat).end(Notice { to, ended, context });
        engine.waiting.push(engine.part(to.seat));
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
    engine.step(Time::ZERO, &clock);
    said();

    let workers = Workers { active: 2, max: 2, wake_nanos: 0, split: SplitCost::NONE };
    let Split::Shares(shares) = engine.plan(workers, Time::ZERO).into_split() else {
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
    let free: Vec<u64> = engine.seats.iter().flat_map(|seats| seats.held().0.map(SeatNumber::number)).collect();
    let numbered = engine.seats.iter().zip(0..).flat_map(|(seats, first)| (first..seats.held().1).step_by(4));
    let each_once = numbered.clone().all(|seat| taken.iter().chain(&free).filter(|held| **held == seat).count() == 1);
    assert!(each_once);
    assert_eq!(taken.len() + free.len(), numbered.count());
}

#[test]
fn keys_used_on_two_workers_stay_taken_after_their_children_have_ended() {
    // Catches the children stood up in a step shared among workers going unnoted in their parents' seats when their
    // parts are folded, which would free a key once its child has ended and left the table.
    let notes = ["spawn 2 spawn 20 close", "spawn 3 spawn 30 close", "self self self tell 2 spawn 20 up again"];
    let (mut engine, _) = engine(&[notes[0], notes[1], notes[2], "self self self tell 3 spawn 30 up again"]);
    let clock = Ticking::default();
    engine.step(Time::ZERO, &clock);
    said();

    let workers = Workers { active: 2, max: 2, wake_nanos: 0, split: SplitCost::NONE };
    let Split::Shares(shares) = engine.plan(workers, Time::ZERO).into_split() else {
        panic!("two workers share the step");
    };
    let reading = engine.reading();
    let done = shares.into_iter().map(|share| reading.run(share, &clock)).collect();
    engine.apply(done);
    let mut heard = said();
    while !engine.is_idle() {
        heard.extend(step(&mut engine));
    }

    assert!(heard.contains(&String::from("2 got spawn 20 up again")));
    assert!(heard.contains(&String::from("3 got spawn 30 up again")));
    let respawned = heard.iter().any(|line| line.contains("again from"));
    assert!(!respawned, "{heard:?}");
}

#[test]
fn children_seated_in_a_part_another_worker_ran_are_kept() {
    // Catches a worker's children being seated in a part another worker still holds, and lost when that worker's
    // part is put back over them. Node 2 sits beside the root in part 0, which is full, so its children take the
    // seats set aside for part 0 in the other parts, and its fourth takes one in part 2, where node 3 runs on the
    // other worker.
    let notes = ["spawn 2 spawn 20 up hi", "tell 2 spawn 21 up hi", "tell 2 spawn 22 up hi", "tell 2 spawn 23 up hi"];
    let (mut engine, root) = engine(&[notes[0], notes[1], notes[2], notes[3], "spawn 3 spawn 30 up hi"]);
    let clock = Ticking::default();
    engine.step(Time::ZERO, &clock);
    said();

    let workers = Workers { active: 2, max: 2, wake_nanos: 0, split: SplitCost::NONE };
    let Split::Shares(shares) = engine.plan(workers, Time::ZERO).into_split() else {
        panic!("two workers share the step");
    };
    let reading = engine.reading();
    let done = shares.into_iter().map(|share| reading.run(share, &clock)).collect();
    engine.apply(done);
    said();

    let mut third = step(&mut engine);
    third.sort();
    assert_eq!(third, ["20 got up hi", "21 got up hi", "22 got up hi", "23 got up hi", "30 got up hi"]);
    let (two, three) = (node(root, 2), node(root, 3));
    let grandchildren = [node(two, 20), node(two, 21), node(two, 22), node(two, 23), node(three, 30)];
    let kept = grandchildren.iter().all(|id| engine.table().seat(*id).is_some());
    assert!(kept);
}

#[test]
fn chain_between_actors_in_one_part_runs_within_the_step() {
    // Catches a chain not fused between actors in one part, a fused send also delivered as mail, and a child not
    // seated in its parent's part while that part has room.
    let (mut fused, _) = fusing(8, &["spawn 2 up hi"]);
    assert_eq!(step(&mut fused), ["1 got spawn 2 up hi"]);
    assert_eq!(step(&mut fused), ["2 got up hi", "1 got hi from 2"]);
    assert!(fused.is_idle());

    let (mut unfused, _) = engine(&["spawn 2 up hi"]);
    assert_eq!(step(&mut unfused), ["1 got spawn 2 up hi"]);
    assert_eq!(step(&mut unfused), ["2 got up hi"]);
    assert_eq!(step(&mut unfused), ["1 got hi from 2"]);
}

#[test]
fn chain_runs_at_most_the_budget_of_hops_in_a_step() {
    // Catches a part running more hops in a step than its budget, or stopping a chain short of it.
    let (mut engine, _) = fusing(3, &["spawn 2 bounce 0", "tell 2 bounce 7"]);
    step(&mut engine);

    let mut steps = Vec::new();
    while !engine.is_idle() {
        steps.push(step(&mut engine));
    }
    // Each step runs the items waiting, then three hops.
    let second = ["2 got bounce 0", "2 got bounce 7", "1 got bounce 6", "2 got bounce 5", "1 got bounce 4"];
    let third = ["2 got bounce 3", "1 got bounce 2", "2 got bounce 1", "1 got bounce 0"];
    assert_eq!(steps, [second.to_vec(), third.to_vec()]);
}

#[test]
fn chains_begun_by_different_items_all_run_in_the_step_in_the_order_their_hops_were_made() {
    // Catches a part fusing only the first chain its items begin, and hops run newest first. Node 2's `self self where`
    // begins a chain to itself and its `reply` one to its parent; the parent's `re` runs between node 2's two hops.
    let (mut engine, _) = fusing(8, &["spawn 2 self self where", "tell 2 reply"]);
    step(&mut engine);

    let second =
        ["2 got self self where", "2 got reply", "2 got self where", "1 got re", "2 got where", "2 at depth 4"];
    assert_eq!(step(&mut engine), second);
    assert!(engine.is_idle());
}

#[test]
fn each_run_sees_only_the_children_it_stood_up_and_a_part_begins_with_none() {
    // Catches two runs of one part sharing what they stood up, and a run kept for its room still holding a child
    // from the part a worker ran before.
    let (engine, root) = fusing(8, &[]);
    let Some(root_path) = engine.table().path(root) else {
        panic!("the root is in the table");
    };
    let segment = PathSegment::new(Node::NAME, &2u64);
    let Ok(child_path) = root_path.child(&segment) else {
        panic!("a child of the root fits");
    };
    let child = Seated { id: child_id(root, &segment), seat: SeatNumber::new(9) };

    let mut fusing = Fusing::default();
    fusing.begin(8);
    let first = fusing.begin_run();
    let second = fusing.begin_run();
    fusing.run_mut(first).stand([(child, child_path.clone())]);
    assert_eq!(fusing.run(first).resolve(&child_path), Some(child));
    assert_eq!(fusing.run(second).resolve(&child_path), None);

    fusing.begin(8);
    let again = fusing.begin_run();
    assert_eq!(fusing.run(again).resolve(&child_path), None);
}

#[test]
fn hop_never_overtakes_mail_its_sender_sent_as_mail() {
    // Catches a hop handed over in the step its sender mailed the same receiver as mail, ahead of that mail. The root
    // answers `pair` with a fused `bounce 2` and `last` as mail, so its `bounce 0` must wait behind `last`.
    let (mut engine, _) = fusing(8, &["spawn 2 up pair"]);
    step(&mut engine);

    assert_eq!(step(&mut engine), ["2 got up pair", "1 got pair from 2", "2 got bounce 2", "1 got bounce 1"]);
    assert_eq!(step(&mut engine), ["2 got last", "2 got bounce 0"]);
}

#[test]
fn each_send_a_part_could_fuse_is_counted_once_by_what_became_of_it() {
    // Catches a send counted under the wrong rule, counted twice or not at all, and sends counted when the engine
    // fuses nothing. The root's `spawn` mails an unseated child; the child's `up pair` fuses to the root, whose `pair`
    // fuses `bounce 2` and sends `last` after it in its turn; the child's `bounce 1` fuses back, and the root's
    // `bounce 0` is held back by the `last` it sent as mail.
    let (mut fused, _) = fusing(8, &["spawn 2 up pair"]);
    while !fused.is_idle() {
        step(&mut fused);
    }
    let counted = Hops { fused: 3, spilled: 1, unseated: 1, burst: 1, ..Hops::default() };
    assert_eq!(fused.hopped(), counted);

    let (mut unfused, _) = engine(&["spawn 2 up pair"]);
    while !unfused.is_idle() {
        step(&mut unfused);
    }
    assert_eq!(unfused.hopped(), Hops::default());
}

#[test]
fn run_sees_its_own_spawns_and_endings_and_no_other_turn_does() {
    // Catches a child a run stood up left unresolved to the run's later turns, an actor that ended in the run still
    // found by them, and either seen by a turn outside the run before the step is folded; by a resolve, and by a
    // typed path's reach, which also reads the child's kind.
    let (engine, root) = fusing(8, &[]);
    let Some(root_path) = engine.table().path(root) else {
        panic!("the root is in the table");
    };
    let segment = PathSegment::new(Node::NAME, &2u64);
    let Ok(child_path) = root_path.child(&segment) else {
        panic!("a child of the root fits");
    };
    let child = Seated { id: child_id(root, &segment), seat: SeatNumber::new(9) };
    let mut run = fuse::Run::default();
    run.stand([(child, child_path.clone())]);
    run.end(root);
    let mut stood = ByActor::default();
    stood.insert(child.id.bits(), (child_path.clone(), child.seat, Declared::of::<Node>()));
    let node = KindId::of::<Node>();
    let nowhere = |path: &ActorPath| Seated { id: path_id(path), seat: SeatNumber::NOWHERE };

    let in_run = Since {
        table: engine.table(),
        stood: &stood,
        sees: Sees::Run(&run),
        blobs: &engine.blobs,
        now: Time::ZERO,
        cycle: &[],
    };
    assert_eq!(Table::resolve(&in_run, &child_path), Some(child));
    assert!(Table::is_live(&in_run, child.id));
    assert_eq!(Table::resolve(&in_run, &root_path), None);
    assert!(!Table::is_live(&in_run, root));
    assert_eq!(Table::reach(&in_run, &child_path, node), child);
    assert_eq!(Table::reach(&in_run, &child_path, KindId::of::<Note>()), nowhere(&child_path));
    assert_eq!(Table::reach(&in_run, &root_path, node), nowhere(&root_path));
    assert_eq!(Table::reach_child(&in_run, root, &segment, node), Some(child));
    assert_eq!(Table::reach_child(&in_run, root, &segment, KindId::of::<Note>()), None);

    let outside = Since {
        table: engine.table(),
        stood: &stood,
        sees: Sees::Step,
        blobs: &engine.blobs,
        now: Time::ZERO,
        cycle: &[],
    };
    assert_eq!(Table::resolve(&outside, &child_path), None);
    assert!(!Table::is_live(&outside, child.id));
    assert!(Table::is_live(&outside, root));
    assert_eq!(Table::reach(&outside, &child_path, node), nowhere(&child_path));
    assert_eq!(Table::reach(&outside, &root_path, node), seated(&engine, root));
    assert_eq!(Table::reach_child(&outside, root, &segment, node), None);
}

#[test]
fn a_child_that_ended_in_a_run_is_not_found_by_its_later_turns() {
    // Catches `turn.child` in a run finding beneath a reference a child the table holds that ended in the run, and
    // a turn outside the run not finding it before the step is folded (ADR-0013, ADR-0022).
    let (mut engine, root) = fusing(8, &["spawn 2 x"]);
    step(&mut engine);
    let segment = PathSegment::new(Node::NAME, &2u64);
    let child = seated(&engine, child_id(root, &segment));
    let mut run = fuse::Run::default();
    run.end(child.id);
    let stood = ByActor::default();
    let node = KindId::of::<Node>();
    let since =
        |sees| Since { table: engine.table(), stood: &stood, sees, blobs: &engine.blobs, now: Time::ZERO, cycle: &[] };

    assert_eq!(Table::reach_child(&since(Sees::Run(&run)), root, &segment, node), None);
    assert_eq!(Table::reach_child(&since(Sees::Step), root, &segment, node), Some(child));
}

#[test]
fn back_to_back_mail_to_one_actor_is_one_run_handed_out_piece_by_piece() {
    // Catches a run that loses or repeats a piece, hands its pieces out of another order than they were sent, is
    // kept as an item for each piece, or counts its sender's mail short, so a later piece of mail reuses an id.
    let (mut engine, root) = engine(&["many 3 a"]);
    assert_eq!(step(&mut engine), ["1 got many 3 a"]);

    let me = seated(&engine, root);
    let pieces: Vec<u32> = engine.streams.shard(me.seat.number()).arrived().map(|(_, _, pieces)| pieces).collect();
    assert_eq!(pieces, [3], "one item, of three pieces");
    let count = engine.lives.shard(me.seat.number()).life(me).map(lives::Life::sent);
    assert_eq!(count, Some(4), "one piece from its begin, three from its first turn");

    assert_eq!(step(&mut engine), ["1 got a0", "1 got a1", "1 got a2"]);
}

#[test]
fn run_fuses_its_first_piece_and_mails_the_rest() {
    // Catches a fused run whose rest is dropped, handed over twice, or run in the step ahead of the piece fused.
    let (mut engine, _) = fusing(8, &["many 3 a"]);

    assert_eq!(step(&mut engine), ["1 got many 3 a", "1 got a0"]);
    assert_eq!(step(&mut engine), ["1 got a1", "1 got a2"]);
    assert!(engine.is_idle());
}

/// Runs one step planned for `workers` workers on this thread, shared or whole as the plan says, timed by `clock`,
/// and answers with what its turns said.
fn step_on(engine: &mut Engine, workers: usize, clock: &Ticking) -> Vec<String> {
    let workers = Workers { active: workers, max: workers, wake_nanos: 0, split: SplitCost::NONE };
    match engine.plan(workers, Time::ZERO).into_split() {
        Split::Whole(whole) => {
            let left = engine.run_whole(whole, clock);
            engine.apply_whole(left);
        }
        Split::Shares(shares) => {
            let reading = engine.reading();
            let done = shares.into_iter().map(|share| reading.run(share, clock)).collect();
            engine.apply(done);
        }
    }

    said()
}

#[test]
fn an_actor_that_sends_past_its_slice_hands_on_the_rest_in_later_steps_in_order() {
    // Catches a slice that never cuts, items an actor was cut off from lost, handed twice, or out of order, among them
    // through a backlog moved to its front, and a hop to an actor cut off in its step run ahead of what it carries.
    // Notes of two lengths, one after the other, are not joined into runs and leave a backlog of many.
    sliced(|index| {
        if index.is_multiple_of(2) {
            format!("y{index:04}")
        } else {
            format!("y{index:05}")
        }
    });
}

#[test]
fn an_actor_cut_off_inside_a_run_hands_on_the_rest_of_it_in_order() {
    // Catches a run cut at the slice that hands on pieces it already handed, skips some, or numbers what it carries
    // as the pieces it began with: notes of one length, all sent by the root's begin, are one run.
    sliced(|index| format!("y{index:04}"));
}

/// Runs a root handed 3,000 notes, named by `name`, that each tell a child, on two workers, and checks the root was
/// cut off at its slice and the child was told, and answered, every note in the order sent, with and without fusion.
fn sliced(name: impl Fn(u64) -> String) {
    for hops in [0, 64] {
        let mut notes = vec![String::from("spawn 2 hi")];
        notes.extend((0..3000).map(|index| format!("tell 2 up {}", name(index))));
        let notes: Vec<&str> = notes.iter().map(String::as_str).collect();
        let (mut engine, _) = fusing(hops, &notes);
        let clock = Ticking::default();
        let mut steps = Vec::new();
        while !engine.is_idle() {
            steps.push(step_on(&mut engine, 2, &clock));
        }

        let told: Vec<&str> = steps.iter().flatten().filter_map(|line| line.strip_prefix("2 got up ")).collect();
        let answered: Vec<&str> =
            steps.iter().flatten().filter_map(|line| line.strip_prefix("1 got ")?.strip_suffix(" from 2")).collect();
        let sent: Vec<String> = (0..3000).map(&name).collect();
        assert_eq!(told, sent, "hops={hops}");
        assert_eq!(answered, sent, "hops={hops}");
        let telling = steps.iter().filter(|said| said.iter().any(|line| line.starts_with("1 got tell"))).count();
        assert!(telling > 1, "the root is cut off at its slice, hops={hops}");
    }
}

#[test]
fn a_child_stood_up_in_a_turn_counts_what_it_sent_from_its_begin() {
    // Catches the count of a child's `begin` routed ahead of the spawn that makes its life, and dropped, so its next
    // turn numbers its mail from nought and gives a later piece the id of one its `begin` sent.
    let (mut engine, root) = engine(&["nest 2 3"]);
    assert_eq!(step(&mut engine), ["1 got nest 2 3"]);
    let grandchild = node(node(root, 2), 3);
    let me = seated(&engine, grandchild);
    let count = engine.lives.shard(me.seat.number()).life(me).map(lives::Life::sent);
    assert_eq!(count, Some(1), "3 sent itself close from its begin");
}

/// The notes a holder sends itself as it begins, and how many times it mails itself a blob of three bytes there,
/// keeping none of it.
#[derive(Plain)]
struct HolderConfig {
    to_self: Vec<String>,
    gives: u64,
}

/// A blob, given in mail, to be given on `left` more times.
#[derive(Bound)]
struct Given {
    blob: BlobRef,
    left: u64,
}

impl Kind for Given {
    const NAME: KindName = KindName::new("aether.test.given");
}

impl Kind for HolderConfig {
    const NAME: KindName = KindName::new("aether.test.holder_config");
}

/// An actor that puts, keeps, reads, and lets go of blobs as its notes say (ADR-0014):
///
/// - `put N` puts a blob of `N` bytes and keeps it, or says why it was refused;
/// - `read` says how many bytes each blob it keeps holds;
/// - `hash` says the hash of each blob it keeps;
/// - `drop` lets go of the blob it kept last;
/// - `fail` fails its turn;
/// - `later text` sends itself `text`, for the next step;
/// - `send` mails itself the blob it kept last, keeping none of it, and `send close` closes as well;
///
/// and keeps a blob it is given, saying how many bytes it holds, but for one it gave itself as it began, which it
/// reads, gives itself on while it has more times to go, and lets go of.
#[derive(Bound)]
struct Holder {
    held: Vec<BlobRef>,
    /// Whether it gave itself a blob as it began.
    gave: bool,
}

impl Kind for Holder {
    const NAME: KindName = KindName::new("aether.test.holder");
}

impl ChildOf for Holder {
    type Parent = Top;
}

impl Actor for Holder {
    type Config = HolderConfig;
    type Key = ();
    type Handles = (Note, Given);
    type BeginSends = (ToType<Self, Note>, ToType<Self, Given>);
    type EndSends = ();

    fn begin(config: HolderConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        for text in config.to_self {
            let me = turn.me();
            turn.send(ToWithoutMail::Held(me), &Note { text })?;
        }
        // Each blob it gives is one only that mail refers to, so mail that loses its pin loses its blob.
        for _ in 0..config.gives {
            let Ok(blob) = turn.put(vec![9; 3]) else {
                return Err(Fault::new(&Note { text: String::from("no room") })?);
            };
            // A note between each two keeps them from joining into one run, so the part carries many items.
            let me = turn.me();
            turn.send(ToWithoutMail::Held(me), &Given { blob, left: 1 })?;
            turn.send(ToWithoutMail::Held(me), &Note { text: String::from("between") })?;
        }

        Ok(Self { held: Vec::new(), gave: config.gives > 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Note> for Holder {
    type Sends = (ToType<Self, Note>, ToType<Self, Given>);

    fn handle(&mut self, note: Note, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Note { text } = note;
        let words: Vec<&str> = text.splitn(2, ' ').collect();
        match words[..] {
            ["put", bytes] => match turn.put(vec![7; bytes.parse().unwrap_or_default()]) {
                Ok(blob) => self.held.push(blob),
                Err(refused) => say(format!("refused {refused:?}")),
            },
            ["read"] => {
                for blob in &self.held {
                    say(format!("read {}", turn.read(blob).len()));
                }
            }
            ["hash"] => {
                for blob in &self.held {
                    say(hashed(turn.hash(blob)));
                }
            }
            ["drop"] => drop(self.held.pop()),
            ["fail"] => return Err(Fault::new(&Note { text })?),
            ["send", ..] => {
                if let Some(blob) = self.held.pop() {
                    let me = turn.me();
                    turn.send(To::Held(me), &Given { blob, left: 0 })?;
                }
                if words.get(1) == Some(&"close") {
                    turn.close();
                }
            }
            ["later", rest] => {
                let me = turn.me();
                turn.send(To::Held(me), &Note { text: String::from(rest) })?;
            }
            _ => {}
        }

        Ok(())
    }
}

impl Handles<Given> for Holder {
    type Sends = (ToType<Self, Given>,);

    fn handle(&mut self, given: Given, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let held = turn.read(&given.blob).len();
        if self.gave {
            let Given { blob, left } = given;
            if left > 0 {
                let me = turn.me();
                turn.send(To::Held(me), &Given { blob, left: left - 1 })?;
            }
        } else {
            say(format!("given {held}"));
            self.held.push(given.blob);
        }

        Ok(())
    }
}

/// An engine whose blobs hold at most `blob_bytes`, with a holder that sends itself `to_self` as it begins.
fn holding(blob_bytes: usize, to_self: &[&str]) -> Engine {
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let mut engine = Engine::new(parts, 1024, 0, blob_bytes, 0).ready();
    let to_self = to_self.iter().map(|text| String::from(*text)).collect();
    let Ok(_) = engine.add_root::<Holder>(&(), HolderConfig { to_self, gives: 0 }) else {
        panic!("the holder begins");
    };

    engine
}

/// What a holder says of a blob with the hash `hash`.
fn hashed(hash: BlobHash) -> String {
    format!("hash {:?}", hash.bytes())
}

#[test]
fn a_hash_asked_for_in_a_turn_is_kept_at_the_fold() {
    // Catches a hash a turn computed never kept, one kept before the blobs put in its step are taken in, which loses
    // the hash of a blob put and hashed in one step, and blobs hashed as they are put, which a blob nothing asks the
    // hash of should never pay for.
    let three = BlobHash::of(&[7; 3]);
    let mut engine = holding(1 << 20, &["put 3", "hash", "put 4", "later hash"]);
    assert_eq!(step(&mut engine), [hashed(three)]);
    assert_eq!(engine.blobs.kept_hashes(), [three], "only the blob asked for is hashed");

    let four = BlobHash::of(&[7; 4]);
    assert_eq!(step(&mut engine), [hashed(three), hashed(four)]);
    let mut kept = engine.blobs.kept_hashes();
    kept.sort();
    let mut both = [three, four];
    both.sort();
    assert_eq!(kept, both);
}

#[test]
fn the_engine_keeps_a_hash_it_is_asked_for_between_steps() {
    // Catches the chassis's ask for a hash answered and not kept, so a turn that asks for it later computes it again.
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0).ready();
    let Ok(blob) = engine.put_blob(vec![1, 2, 3]) else {
        panic!("three bytes fit");
    };
    assert_eq!(engine.blobs.kept_hashes(), []);

    assert_eq!(engine.hash(&blob), Some(BlobHash::of(&[1, 2, 3])));
    assert_eq!(engine.blobs.kept_hashes(), [BlobHash::of(&[1, 2, 3])]);
}

#[test]
fn a_blob_mailed_and_kept_by_no_one_else_reaches_its_receiver_as_a_hop_or_as_it_waited() {
    // Catches mail read with no pins where it was fused into a hop, whose pins lie in the arena its receiver's turn
    // writes into, or where it waited: its reference is refused, and its turn fails.
    for (parts, hops) in [(1, 0), (1, 64), (4, 0), (4, 64)] {
        let Some(parts) = NonZeroUsize::new(parts) else {
            panic!("an engine has parts");
        };
        let mut engine = Engine::new(parts, 1024, 0, 1 << 20, hops).ready();
        let to_self = ["put 3", "send"].map(String::from).to_vec();
        let Ok(_) = engine.add_root::<Holder>(&(), HolderConfig { to_self, gives: 0 }) else {
            panic!("the holder begins");
        };
        said();
        let mut heard = Vec::new();
        while !engine.is_idle() {
            heard.extend(step(&mut engine));
        }

        assert_eq!(heard, ["given 3"], "{parts} parts, {hops} hops");
    }
}

#[test]
fn a_blob_put_in_a_turn_is_read_in_that_step_and_the_ones_after() {
    // Catches a read that looks only in the store and misses what its worker put in the step, and a store that never
    // takes in what was put.
    let mut engine = holding(1 << 20, &["put 3", "read", "later read"]);
    assert_eq!(step(&mut engine), ["read 3"]);
    assert_eq!(engine.blobs_held(), (1, 3));

    assert_eq!(step(&mut engine), ["read 3"]);
}

#[test]
fn a_blob_is_freed_at_the_fold_after_its_last_reference_goes() {
    // Catches a blob kept once nothing refers to it, and the bytes it held kept against the budget.
    let mut engine = holding(1 << 20, &["put 4", "put 5", "later drop"]);
    step(&mut engine);
    assert_eq!(engine.blobs_held(), (2, 9));

    step(&mut engine);
    assert_eq!(engine.blobs_held(), (1, 4));
}

#[test]
fn a_put_past_the_budget_is_refused_until_a_blob_is_freed() {
    // Catches a put that goes past the budget, a refusal the actor is not told of, and freed bytes never given back.
    let mut engine = holding(8, &["put 6", "put 6", "later drop", "later later put 6"]);
    assert_eq!(step(&mut engine), ["refused NoRoom"]);
    assert_eq!(engine.blobs_held(), (1, 6));

    step(&mut engine);
    assert_eq!(engine.blobs_held(), (0, 0));
    assert_eq!(step(&mut engine), [""; 0]);
    assert_eq!(engine.blobs_held(), (1, 6));
}

#[test]
fn an_actor_that_fails_lets_go_of_what_it_held_when_it_ends() {
    // Catches the pins an actor's state held lost when the actor is dropped, which leaves its blobs held for good.
    let mut engine = holding(1 << 20, &["put 4", "later fail"]);
    step(&mut engine);
    assert_eq!(engine.blobs_held(), (1, 4));

    step(&mut engine);
    step(&mut engine);
    assert_eq!(engine.blobs_held(), (0, 0));
}

#[test]
fn references_held_or_dropped_between_steps_are_counted() {
    // Catches the changes made outside a step's turns never summed: a blob the chassis let go of kept for good, and
    // one it holds freed for want of its pin.
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0).ready();
    let Ok(kept) = engine.put_blob(vec![1, 2, 3]) else {
        panic!("three bytes fit");
    };
    let Ok(dropped) = engine.put_blob(vec![4; 5]) else {
        panic!("five bytes fit");
    };
    drop(dropped);
    engine.step(Time::ZERO, &Stopped);

    assert_eq!(engine.blobs_held(), (1, 3));
    assert_eq!(engine.blob(&kept), Some(&[1, 2, 3][..]));
}

#[test]
fn one_engine_sums_only_the_changes_to_its_own_blobs() {
    // Catches the changes made outside a step shared by every engine of the process, so one engine's dropped
    // reference takes a pin from another's blob that has the same number, and frees it while it is held.
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let mut kept = Engine::new(parts, 1024, 0, 1 << 20, 0).ready();
    let mut dropping = Engine::new(parts, 1024, 0, 1 << 20, 0).ready();
    let Ok(blob) = kept.put_blob(vec![1, 2, 3]) else {
        panic!("three bytes fit");
    };
    let Ok(other) = dropping.put_blob(vec![4, 5, 6]) else {
        panic!("three bytes fit");
    };
    assert_eq!(blob.id().bits(), other.id().bits(), "the two engines number their first blobs alike");
    drop(other);
    kept.step(Time::ZERO, &Stopped);
    dropping.step(Time::ZERO, &Stopped);

    assert_eq!(kept.blobs_held(), (1, 3));
    assert_eq!(dropping.blobs_held(), (0, 0));
}

#[test]
fn a_blob_sent_in_mail_lives_until_its_receiver_holds_it() {
    // Catches mail that holds no pin, so a blob nobody else refers to is freed before it is delivered, and a receiver
    // that reads a reference back with no pin of its own, so the blob goes once the mail's arena is emptied.
    let mut engine = holding(1 << 20, &["put 3", "later send"]);
    step(&mut engine);
    assert_eq!(step(&mut engine), [""; 0]);
    assert_eq!(engine.blobs_held(), (1, 3), "only the mail refers to it");

    assert_eq!(step(&mut engine), ["given 3"]);
    for _ in 0..3 {
        step(&mut engine);
    }
    assert_eq!(engine.blobs_held(), (1, 3), "the receiver keeps it once the mail is gone");
}

#[test]
fn mail_that_is_never_delivered_lets_go_of_its_blob() {
    // Catches the pin of mail dropped undelivered kept for good, which holds its blob past everything that could
    // read it.
    let mut engine = holding(1 << 20, &["put 3", "later send close"]);
    for _ in 0..6 {
        step(&mut engine);
    }

    assert_eq!(engine.blobs_held(), (0, 0));
}

#[test]
fn mail_sent_to_all_reaches_each_receiver_once_wherever_it_is_seated() {
    // Catches a send to all that leaves a receiver out, hands one of them the piece twice, or reads the shared bytes
    // wrong for a receiver in another part than its sender's, fused or not.
    for hops in [0, 64] {
        let (mut engine, root) = fusing(hops, &["fan 2,3,4,5,6,7,8,9 hello"]);
        let mut heard = Vec::new();
        while !engine.is_idle() {
            heard.extend(step(&mut engine));
        }
        heard.sort();
        let part = |id: ActorId| seated(&engine, id).seat.index() % 4;
        let elsewhere = (2..=9).filter(|number| part(node(root, *number)) != part(root)).count();
        assert!(elsewhere > 0, "some receiver is seated in another part than its sender, hops={hops}");

        let expected: Vec<String> = (2..=9).map(|number| format!("{number} got hello")).collect();
        let got: Vec<String> = heard.into_iter().filter(|line| line.ends_with("got hello")).collect();
        assert_eq!(got, expected, "hops={hops}");
    }
}

#[test]
fn mail_carried_past_its_receivers_slice_keeps_its_blob_until_it_is_handed() {
    // Catches the bytes of mail carried over into a part's backlog without the pins of what they refer to, so the
    // blob is freed once the arena it was sent in is emptied while the backlog still holds mail that refers to it:
    // mail carried as it arrived, mail moved to the front of a backlog, and a hop to an actor cut off in its step.
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    for hops in [0, 64] {
        let mut engine = Engine::new(parts, 1024, 0, 1 << 20, hops).ready();
        let Ok(_) = engine.add_root::<Holder>(&(), HolderConfig { to_self: Vec::new(), gives: 3000 }) else {
            panic!("the holder begins");
        };
        let clock = Ticking::default();
        let mut steps = 0;
        while !engine.is_idle() {
            step_on(&mut engine, 2, &clock);
            steps += 1;
        }
        assert!(steps > 10, "the holder is cut off at its slice and carries what it was given, hops={hops}");

        // A part idle for long enough gives back what it carried, and the pins with it.
        for _ in 0..=stream::IDLE_STEPS {
            step_on(&mut engine, 2, &clock);
        }
        assert_eq!(engine.blobs_held(), (0, 0), "hops={hops}");
    }
}

/// What a clocked actor is told as it begins: its number, and the note it sends itself.
#[derive(Plain)]
struct ClockedConfig {
    number: u64,
    to_self: String,
}

impl Kind for ClockedConfig {
    const NAME: KindName = KindName::new("aether.test.clocked_config");
}

/// An actor that says the time each of its turns reads, and puts a blob of three bytes as it begins, which its
/// closing turn reads (ADR-0014, ADR-0018): `spawn` spawns the actors `2` and `3` beneath it, which each send
/// themselves `close`.
#[derive(Bound)]
struct Clocked {
    number: u64,
    blob: BlobRef,
}

impl Kind for Clocked {
    const NAME: KindName = KindName::new("aether.test.clocked");
}

impl ChildOf for Clocked {
    type Parent = Recursive;
}

impl Actor for Clocked {
    type Config = ClockedConfig;
    type Key = u64;
    type Handles = (Note,);
    type BeginSends = (ToType<Self, Note>,);
    type EndSends = ();

    fn begin(config: ClockedConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let ClockedConfig { number, to_self } = config;
        say(format!("{number} began at {}", turn.now().nanos()));
        let Ok(blob) = turn.put(vec![7; 3]) else {
            return Err(Fault::new(&Note { text: String::from("no room") })?);
        };
        let me = turn.me();
        turn.send(ToWithoutMail::Held(me), &Note { text: to_self })?;

        Ok(Self { number, blob })
    }

    fn end(self, turn: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        let bytes = turn.read(&self.blob).len();
        say(format!("{} ended at {} holding {bytes}", self.number, turn.now().nanos()));

        Ok(())
    }
}

impl Handles<Note> for Clocked {
    type Sends = ();

    fn handle(&mut self, note: Note, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Note { text } = note;
        say(format!("{} got {text} at {}", self.number, turn.now().nanos()));
        match text.as_str() {
            "spawn" => {
                for number in [2, 3] {
                    let config = ClockedConfig { number, to_self: String::from("close") };
                    let _ = turn.spawn::<Self>(&number, config);
                }
            }
            "close" => turn.close(),
            _ => {}
        }

        Ok(())
    }
}

#[test]
fn every_turn_reads_the_time_its_step_was_given_and_a_closing_turn_reads_blobs() {
    // Catches a turn, a `begin` run inside one, or a closing turn reading a time other than its step's: one kept from
    // an earlier step, or none, as the bare table a closing turn was handed holds, which also held no blob for it to
    // read. A root made before any step reads the host's first moment.
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0).ready();
    let config = ClockedConfig { number: 1, to_self: String::from("spawn") };
    let Ok(_) = engine.add_root::<Clocked>(&1, config) else {
        panic!("the root begins");
    };
    assert_eq!(said(), ["1 began at 0"]);

    engine.step(Time::from_nanos(5), &Stopped);
    assert_eq!(said(), ["1 got spawn at 5", "2 began at 5", "3 began at 5"]);
    engine.step(Time::from_nanos(9), &Stopped);
    assert_eq!(sorted(said()), ["2 got close at 9", "3 got close at 9"]);
    engine.step(Time::from_nanos(12), &Stopped);
    assert_eq!(sorted(said()), ["2 ended at 12 holding 3", "3 ended at 12 holding 3"]);
}

#[test]
fn the_workers_of_a_shared_step_read_the_time_it_was_planned_with() {
    // Catches the workers of a step shared among them reading another time than the one the host planned it with.
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0).ready();
    // Each root sits in the part with fewest actors, so the two are in parts of their own.
    for number in [1, 2] {
        let config = ClockedConfig { number, to_self: String::from("tick") };
        let Ok(_) = engine.add_root::<Clocked>(&number, config) else {
            panic!("the root begins");
        };
    }
    said();

    let workers = Workers { active: 2, max: 2, wake_nanos: 0, split: SplitCost::NONE };
    let Split::Shares(shares) = engine.plan(workers, Time::from_nanos(7)).into_split() else {
        panic!("two workers share the step");
    };
    let reading = engine.reading();
    let done = shares.into_iter().map(|share| reading.run(share, &Stopped)).collect();
    engine.apply(done);
    assert_eq!(sorted(said()), ["1 got tick at 7", "2 got tick at 7"]);
}

/// `said`, in order: for turns of one step, which run in no order the tests depend on.
fn sorted(mut said: Vec<String>) -> Vec<String> {
    said.sort();

    said
}

/// The chain the `begin` of the root `root` set off (ADR-0012).
const fn begun(root: ActorId) -> MailId {
    MailId { sender: root, count: 0 }
}

#[test]
fn a_root_s_chain_finishes_once_nothing_it_set_off_is_left_to_run() {
    // Catches a chain taken for finished while mail of it waits, while an actor it ended is still to run its closing
    // turn, or while one watching that actor is still to be shown the ending, and a chain taken for open once all of
    // it has run. Everything here follows from the root's `begin`, so its chain is open exactly while a step would
    // run a turn. Node 13's closing turn fails, so what it sends is dropped and only the turn itself is left to run.
    let scripts: [&[&str]; 5] =
        [&["spawn 2 up hello"], &["watch 2 close"], &["spawn 13 close"], &["fan 2,3 up hi"], &["bounce 3", "pair"]];
    for script in scripts {
        let (mut engine, root) = engine(script);
        let mut steps = 0;
        loop {
            let finished = engine.has_finished(begun(root));
            assert_eq!(finished, engine.is_idle(), "{script:?} after {steps} steps");
            if finished {
                break;
            }
            step(&mut engine);
            steps += 1;
            assert!(steps < 50, "{script:?} settles");
        }
        assert!(steps > 1, "{script:?} runs past its first step");
    }
}

#[test]
fn a_chain_carried_past_its_receivers_slice_has_not_finished() {
    // Catches the items a part carries over into its next step left out of whether their chain has finished: an
    // actor cut off at its slice, with mail as it arrived and hops, still has turns of the chain to run.
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    for hops in [0, 64] {
        let mut engine = Engine::new(parts, 1024, 0, 1 << 20, hops).ready();
        let Ok(holder) = engine.add_root::<Holder>(&(), HolderConfig { to_self: Vec::new(), gives: 3000 }) else {
            panic!("the holder begins");
        };
        let clock = Ticking::default();
        let mut steps = 0;
        while !engine.is_idle() {
            assert!(!engine.has_finished(begun(holder)), "hops={hops} after {steps} steps");
            step_on(&mut engine, 2, &clock);
            steps += 1;
        }
        assert!(steps > 10, "the holder is cut off at its slice and carries what it was given, hops={hops}");
        assert!(engine.has_finished(begun(holder)), "hops={hops}");
    }
}

#[test]
fn one_chain_finishes_while_another_runs_on() {
    // Catches whether a chain has finished answered for every chain at once: the second root's chain ends with its
    // closing turn while the first's mail is still going.
    let (mut engine, first) = engine(&["spawn 2 spawn 3 up hello"]);
    let config = NodeConfig { number: 2, to_self: vec![String::from("close")], spawns: Vec::new() };
    let Ok(second) = engine.add_root::<Node>(&2, config) else {
        panic!("the second root begins");
    };
    step(&mut engine);
    assert!(!engine.has_finished(begun(second)), "the second root is still to run its closing turn");
    step(&mut engine);
    assert!(engine.has_finished(begun(second)));
    assert!(!engine.has_finished(begun(first)), "3 has yet to say hello");
}

/// A root that says what it is told, at the listener's name.
#[derive(Plain)]
struct Listener;

/// A root at the listener's name of another layout, so another kind.
#[derive(Plain)]
struct Impostor {
    at: u64,
}

/// What a root at the listener's name is: one that says what it is told.
macro_rules! listening {
    ($name:ident, $says:literal) => {
        impl Kind for $name {
            const NAME: KindName = KindName::new("aether.test.listener");
        }

        impl ChildOf for $name {
            type Parent = Top;
        }

        impl Actor for $name {
            type Config = Self;
            type Key = u64;
            type Handles = (Note,);
            type BeginSends = ();
            type EndSends = ();

            fn begin(listener: Self, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
                Ok(listener)
            }

            fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
                Ok(())
            }
        }

        impl Handles<Note> for $name {
            type Sends = ();

            fn handle(&mut self, note: Note, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
                say(format!("{} got {}", $says, note.text));

                Ok(())
            }
        }
    };
}

listening!(Listener, "the listener");
listening!(Impostor, "the impostor");

/// A root that, as it begins, tells the listener of key zero at its typed path what it is made from: a keyed path,
/// which its spawn does not check, so it may hold anything as the caller sends.
#[derive(Plain)]
struct Caller {
    text: String,
}

impl Kind for Caller {
    const NAME: KindName = KindName::new("aether.test.caller");
}

impl ChildOf for Caller {
    type Parent = Top;
}

impl Actor for Caller {
    type Config = Self;
    type Key = ();
    type Handles = ();
    type BeginSends = (Tells<Path<Listener>, Note>,);
    type EndSends = ();

    fn begin(caller: Self, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.send_to(path::<Listener>(0), &Note { text: caller.text.clone() })?;

        Ok(caller)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// An engine with whichever root `listening` makes at the listener's name, and a caller that tells it `text`.
fn calling(listening: impl FnOnce(&mut Engine), text: &str) -> Vec<String> {
    let mut engine = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).ready();
    listening(&mut engine);
    let Ok(_) = engine.add_root::<Caller>(&(), Caller { text: String::from(text) }) else {
        panic!("the caller begins");
    };

    step(&mut engine)
}

#[test]
fn a_send_at_a_typed_path_reaches_only_a_live_actor_of_the_paths_type() {
    // Catches a send at a path that does not find the actor live there, one handed to an actor of another kind that
    // shares its name and so its id, and one at a place that holds no actor kept rather than dropped (ADR-0022).
    let listener = calling(
        |engine| {
            let Ok(_) = engine.add_root::<Listener>(&0, Listener) else {
                panic!("the listener begins");
            };
        },
        "hello",
    );
    let impostor = calling(
        |engine| {
            let Ok(_) = engine.add_root::<Impostor>(&0, Impostor { at: 0 }) else {
                panic!("the impostor begins");
            };
        },
        "hello",
    );
    let nobody = calling(|_| {}, "hello");

    assert_eq!(listener, ["the listener got hello"]);
    assert_eq!(impostor, Vec::<String>::new(), "an actor of another kind at the path is not reached");
    assert_eq!(nobody, Vec::<String>::new());
}

/// A question an oracle answers.
#[derive(Plain)]
struct Question;

impl Kind for Question {
    const NAME: KindName = KindName::new("aether.test.question");
}

/// An oracle's answer to a question.
#[derive(Plain)]
struct Reply {
    text: String,
}

impl Kind for Reply {
    const NAME: KindName = KindName::new("aether.test.reply");
}

/// What a seer keeps while its question is out.
#[derive(Plain)]
struct Wondering;

impl Kind for Wondering {
    const NAME: KindName = KindName::new("aether.test.wondering");
}

/// A root that answers each question, at the oracle's name.
#[derive(Plain)]
struct Oracle;

/// A root at the oracle's name of another layout, so another kind.
#[derive(Plain)]
struct FalseOracle {
    at: u64,
}

/// What a root at the oracle's name is: one that answers each question with `$answers`.
macro_rules! answering {
    ($name:ident, $answers:literal) => {
        impl Kind for $name {
            const NAME: KindName = KindName::new("aether.test.oracle");
        }

        impl ChildOf for $name {
            type Parent = Top;
        }

        impl Actor for $name {
            type Config = Self;
            type Key = u64;
            type Handles = (Question,);
            type BeginSends = ();
            type EndSends = ();

            fn begin(oracle: Self, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
                Ok(oracle)
            }

            fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
                Ok(())
            }
        }

        impl Carries<Question> for $name {
            type Answers = (Reply,);
        }

        impl Handles<Question> for $name {
            type Sends = (ToSender<Reply>,);

            fn handle(&mut self, _: Question, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
                Ok(turn.reply(&Reply { text: String::from($answers) })?)
            }
        }
    };
}

answering!(Oracle, "the truth");
answering!(FalseOracle, "a lie");

/// A root that, as it begins, asks the oracle of key zero at its typed path a question and watches it, and says what it
/// hears: a keyed path, which its spawn does not check.
#[derive(Plain)]
struct Seer;

impl Kind for Seer {
    const NAME: KindName = KindName::new("aether.test.seer");
}

impl ChildOf for Seer {
    type Parent = Top;
}

impl Actor for Seer {
    type Config = Self;
    type Key = ();
    type Handles = (Answer<Reply, Wondering>, Ended);
    type BeginSends = (Asks<Path<Oracle>, Question, Wondering>,);
    type EndSends = ();

    fn begin(seer: Self, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.ask_to(path::<Oracle>(0), &Question, &Wondering)?;
        turn.watch(path::<Oracle>(0));

        Ok(seer)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Answer<Reply, Wondering>> for Seer {
    type Sends = ();

    fn handle(&mut self, (answered, _): (Answered<Reply>, Wondering), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        match answered {
            Answered::Yes(reply) => say(format!("the seer was told {}", reply.text)),
            Answered::No(why) => say(format!("the seer went unanswered, {why:?}")),
        }

        Ok(())
    }
}

impl Handles<Ended> for Seer {
    type Sends = ();

    fn handle(&mut self, ended: Ended, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        say(format!("the seer saw the oracle end {}", how(ended.how())));

        Ok(())
    }
}

/// What a seer says over a few steps, beside whichever root `oracle` makes at the oracle's name, in order.
fn seeing(oracle: impl FnOnce(&mut Engine)) -> Vec<String> {
    let mut engine = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).ready();
    oracle(&mut engine);
    let Ok(_) = engine.add_root::<Seer>(&(), Seer) else {
        panic!("the seer begins");
    };
    let mut said: Vec<String> = (0..3).flat_map(|_| step(&mut engine)).collect();
    said.sort();

    said
}

#[test]
fn an_ask_and_a_watch_at_a_typed_path_reach_only_a_live_actor_of_the_paths_type() {
    // Catches an ask at a path whose answer never comes, one handed to an actor of another kind that shares its name
    // and so its id, and one at a place that holds no actor that goes unanswered forever, rather than `Ended`; and a
    // watch at such a place that waits for an ending that will never come (ADR-0022).
    let oracle = seeing(|engine| {
        let Ok(_) = engine.add_root::<Oracle>(&0, Oracle) else {
            panic!("the oracle begins");
        };
    });
    let false_oracle = seeing(|engine| {
        let Ok(_) = engine.add_root::<FalseOracle>(&0, FalseOracle { at: 0 }) else {
            panic!("the false oracle begins");
        };
    });
    let nobody = seeing(|_| {});

    let gone = ["the seer saw the oracle end earlier", "the seer went unanswered, Ended"];
    assert_eq!(oracle, ["the seer was told the truth"], "a live oracle is not shown as ended");
    assert_eq!(false_oracle, gone, "an actor of another kind at the path is not reached");
    assert_eq!(nobody, gone);
}

/// A root, a singleton, that other types reach at its typed path.
#[derive(Plain)]
struct Lighthouse;

/// A root at the lighthouse's name of another layout, so another kind.
#[derive(Plain)]
struct FalseLighthouse {
    at: u64,
}

/// What a root at the lighthouse's name is: one that takes notes and does nothing with them.
macro_rules! lighthouse {
    ($name:ident) => {
        impl Kind for $name {
            const NAME: KindName = KindName::new("aether.test.lighthouse");
        }

        impl ChildOf for $name {
            type Parent = Top;
        }

        impl Actor for $name {
            type Config = Self;
            type Key = ();
            type Handles = (Note,);
            type BeginSends = ();
            type EndSends = ();

            fn begin(lighthouse: Self, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
                Ok(lighthouse)
            }

            fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
                Ok(())
            }
        }

        impl Handles<Note> for $name {
            type Sends = ();

            fn handle(&mut self, _: Note, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
                Ok(())
            }
        }
    };
}

lighthouse!(Lighthouse);
lighthouse!(FalseLighthouse);

/// A root that declares a send at the lighthouse's typed path in its `begin`, in its handler, or as it ends, whichever
/// of `$begins`, `$handler`, and `$ends` names it, and sends nothing: a declaration is what its spawn is checked by.
macro_rules! reaching {
    ($name:ident, $named:literal, $begins:ty, $handler:ty, $ends:ty) => {
        #[derive(Plain, Default)]
        struct $name;

        impl Kind for $name {
            const NAME: KindName = KindName::new($named);
        }

        impl ChildOf for $name {
            type Parent = Top;
        }

        impl Actor for $name {
            type Config = Self;
            type Key = ();
            type Handles = (Note,);
            type BeginSends = $begins;
            type EndSends = $ends;

            fn begin(reaching: Self, _: &mut Beginning<'_, Self, $begins>) -> Result<Self, Fault> {
                Ok(reaching)
            }

            fn end(self, _: &mut Ending<'_, Self, $ends>) -> Result<(), Fault> {
                Ok(())
            }
        }

        impl Handles<Note> for $name {
            type Sends = $handler;

            fn handle(&mut self, _: Note, _: &mut Turn<'_, Self, $handler>) -> Result<(), Fault> {
                Ok(())
            }
        }
    };
}

/// A send at the lighthouse's typed path.
type ToLighthouse = (Tells<Path<Lighthouse>, Note>,);

reaching!(Sailor, "aether.test.sailor", ToLighthouse, (), ());
reaching!(Lookout, "aether.test.lookout", (), ToLighthouse, ());
reaching!(Wrecker, "aether.test.wrecker", (), (), ToLighthouse);

/// Whether a root of type `R` is made beside whichever root `lighthouse` makes at the lighthouse's name.
fn reaches<R: Root<Config = R, Key = ()> + Default>(lighthouse: impl FnOnce(&mut Engine)) -> Result<(), Refused> {
    let mut engine = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).ready();
    lighthouse(&mut engine);

    engine.add_root::<R>(&(), R::default()).map(|_| ())
}

#[test]
fn a_root_that_reaches_a_typed_path_of_singletons_is_refused_while_no_actor_of_its_type_is_live_there() {
    // Catches a check that reads only the `begin`'s sends and not a handler's or the closing turn's, one that counts
    // an actor of another kind of the same name as live there, and one that refuses a root whose path is live, or
    // names something other than the path's type (ADR-0022).
    let missing = Err(Refused::Missing(Lighthouse::NAME));
    let made = |engine: &mut Engine| {
        let Ok(_) = engine.add_root::<Lighthouse>(&(), Lighthouse) else {
            panic!("the lighthouse begins");
        };
    };
    let impostor = |engine: &mut Engine| {
        let Ok(_) = engine.add_root::<FalseLighthouse>(&(), FalseLighthouse { at: 0 }) else {
            panic!("the false lighthouse begins");
        };
    };

    assert_eq!(reaches::<Sailor>(|_| {}), missing, "a send its begin declares");
    assert_eq!(reaches::<Lookout>(|_| {}), missing, "a send a handler declares");
    assert_eq!(reaches::<Wrecker>(|_| {}), missing, "a send its closing turn declares");
    assert_eq!(reaches::<Sailor>(impostor), missing, "an actor of another kind at the place");
    assert_eq!(reaches::<Sailor>(made), Ok(()));
    assert_eq!(reaches::<Lookout>(made), Ok(()));
    assert_eq!(reaches::<Wrecker>(made), Ok(()));
}

/// A root that, as it begins, spawns a pilot, which reaches it back at its typed path, and a smuggler, which reaches
/// the lighthouse, and says what became of each.
#[derive(Plain)]
struct Harbor;

impl Kind for Harbor {
    const NAME: KindName = KindName::new("aether.test.harbor");
}

impl ChildOf for Harbor {
    type Parent = Top;
}

impl Actor for Harbor {
    type Config = Self;
    type Key = ();
    type Handles = (Note,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(harbor: Self, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let pilot: Result<ActorRef<Pilot>, _> = turn.spawn(&(), Pilot);
        let smuggler: Result<ActorRef<Smuggler>, _> = turn.spawn(&(), Smuggler);
        say(format!("pilot {:?}, smuggler {:?}", pilot.err(), smuggler.err()));

        Ok(harbor)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Note> for Harbor {
    type Sends = ();

    fn handle(&mut self, _: Note, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// What reaches the harbor that spawns it, at its typed path.
#[derive(Plain)]
struct Pilot;

/// What reaches the lighthouse, at its typed path.
#[derive(Plain)]
struct Smuggler;

/// What a child of the harbor is: one that declares a send at a typed path as it begins.
macro_rules! harbored {
    ($name:ident, $named:literal, $reaches:ty) => {
        impl Kind for $name {
            const NAME: KindName = KindName::new($named);
        }

        impl ChildOf for $name {
            type Parent = Harbor;
        }

        impl Actor for $name {
            type Config = Self;
            type Key = ();
            type Handles = ();
            type BeginSends = (Tells<Path<$reaches>, Note>,);
            type EndSends = ();

            fn begin(child: Self, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
                Ok(child)
            }

            fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
                Ok(())
            }
        }
    };
}

harbored!(Pilot, "aether.test.pilot", Harbor);
harbored!(Smuggler, "aether.test.smuggler", Lighthouse);

#[test]
fn a_child_that_reaches_a_typed_path_of_singletons_is_refused_while_no_actor_is_live_there_but_its_spawner() {
    // Catches a turn's spawn that does not check the paths its child reaches, and one that refuses a child reaching
    // back the actor whose `begin` spawns it, which is not yet in the table (ADR-0022).
    let mut engine = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).ready();
    let Ok(_) = engine.add_root::<Harbor>(&(), Harbor) else {
        panic!("the harbor begins");
    };

    assert_eq!(said(), ["pilot None, smuggler Some(Missing(KindName(\"aether.test.lighthouse\")))"]);
}

#[test]
fn a_setup_makes_each_root_of_an_instanced_type_it_is_handed_at_once() {
    // Catches a setup that makes only the first of the roots it is handed together, or none.
    let made =
        Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).add_roots::<Listener>([(0, Listener), (1, Listener)]);
    let Ok(setup) = made else {
        panic!("the listeners begin");
    };
    let mut engine = setup.ready();

    assert_eq!(engine.add_root::<Listener>(&0, Listener), Err(Refused::KeyTaken));
    assert_eq!(engine.add_root::<Listener>(&1, Listener), Err(Refused::KeyTaken));
}

/// A root that, as it begins, spawns a squad beneath it, which tells it of its parent, and a scout and a cell, which
/// may stand anywhere and beneath their own type, and says what each tells it.
#[derive(Plain)]
struct Barracks;

impl Kind for Barracks {
    const NAME: KindName = KindName::new("aether.test.barracks");
}

impl ChildOf for Barracks {
    type Parent = Top;
}

impl Actor for Barracks {
    type Config = Self;
    type Key = ();
    type Handles = (Note,);
    type BeginSends = (ToType<Squad, Note>,);
    type EndSends = ();

    fn begin(barracks: Self, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let squad: Result<ActorRef<Squad>, _> = turn.spawn(&(), Squad);
        let scout: Result<ActorRef<Scout>, _> = turn.spawn(&(), Scout);
        let (Ok(squad), Ok(_)) = (squad, scout) else {
            return Err(Fault::new(&Note { text: String::from("a child did not begin") })?);
        };
        turn.send_to(squad, &Note { text: String::from("report") })?;

        Ok(barracks)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Note> for Barracks {
    type Sends = ();

    fn handle(&mut self, note: Note, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        say(format!("the barracks got {}", note.text));

        Ok(())
    }
}

/// A child of the barracks that, as it begins and when it is told to report, tells its parent it stands beneath it.
#[derive(Plain)]
struct Squad;

impl Kind for Squad {
    const NAME: KindName = KindName::new("aether.test.squad");
}

impl ChildOf for Squad {
    type Parent = Barracks;
}

impl Actor for Squad {
    type Config = Self;
    type Key = ();
    type Handles = (Note,);
    type BeginSends = (ToType<Barracks, Note>,);
    type EndSends = ();

    fn begin(squad: Self, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.send_to(turn.parent(), &Note { text: String::from("the squad") })?;

        Ok(squad)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Note> for Squad {
    type Sends = (ToType<Barracks, Note>,);

    fn handle(&mut self, note: Note, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        Ok(turn.send_to(turn.parent(), &Note { text: format!("the squad's {}", note.text) })?)
    }
}

/// An actor that may stand anywhere: as it begins, it tells its parent, when it has one, that it stands beneath it, and
/// says whether it is a root.
#[derive(Plain)]
struct Scout;

impl Kind for Scout {
    const NAME: KindName = KindName::new("aether.test.scout");
}

impl ChildOf for Scout {
    type Parent = Anywhere;
}

impl Actor for Scout {
    type Config = Self;
    type Key = ();
    type Handles = ();
    type BeginSends = (ToAny<(Note,), Note>,);
    type EndSends = ();

    fn begin(scout: Self, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let parent = turn.parent().and_then(|parent| turn.cast::<(Note,)>(parent));
        match parent {
            Some(parent) => turn.send_to(parent, &Note { text: String::from("the scout") })?,
            None => say(String::from("the scout is a root")),
        }

        Ok(scout)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// An actor of a tree of its own type: one of depth zero, a root, spawns one beneath it, and each says whether it has
/// a parent of its own type.
#[derive(Plain)]
struct Cell {
    depth: u64,
}

impl Kind for Cell {
    const NAME: KindName = KindName::new("aether.test.cell");
}

impl ChildOf for Cell {
    type Parent = Recursive;
}

impl Actor for Cell {
    type Config = Self;
    type Key = ();
    type Handles = ();
    type BeginSends = ();
    type EndSends = ();

    fn begin(cell: Self, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let parent: Option<ActorRef<Self>> = turn.parent();
        say(format!("cell {} has a parent: {}", cell.depth, parent.is_some()));
        if cell.depth == 0 {
            let child: Result<ActorRef<Self>, _> = turn.spawn(&(), Self { depth: 1 });
            if child.is_err() {
                return Err(Fault::new(&Note { text: String::from("the cell did not divide") })?);
            }
        }

        Ok(cell)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

#[test]
fn a_turn_is_handed_its_parent_typed_by_where_its_type_stands() {
    // Catches a parent that is not the actor a child was spawned beneath, a root of a type that may stand anywhere or
    // beneath its own handed a parent it does not have, and one beneath its own type handed none (ADR-0022).
    let made = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0)
        .add_root::<Barracks>(&(), Barracks)
        .and_then(|setup| setup.add_root::<Scout>(&(), Scout))
        .and_then(|setup| setup.add_root::<Cell>(&(), Cell { depth: 0 }));
    let Ok(setup) = made else {
        panic!("the roots begin");
    };
    let mut engine = setup.ready();
    let mut heard = said();
    heard.extend(step(&mut engine));
    heard.extend(step(&mut engine));
    heard.sort();

    let told = [
        "cell 0 has a parent: false",
        "cell 1 has a parent: true",
        "the barracks got the scout",
        "the barracks got the squad",
        "the barracks got the squad's report",
        "the scout is a root",
    ];
    assert_eq!(heard, told);
}
