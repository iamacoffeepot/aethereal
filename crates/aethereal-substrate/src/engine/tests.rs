//! The engine's tests: what a step runs, and what is there for the next one.

use super::*;
use crate::watch::Notice;
use crate::{ByActor, Entry, Standing, child_id};
use aethereal_actor::{
    Ended, Ending, Fault, Handles, How, Level, Made, Table, To, ToSender, ToType, ToWithoutMail, Turn,
};
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
/// - `bye N`, which a child sends as it ends, has it send that child `late`;
/// - `bounce N` answers the sender `bounce N-1` until `N` is zero, and `pair` answers it `bounce 2`, then `last`.
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
    type Handles = (Note, Ended);
    type BeginSends = (ToType<Self, Note>,);
    type EndSends = (ToType<Self, Note>,);

    fn begin(config: NodeConfig, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        for text in config.to_self {
            let me = turn.me();
            turn.to::<Self>(ToWithoutMail::Held(me)).send(&Note { text })?;
        }

        Ok(Self { number: config.number })
    }

    fn end(self, turn: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        say(format!("{} ended {:?}", self.number, turn.why()));
        turn.to::<Self>(ToWithoutMail::Parent).send(&Note { text: format!("bye {}", self.number) })?;
        let fails = self.number == 13;
        if fails {
            return Err(Fault::new(&Note { text: String::from("unlucky") })?);
        }

        Ok(())
    }
}

impl Handles<Note> for Node {
    type Sends = (ToType<Self, Note>, ToSender<Note>);

    fn handle(&mut self, note: Note, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        let Note { text } = note;
        say(format!("{} got {text}", self.number));

        let words: Vec<&str> = text.splitn(3, ' ').collect();
        match words[..] {
            ["close"] => turn.close(),
            ["fail"] => return Err(Fault::new(&Note { text })?),
            ["fail", line] => {
                turn.log(Level::Warn, line);
                turn.to::<Self>(To::Parent).send(&Note { text: String::from("never") })?;

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
            ["up", said] => turn.to::<Self>(To::Parent).send(&Note { text: format!("{said} from {}", self.number) })?,
            ["spawn" | "watch", number, text] => {
                let number = number.parse().unwrap_or_default();
                if let Ok(child) = turn.spawn::<Self>(&number, NodeConfig { number, to_self: Vec::new() }) {
                    let watches = words[0] == "watch";
                    if watches {
                        turn.watch(child);
                    }
                    turn.to::<Self>(Self::child(number)).send(&Note { text: String::from(text) })?;
                }
            }
            ["tell", number, text] => {
                let child = Self::child(number.parse().unwrap_or_default());
                turn.to::<Self>(child).send(&Note { text: String::from(text) })?;
            }
            ["both"] => {
                turn.to::<Self>(To::Parent).send(&Note { text: String::from("watchme") })?;
                let me = turn.me();
                turn.to::<Self>(To::Held(me)).send(&Note { text: String::from("close") })?;
            }
            ["watchme", ..] => {
                let sender = turn.sender();
                turn.watch(sender);
            }
            ["self", ..] => {
                let (_, rest) = text.split_once(' ').unwrap_or_default();
                let me = turn.me();
                turn.to::<Self>(To::Held(me)).send(&Note { text: String::from(rest) })?;
            }
            ["where", ..] => say(format!("{} at depth {}", self.number, turn.chain().depth)),
            ["bye", number] => {
                let watches = number == "5";
                if watches {
                    let sender = turn.sender();
                    turn.watch(sender);
                }
                let child = Self::child(number.parse().unwrap_or_default());
                turn.to::<Self>(child).send(&Note { text: String::from("late") })?;
            }
            _ => {}
        }

        Ok(())
    }
}

impl Handles<Ended> for Node {
    type Sends = ();

    fn handle(&mut self, ended: Ended, _: &mut Turn<'_, ()>) -> Result<(), Fault> {
        say(format!("{} saw {}", self.number, how(ended.how())));

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
    let mut engine = Engine::new(parts, 1024, 0, hops);
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
    let free: Vec<u64> = engine.seats.iter().flat_map(|seats| seats.held().0.map(SeatNumber::number)).collect();
    let numbered = engine.seats.iter().zip(0..).flat_map(|(seats, first)| (first..seats.held().1).step_by(4));
    let each_once = numbered.clone().all(|seat| taken.iter().chain(&free).filter(|held| **held == seat).count() == 1);
    assert!(each_once);
    assert_eq!(taken.len() + free.len(), numbered.count());
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
    engine.step(&clock);
    said();

    let workers = Workers { active: 2, max: 2, wake_nanos: 0 };
    let Split::Shares(shares) = engine.plan(workers).into_split() else {
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
    // Catches a child a run stood up left unresolved or unproven to the run's later turns, an actor that ended in the
    // run still found by them, and either seen by a turn outside the run before the step is folded.
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
    let stood = ByActor::default();

    let in_run = Since { table: engine.table(), stood: &stood, sees: Sees::Run(&run) };
    assert_eq!(Table::resolve(&in_run, &child_path), Some(child));
    assert!(Table::is_live(&in_run, child.id));
    assert_eq!(Made::seat(&in_run, child.id), Some(child.seat));
    assert_eq!(Table::resolve(&in_run, &root_path), None);
    assert!(!Table::is_live(&in_run, root));

    let outside = Since { table: engine.table(), stood: &stood, sees: Sees::Step };
    assert_eq!(Table::resolve(&outside, &child_path), None);
    assert!(!Table::is_live(&outside, child.id));
    assert_eq!(Made::seat(&outside, child.id), None);
    assert!(Table::is_live(&outside, root));
}
