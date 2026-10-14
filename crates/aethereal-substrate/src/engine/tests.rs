//! The engine's tests: what a step runs, and what is there for the next one.

use super::*;
use aethereal_actor::{Fault, How, Level, ToWithoutMail};
use aethereal_data::{Budget, Kind, KindName, Plain, TypedKey};
use std::cell::RefCell;

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
/// - the node `13` fails its closing turn;
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

    fn handle(&mut self, mail: Mail<'_>, turn: &mut Turn<'_>) -> Result<(), Fault> {
        let Some(Note { text }) = mail.read()? else {
            return Ok(());
        };
        if !turn.ended().is_empty() {
            let saw: Vec<String> = turn.ended().iter().map(|ended| how(ended.how())).collect();
            say(format!("{} saw {}", self.number, saw.join(", ")));
        }
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

/// An engine with the root node `1`, which sends itself `to_self` as it begins.
fn engine(to_self: &[&str]) -> (Engine, ActorId) {
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let mut engine = Engine::new(parts, 1024);
    let to_self = to_self.iter().map(|text| String::from(*text)).collect();
    let Ok(root) = engine.add_root::<Node>(&1, NodeConfig { number: 1, to_self }) else {
        panic!("the root begins");
    };

    (engine, root)
}

/// Runs one step and answers with what its turns said.
fn step(engine: &mut Engine) -> Vec<String> {
    engine.step();

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
fn watcher_is_shown_an_ending_once_with_its_fault() {
    // Catches a watcher never shown an ending, shown it without the fault, shown it before it has mail to run a
    // turn for, and shown it again in a later step.
    let (mut engine, root) = engine(&["watch 2 fail"]);
    step(&mut engine);
    step(&mut engine);

    assert_eq!(step(&mut engine), ["2 ended Faulted"]);
    assert_eq!(step(&mut engine), ["1 saw failed over fail", "1 got bye 2"]);
    assert!(engine.seats[&root.bits()].ended.is_empty());
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
    assert_eq!(step(&mut engine), ["1 saw closed", "1 got bye 2"]);
}

#[test]
fn watch_of_an_actor_that_ended_earlier_is_shown() {
    // Catches a watch on an ended actor that waits for an ending which has already been.
    let (mut engine, _) = engine(&["spawn 2 up watchme", "tell 2 close"]);
    step(&mut engine);
    assert_eq!(step(&mut engine), ["2 got up watchme", "2 got close"]);

    let mut third = step(&mut engine);
    third.sort();
    assert_eq!(third, ["1 got watchme from 2", "2 ended Closed"]);
    assert_eq!(step(&mut engine), ["1 saw earlier", "1 got bye 2"]);
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
fn depth_counts_each_send_down_a_chain() {
    // Catches a depth that is not carried through a spawn and a reply, and one that starts from the wrong end:
    // a root's first mail is at depth one.
    let (mut engine, _) = engine(&["where", "spawn 2 up where"]);

    assert_eq!(step(&mut engine), ["1 got where", "1 at depth 1", "1 got spawn 2 up where"]);
    step(&mut engine);
    assert_eq!(step(&mut engine), ["1 got where from 2", "1 at depth 3"]);
}
