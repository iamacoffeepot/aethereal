//! The library's tools, `publish` and `upgrades` (ADR-0037, ADR-0054): a module of `Code` published for an agent, and
//! what the engine records of the moves onto every kind's head, read when the agent asks.

mod record;
#[cfg(test)]
mod tests;

pub use record::{BehindGuest, GuestStanding, HeadRecord, PublishRecord, Upgrading};

use crate::{Library, ModulePublished, Numbered, PublishModule, Unloaded};
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anywhere, Asks, Beginning, ChildOf, Ending, Fault, Handles, Held, Moves, Path,
    ProtocolRef, ToSender, Turn, path,
};
use aethereal_data::{Bound, Budget, FilePath, Plain, Schema, SchemaNodes, from_bytes, to_bytes};
use aethereal_tools::{Call, Called, Output, Tool, ToolSheet, Tooling};
use core::fmt::Write as _;
use std::collections::BTreeMap;

/// The tool that publishes.
const PUBLISH: &str = "publish";
/// The tool that shows every kind's head and the guests behind them.
const UPGRADES: &str = "upgrades";
/// The most bytes reading an input may take: a path.
const INPUT_BYTES: usize = 8 * 1024;
/// The most bytes reading the kinds a module makes may take: a name each, and its publish's number.
const KINDS_BYTES: usize = 1 << 20;

/// What `publish` is called with.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.library.publish")]
pub struct Publish {
    /// The module's path in `Code`.
    #[flag(position)]
    pub path: FilePath,
}

/// What `upgrades` is called with: nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.library.read_upgrades")]
pub struct ReadUpgrades {}

/// What the tool keeps for its ask of the library: which call it answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.library.calling")]
pub struct Calling {
    call: u64,
}

/// What the tool is made from: nothing.
#[derive(Debug, Default, PartialEq, Eq, Plain)]
#[kind(name = "aether.library.library_tool_config")]
pub struct LibraryToolConfig;

/// The tools `publish` and `upgrades`. `publish` asks the [`Library`] to publish a module of `Code`, and answers as
/// soon as it has, with each kind and the number of its publish. `upgrades` answers what the engine records of the
/// moves onto every kind's head: each head with how many moved onto it, and each running guest behind its head.
#[derive(Bound)]
#[kind(name = "aether.library.library_tool")]
pub struct LibraryTool {
    /// The publishes called and asked of the library, each held until the library answers, by number.
    calls: BTreeMap<u64, Held<Called>>,
    /// The number of the next call.
    next: u64,
}

/// What every turn of the tool may send: an ask of the library, and an answer to its caller.
type Sends = (Asks<Path<Library>, PublishModule, Calling>, ToSender<Called>);

impl ChildOf for LibraryTool {
    type Parent = Anywhere;
}

impl Actor for LibraryTool {
    type Config = LibraryToolConfig;
    type Key = ();
    type Handles = (Call, Answer<ModulePublished, Calling>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: LibraryToolConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { calls: BTreeMap::new(), next: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Tool for LibraryTool {
    fn sheets() -> Vec<ToolSheet> {
        vec![
            ToolSheet::of::<Publish>(
                PUBLISH,
                "Publishes a guest module of `Code` as the head of every kind it makes, moving the heads of kinds \
                 another module made before, and answers at once with the module's hash and each kind with the number \
                 of its publish. Each running actor of those kinds is asked before its next batch whether to move onto \
                 it: `upgrades` shows who has.",
                Output::Value(SchemaNodes::of(&PublishRecord::SHAPE)),
            ),
            ToolSheet::json::<ReadUpgrades>(
                UPGRADES,
                "Shows every kind's head, with the number of the publish that made it, how many actors have moved \
                 onto it, and the fields their old states lacked; and every running guest that is not on its kind's \
                 head: still to be asked, behind with how many batches it has answered `Later`, or parked with the \
                 publish its move failed on and why. A parked guest runs its old code until its kind's head moves \
                 again.",
                Output::Value(SchemaNodes::of(&Upgrading::SHAPE)),
            ),
        ]
    }

    fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling> {
        me.protocol()
    }
}

/// `publish` is held and asked of the library; `upgrades` is answered at once.
impl Handles<Call> for LibraryTool {
    type Sends = Sends;

    fn handle(&mut self, call: Call, turn: &mut Turn<'_, Self, Sends>) -> Result<(), Fault> {
        let input = turn.read(&call.input);
        let mut budget = Budget::new(INPUT_BYTES);
        match call.tool.as_str() {
            PUBLISH => {
                let publish = match from_bytes::<Publish>(input, &mut budget) {
                    Ok(publish) => publish,
                    Err(fault) => {
                        turn.reply(&refused(&format!("the input of `{PUBLISH}`: {fault}")))?;
                        return Ok(());
                    }
                };
                let call = self.next;
                self.next += 1;
                turn.to(path::<Library>(())).ask(&PublishModule { path: publish.path }, &Calling { call })?;
                self.calls.insert(call, turn.hold()?);
            }
            UPGRADES => {
                let Moves::Read(standings) = turn.moves() else {
                    turn.reply(&refused(&format!("`{UPGRADES}` runs where no engine's heads are read")))?;
                    return Ok(());
                };
                let called = Called::output(turn, to_bytes(&Upgrading::of(&standings))?);
                turn.reply(&called)?;
            }
            _ => turn.reply(&refused("this worker serves publish and upgrades alone"))?,
        }

        Ok(())
    }
}

/// The library published the module, or did not: the publish is answered either way.
impl Handles<Answer<ModulePublished, Calling>> for LibraryTool {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, Calling { call }): (Answered<ModulePublished>, Calling),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Some(held) = self.calls.remove(&call) else {
            return Ok(());
        };
        let (module, kinds) = match answered {
            Answered::Yes(ModulePublished::Published { module, kinds }) => (module, kinds),
            Answered::Yes(ModulePublished::Unloaded(unloaded)) => {
                turn.answer(held, &refused(&unloaded_why(&unloaded)))?;
                return Ok(());
            }
            Answered::Yes(ModulePublished::Unopened) => {
                turn.answer(held, &refused("the library is still publishing `Code` as the program starts"))?;
                return Ok(());
            }
            Answered::No(why) => {
                turn.answer(held, &refused(&format!("the library did not answer: {why:?}")))?;
                return Ok(());
            }
        };
        let Ok(kinds) = from_bytes::<Vec<Numbered>>(turn.read(&kinds), &mut Budget::new(KINDS_BYTES)) else {
            turn.answer(held, &refused("the library's list of the kinds it published does not read"))?;
            return Ok(());
        };
        let blob = hex(&turn.hash(&module).bytes());
        let called = Called::output(turn, to_bytes(&PublishRecord { blob, kinds })?);
        turn.answer(held, &called)?;

        Ok(())
    }
}

/// Why a module was not published, for the agent to read.
fn unloaded_why(unloaded: &Unloaded) -> String {
    match unloaded {
        Unloaded::Missing => String::from("no module is at that path in `Code`"),
        Unloaded::Unavailable => String::from("this host has no `Code`"),
        Unloaded::Unread => String::from("the module could not be read"),
        Unloaded::Unfit => String::from("the module is not one this engine runs; the library's log says why"),
        Unloaded::MakesNothing => String::from("the module makes no actor type"),
    }
}

/// `bytes` in lower-case hexadecimal.
fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }

    text
}

/// A call refused, for the agent to read.
fn refused(why: &str) -> Called {
    Called::Refused(String::from(why))
}
