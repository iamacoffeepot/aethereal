//! The store's tools: the agent checks a binary in, lists what the store holds, and pins an entry or clears its pin
//! (ADR-0039).

use crate::{CheckIn, CheckedIn, ListStored, Pin, Pinned, Store, Stored, StoredBinary, Unpin};
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anywhere, Asks, Beginning, ChildOf, Ending, Fault, Handles, Held, Path,
    ProtocolRef, ToSender, Turn, path,
};
use aethereal_data::{Bound, Budget, Plain, Schema, SchemaNodes, from_bytes, to_bytes};
use aethereal_tools::{Call, Called, Done, Tool, ToolSheet, Tooling};
use std::collections::BTreeMap;

/// The tool that checks a binary in.
const CHECK_IN: &str = "check_in";
/// The tool that lists the store's entries.
const LIST_STORED: &str = "list_stored";
/// The tool that pins an entry.
const PIN: &str = "pin";
/// The tool that clears an entry's pin.
const UNPIN: &str = "unpin";

/// The most bytes reading an input may take: a path and a name are short.
const INPUT_BYTES: usize = 8 * 1024;

/// What the tools keep for an ask they made: the call it was made for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.tools_asking")]
pub struct Asking {
    number: u64,
}

/// What the tools are made from: nothing.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.store_tools_config")]
pub struct StoreToolsConfig;

/// What a call asks of the store: its input, which is the store's own ask (ADR-0037).
enum Order {
    CheckIn(CheckIn),
    List,
    Pin(Pin),
    Unpin(Unpin),
}

/// The store's four tools for the agent (ADR-0039), served by one worker: `check_in` asks [`Store`] to copy the file
/// at a path on the host's machine in under a name, `list_stored` asks it for every entry, and `pin` and `unpin` ask it
/// to keep an entry from pruning or to clear that. Each takes the store's own ask as its input, and answers what the
/// store answers, as both are plain (ADR-0037); each case short of done is refused with a reason the agent reads.
#[derive(Bound)]
#[kind(name = "aether.store.store_tools")]
pub struct StoreTools {
    /// Each call held, by the number of the ask made for it. The kind of the answer says which ask it was.
    calls: BTreeMap<u64, Held<Called>>,
    next: u64,
}

impl ChildOf for StoreTools {
    type Parent = Anywhere;
}

impl Actor for StoreTools {
    type Config = StoreToolsConfig;
    type Key = ();
    type Handles = (Call, Answer<CheckedIn, Asking>, Answer<Stored, Asking>, Answer<Pinned, Asking>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: StoreToolsConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { calls: BTreeMap::new(), next: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Tool for StoreTools {
    fn sheets() -> Vec<ToolSheet> {
        vec![
            ToolSheet {
                name: String::from(CHECK_IN),
                description: String::from(
                    "Copies the file at a path on the hub's machine into the store, and points a name at it: 1 to \
                     128 of ASCII letters, digits, `_`, `-`, and `.`, and not the 64 lowercase hexadecimal digits \
                     that spell a hash. Answers the BLAKE3 hash of its bytes, how many bytes it holds, and whether \
                     they were not already stored. A name checked in again moves to the newer bytes.",
                ),
                input: SchemaNodes::of(&CheckIn::SHAPE),
                output: SchemaNodes::of(&StoredBinary::SHAPE),
            },
            ToolSheet {
                name: String::from(LIST_STORED),
                description: String::from(
                    "Lists every binary in the store, the last checked in first: each one's hash, the names that \
                     point at it, how many bytes it holds, whether it is pinned, and whether an engine holds it.",
                ),
                input: SchemaNodes::of(&ListStored::SHAPE),
                output: SchemaNodes::of(&Stored::SHAPE),
            },
            ToolSheet {
                name: String::from(PIN),
                description: String::from("Keeps the binary of a hash from being pruned, until it is unpinned."),
                input: SchemaNodes::of(&Pin::SHAPE),
                output: SchemaNodes::of(&Done::SHAPE),
            },
            ToolSheet {
                name: String::from(UNPIN),
                description: String::from(
                    "Clears the pin of the binary of a hash. Its names and the engines that hold it still keep it.",
                ),
                input: SchemaNodes::of(&Unpin::SHAPE),
                output: SchemaNodes::of(&Done::SHAPE),
            },
        ]
    }

    fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling> {
        me.protocol()
    }
}

impl Handles<Call> for StoreTools {
    type Sends = (
        Asks<Path<Store>, CheckIn, Asking>,
        Asks<Path<Store>, ListStored, Asking>,
        Asks<Path<Store>, Pin, Asking>,
        Asks<Path<Store>, Unpin, Asking>,
        ToSender<Called>,
    );

    /// Reads the call's input, or refuses it; holds the call, and asks the store.
    fn handle(&mut self, call: Call, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let order = match order(&call) {
            Ok(order) => order,
            Err(refusal) => {
                turn.reply(&refusal)?;
                return Ok(());
            }
        };
        let held = turn.hold()?;
        let asking = Asking { number: self.next };
        self.next += 1;
        let store = path::<Store>(());
        match order {
            Order::CheckIn(check_in) => turn.to(store).ask(&check_in, &asking)?,
            Order::List => turn.to(store).ask(&ListStored, &asking)?,
            Order::Pin(pin) => turn.to(store).ask(&pin, &asking)?,
            Order::Unpin(unpin) => turn.to(store).ask(&unpin, &asking)?,
        };
        self.calls.insert(asking.number, held);

        Ok(())
    }
}

impl Handles<Answer<CheckedIn, Asking>> for StoreTools {
    type Sends = ();

    /// The check-in's hash, size, and freshness, or its refusal.
    fn handle(
        &mut self,
        (answered, Asking { number }): (Answered<CheckedIn>, Asking),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Some(held) = self.calls.remove(&number) else {
            return Ok(());
        };
        let called = match answered {
            Answered::Yes(CheckedIn::Stored(stored)) => Called::Output(to_bytes(&stored)?),
            Answered::Yes(CheckedIn::Missing) => refused("nothing is at that path"),
            Answered::Yes(CheckedIn::NotAFile) => refused("that is not a regular file"),
            Answered::Yes(CheckedIn::TooLarge) => refused("the file is larger than the store takes"),
            Answered::Yes(CheckedIn::Full) => refused("the store has no room for it, even after pruning all it may"),
            Answered::Yes(CheckedIn::Failed) => refused("the hub could not read the file or write the store"),
            Answered::No(_) => unanswered(),
        };
        turn.answer(held, &called)?;

        Ok(())
    }
}

impl Handles<Answer<Stored, Asking>> for StoreTools {
    type Sends = ();

    /// Every entry the store lists.
    fn handle(
        &mut self,
        (answered, Asking { number }): (Answered<Stored>, Asking),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Some(held) = self.calls.remove(&number) else {
            return Ok(());
        };
        let called = match answered {
            Answered::Yes(stored) => Called::Output(to_bytes(&stored)?),
            Answered::No(_) => unanswered(),
        };
        turn.answer(held, &called)?;

        Ok(())
    }
}

impl Handles<Answer<Pinned, Asking>> for StoreTools {
    type Sends = ();

    /// That the pin was set or cleared, or the refusal of a hash the store does not hold.
    fn handle(
        &mut self,
        (answered, Asking { number }): (Answered<Pinned>, Asking),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Some(held) = self.calls.remove(&number) else {
            return Ok(());
        };
        let called = match answered {
            Answered::Yes(Pinned::Done) => Called::Output(to_bytes(&Done {})?),
            Answered::Yes(Pinned::NotStored) => refused("no binary of that hash is stored"),
            Answered::No(_) => unanswered(),
        };
        turn.answer(held, &called)?;

        Ok(())
    }
}

/// A refusal saying `why`.
fn refused(why: impl Into<String>) -> Called {
    Called::Refused(why.into())
}

/// The refusal of a call the store did not answer.
fn unanswered() -> Called {
    refused("the store did not answer")
}

/// What `call` asks of the store, or its refusal: a tool these are not, or input that is not of the tool's, with the
/// rule it breaks, such as a name's.
fn order(call: &Call) -> Result<Order, Called> {
    let budget = &mut Budget::new(INPUT_BYTES);
    let read = match call.tool.as_str() {
        CHECK_IN => from_bytes(&call.input, budget).map(Order::CheckIn),
        LIST_STORED => from_bytes::<ListStored>(&call.input, budget).map(|_| Order::List),
        PIN => from_bytes(&call.input, budget).map(Order::Pin),
        UNPIN => from_bytes(&call.input, budget).map(Order::Unpin),
        other => return Err(refused(format!("there is no tool `{other}` here"))),
    };

    read.map_err(|fault| refused(format!("the input of `{}`: {fault}", call.tool)))
}
