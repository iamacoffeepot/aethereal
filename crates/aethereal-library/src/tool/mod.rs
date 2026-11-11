//! The library's tools, `publish` and `upgrades` (ADR-0037, ADR-0054): a module of `Code` published for an agent, with
//! the record of who moved onto it, and the guests still behind their kinds' heads.

mod record;
#[cfg(test)]
mod tests;

pub use record::{
    BehindGuest, GuestStanding, HeadRecord, KindMoves, KindRecord, ParkedGuest, PublishRecord, Upgrading, WaitingGuest,
};

use crate::{Library, ModulePublished, Numbered, PublishModule, Unloaded};
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anywhere, Asks, Beginning, ChildOf, Ending, Fault, Handles, Held, Moves, Path,
    ProtocolRef, Sleep, Slept, Time, ToSender, Turn, path,
};
use aethereal_data::{Bound, Budget, FilePath, Plain, Schema, SchemaNodes, from_bytes, to_bytes};
use aethereal_tools::{Call, Called, Output, Tool, ToolSheet, Tooling};
use core::fmt::Write as _;
use core::time::Duration;
use std::collections::BTreeMap;

/// The tool that publishes.
const PUBLISH: &str = "publish";
/// The tool that lists the guests behind their kinds' heads.
const UPGRADES: &str = "upgrades";
/// The most bytes reading an input may take: a path, and a count.
const INPUT_BYTES: usize = 8 * 1024;
/// The most bytes reading the kinds a module makes may take: a name each, and its publish's number.
const KINDS_BYTES: usize = 1 << 20;
/// How often a publish that waits looks again at who has moved, in milliseconds.
const LOOK_EVERY_MILLIS: u64 = 10;

/// What `publish` is called with.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.library.publish")]
pub struct Publish {
    /// The module's path in `Code`.
    #[flag(position)]
    pub path: FilePath,
    /// How long to wait, in milliseconds, for every running actor of the kinds it makes to have moved, be parked, or
    /// have answered `Later`: none to answer as soon as the heads have moved.
    #[flag]
    pub wait_millis: Option<u64>,
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

/// What the tool keeps for a look it waits on: which call it answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.library.looking")]
pub struct Looking {
    call: u64,
}

/// What the tool is made from: nothing.
#[derive(Debug, Default, PartialEq, Eq, Plain)]
#[kind(name = "aether.library.library_tool_config")]
pub struct LibraryToolConfig;

/// The tools `publish` and `upgrades`. `publish` asks the [`Library`] to publish a module of `Code`, then looks at
/// what the engine records of the moves onto the heads it made until every running actor of their kinds has moved,
/// is parked, or has answered `Later`, or its wait ends, and answers the publish's record. `upgrades` answers every
/// running guest the engine found behind its kind's head.
#[derive(Bound)]
#[kind(name = "aether.library.library_tool")]
pub struct LibraryTool {
    /// The publishes called and not yet answered, by number.
    calls: BTreeMap<u64, Publishing>,
    /// The number of the next call.
    next: u64,
}

/// A publish called, and where it stands.
#[derive(Bound)]
enum Publishing {
    /// Asked of the library, waiting `wait_millis` once it is published.
    Asked { held: Held<Called>, wait_millis: u64 },
    /// Published: the module's hash and each kind with its publish's number, looked at until `until`.
    Watching { held: Held<Called>, blob: String, kinds: Vec<Numbered>, until: Time },
}

/// What every turn of the tool may send: an ask of the library, and an answer to its caller.
type Sends = (Asks<Path<Library>, PublishModule, Calling>, ToSender<Called>);

impl ChildOf for LibraryTool {
    type Parent = Anywhere;
}

impl Actor for LibraryTool {
    type Config = LibraryToolConfig;
    type Key = ();
    type Handles = (Call, Answer<ModulePublished, Calling>, Answer<Slept, Looking>);
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
                 another module made before. Each running actor of those kinds is asked before its next batch whether \
                 to move onto it. With `wait_millis`, it answers once every one has moved, is parked, or has answered \
                 `Later`, or when the wait ends. Per kind, it answers how many moved and the fields their old states \
                 lacked, who answered `Later` and before how many batches, who is parked and why, and who was still \
                 to be asked.",
                Output::Value(SchemaNodes::of(&PublishRecord::SHAPE)),
            ),
            ToolSheet::json::<ReadUpgrades>(
                UPGRADES,
                "Lists every running guest that is not on its kind's head: still to be asked, behind with how many \
                 batches it has answered `Later`, or parked with the publish its move failed on and why. A parked \
                 guest runs its old code until its kind's head moves again.",
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
                let wait_millis = publish.wait_millis.unwrap_or(0);
                self.calls.insert(call, Publishing::Asked { held: turn.hold()?, wait_millis });
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

/// The library published the module, or did not: once it has, the tool looks at who has moved, from the next step.
impl Handles<Answer<ModulePublished, Calling>> for LibraryTool {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, Calling { call }): (Answered<ModulePublished>, Calling),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Some(Publishing::Asked { held, wait_millis }) = self.calls.remove(&call) else {
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
        let now = turn.now();
        let until = now + Duration::from_millis(wait_millis);
        // The heads move as the step's effects are folded, so the first look is in a step after this one.
        turn.run(&Sleep { until: now }, &Looking { call })?;
        self.calls.insert(call, Publishing::Watching { held, blob, kinds, until });

        Ok(())
    }
}

/// A look at who has moved: the publish answered once every kind it made is settled or its wait has ended, and looked
/// at again a moment later otherwise.
impl Handles<Answer<Slept, Looking>> for LibraryTool {
    type Sends = ();

    fn handle(
        &mut self,
        (_, Looking { call }): (Answered<Slept>, Looking),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Moves::Read(standings) = turn.moves() else {
            let Some(Publishing::Watching { held, .. }) = self.calls.remove(&call) else {
                return Ok(());
            };
            turn.answer(held, &refused(&format!("`{PUBLISH}` runs where no engine's heads are read")))?;
            return Ok(());
        };
        let Some(Publishing::Watching { kinds, until, .. }) = self.calls.get(&call) else {
            return Ok(());
        };
        let records: Option<Vec<KindRecord>> =
            kinds.iter().map(|numbered| KindRecord::of(numbered, &standings)).collect();
        let (now, until) = (turn.now(), *until);
        // A publish the engine has not folded yet has no record: it folds in a step to come.
        let is_folded = records.is_some();
        let is_settled = records.as_deref().is_some_and(|records| records.iter().all(KindRecord::is_settled));
        let has_waited = now >= until;
        let is_done = is_settled || has_waited;
        let looks_again = !is_folded || !is_done;
        if looks_again {
            let next = (now + Duration::from_millis(LOOK_EVERY_MILLIS)).min(until);
            turn.run(&Sleep { until: next }, &Looking { call })?;
            return Ok(());
        }
        let (Some(Publishing::Watching { held, blob, .. }), Some(kinds)) = (self.calls.remove(&call), records) else {
            return Ok(());
        };
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
