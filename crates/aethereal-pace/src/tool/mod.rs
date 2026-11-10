//! The clock's tools, `set_pace` and `advance` (ADR-0046): the pacer asked for an agent, so it spaces what it does by
//! exact cycles.

#[cfg(test)]
mod tests;

use crate::pacer::{Advance, Advanced, PaceSet, Pacer, SetPace};
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anywhere, Asks, Beginning, ChildOf, Ending, Fault, Handles, Held, Path,
    ProtocolRef, ToSender, Turn, path,
};
use aethereal_data::{Bound, Budget, Plain, Schema, SchemaNodes, from_bytes, to_bytes};
use aethereal_tools::{Call, Called, Output, Tool, ToolSheet, Tooling};
use std::collections::BTreeMap;

/// The most bytes reading an input may take: a pace, or two counts.
const INPUT_BYTES: usize = 64;

/// What the tool keeps for its ask of the pacer: which call it answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.pace.calling")]
pub struct Calling {
    call: u64,
}

/// What the tool is made from: nothing.
#[derive(Debug, Default, PartialEq, Eq, Plain)]
#[kind(name = "aether.pace.pace_tool_config")]
pub struct PaceToolConfig;

/// The tools `set_pace` and `advance`: each asks the [`Pacer`] what it was called with, and answers what the pacer
/// answered. It keeps no state of the clock: a clock it held stays held until it is set running.
#[derive(Bound)]
#[kind(name = "aether.pace.pace_tool")]
pub struct PaceTool {
    /// The calls asked of the pacer and not yet answered, by number.
    calls: BTreeMap<u64, Held<Called>>,
    /// The number of the next call.
    next: u64,
}

/// What every turn of the tool may send: an ask of the pacer, and an answer to its caller.
type Sends = (Asks<Path<Pacer>, SetPace, Calling>, Asks<Path<Pacer>, Advance, Calling>, ToSender<Called>);

impl ChildOf for PaceTool {
    type Parent = Anywhere;
}

impl Actor for PaceTool {
    type Config = PaceToolConfig;
    type Key = ();
    type Handles = (Call, Answer<PaceSet, Calling>, Answer<Advanced, Calling>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: PaceToolConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { calls: BTreeMap::new(), next: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Tool for PaceTool {
    fn sheets() -> Vec<ToolSheet> {
        vec![
            ToolSheet::of::<SetPace>(
                "set_pace",
                "Sets how the program's clock moves: `Held`, so no cycle runs but those `advance` asks for, or \
                 `Real`, with the chassis's own time again. It answers how it moved before. A clock set running \
                 refuses the advances it held. The program must list the pacer.",
                Output::Value(SchemaNodes::of(&PaceSet::SHAPE)),
            ),
            ToolSheet::of::<Advance>(
                "advance",
                "Runs `cycles` cycles of a held clock, one after another, each Tick carrying exactly \
                 `delta_nanos`, and answers once the last has finished, with the program's elapsed time then. \
                 Refused while the clock runs, for no cycles or no time, and for a delta past the most the \
                 engine gives one Tick. Hold the clock with set_pace first.",
                Output::Value(SchemaNodes::of(&Advanced::SHAPE)),
            ),
        ]
    }

    fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling> {
        me.protocol()
    }
}

/// Holds the call, and asks the pacer what it was called with: answered once the pacer has answered.
impl Handles<Call> for PaceTool {
    type Sends = Sends;

    fn handle(&mut self, call: Call, turn: &mut Turn<'_, Self, Sends>) -> Result<(), Fault> {
        let calling = Calling { call: self.next };
        let input = turn.read(&call.input);
        let mut budget = Budget::new(INPUT_BYTES);
        match call.tool.as_str() {
            "set_pace" => {
                let Ok(set) = from_bytes::<SetPace>(input, &mut budget) else {
                    turn.reply(&refused("that is no pace to set"))?;
                    return Ok(());
                };
                turn.to(path::<Pacer>(())).ask(&set, &calling)?;
            }
            "advance" => {
                let Ok(advance) = from_bytes::<Advance>(input, &mut budget) else {
                    turn.reply(&refused("that is no advance"))?;
                    return Ok(());
                };
                turn.to(path::<Pacer>(())).ask(&advance, &calling)?;
            }
            _ => {
                turn.reply(&refused("this worker serves set_pace and advance alone"))?;
                return Ok(());
            }
        }
        self.next += 1;
        self.calls.insert(calling.call, turn.hold()?);

        Ok(())
    }
}

/// The pacer set the pace, or no pacer answered.
impl Handles<Answer<PaceSet, Calling>> for PaceTool {
    type Sends = Sends;

    fn handle(
        &mut self,
        (answered, calling): (Answered<PaceSet>, Calling),
        turn: &mut Turn<'_, Self, Sends>,
    ) -> Result<(), Fault> {
        self.answer(calling, answered, turn)
    }
}

/// The pacer ran the cycles, or refused them, or no pacer answered.
impl Handles<Answer<Advanced, Calling>> for PaceTool {
    type Sends = Sends;

    fn handle(
        &mut self,
        (answered, calling): (Answered<Advanced>, Calling),
        turn: &mut Turn<'_, Self, Sends>,
    ) -> Result<(), Fault> {
        self.answer(calling, answered, turn)
    }
}

impl PaceTool {
    /// Answers the call `calling` with what the pacer answered, or that none did.
    fn answer<A: Plain>(
        &mut self,
        Calling { call }: Calling,
        answered: Answered<A>,
        turn: &mut Turn<'_, Self, Sends>,
    ) -> Result<(), Fault> {
        let Some(held) = self.calls.remove(&call) else {
            return Ok(());
        };
        let called = match answered {
            Answered::Yes(answer) => Called::output(turn, to_bytes(&answer)?),
            Answered::No(why) => Called::Refused(format!("the pacer did not answer: {why:?}")),
        };
        turn.answer(held, &called)?;

        Ok(())
    }
}

/// A call refused, for the agent to read.
fn refused(why: &str) -> Called {
    Called::Refused(String::from(why))
}
