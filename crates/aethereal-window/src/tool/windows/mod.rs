//! `list_windows`, `resize_window`, `move_window`, and `set_window_mode` (ADR-0047): the window root asked for an
//! agent, so it names windows it has listed, and lays them out where the platform lets a program.

#[cfg(test)]
mod tests;

use crate::windows::{ListWindows, ModeSet, MoveWindow, Moved, ResizeWindow, Resized, SetMode, WindowList, Windows};
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anywhere, Asks, Beginning, ChildOf, Ending, Fault, Handles, Held, Path,
    ProtocolRef, ToSender, Turn, path,
};
use aethereal_data::{Bound, Budget, Plain, Schema, SchemaNodes, from_bytes, to_bytes};
use aethereal_tools::{Call, Called, Output, Tool, ToolSheet, Tooling};
use std::collections::BTreeMap;

/// The most bytes reading an input may take: a window, and a size, a place, or a mode.
const INPUT_BYTES: usize = 64;

/// What the tool keeps for its ask of the window root: which call it answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.window.placing")]
pub struct Placing {
    call: u64,
}

/// What the tool is made from: nothing.
#[derive(Debug, Default, PartialEq, Eq, Plain)]
#[kind(name = "aether.window.window_tool_config")]
pub struct WindowToolConfig;

/// The tools `list_windows`, `resize_window`, `move_window`, and `set_window_mode`: each asks the [`Windows`] root
/// what it was called with, and answers what the root answered.
#[derive(Bound)]
#[kind(name = "aether.window.window_tool")]
pub struct WindowTool {
    /// The calls asked of the root and not yet answered, by number.
    calls: BTreeMap<u64, Held<Called>>,
    /// The number of the next call.
    next: u64,
}

/// What every turn of the tool may send: an ask of the window root, and an answer to its caller.
type Sends = (
    Asks<Path<Windows>, ListWindows, Placing>,
    Asks<Path<Windows>, ResizeWindow, Placing>,
    Asks<Path<Windows>, MoveWindow, Placing>,
    Asks<Path<Windows>, SetMode, Placing>,
    ToSender<Called>,
);

impl ChildOf for WindowTool {
    type Parent = Anywhere;
}

impl Actor for WindowTool {
    type Config = WindowToolConfig;
    type Key = ();
    type Handles =
        (Call, Answer<WindowList, Placing>, Answer<Resized, Placing>, Answer<Moved, Placing>, Answer<ModeSet, Placing>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: WindowToolConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { calls: BTreeMap::new(), next: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Tool for WindowTool {
    fn sheets() -> Vec<ToolSheet> {
        vec![
            ToolSheet {
                name: String::from("list_windows"),
                description: String::from(
                    "Lists every open window, in the order of their ids: each one's id, which names it to \
                     capture_frame, record_clip, send_input, and the tools here; its title; its surface, in its \
                     pixels, and the screen's scale; the mode it is shown in; and whether it has the keyboard. A \
                     window still opening is not listed.",
                ),
                input: SchemaNodes::of(&ListWindows::SHAPE),
                output: Output::Value(SchemaNodes::of(&WindowList::SHAPE)),
            },
            ToolSheet {
                name: String::from("resize_window"),
                description: String::from(
                    "Resizes a window shown windowed to `size` in logical pixels, and answers its surface as it is \
                     drawn now; a windowing system that resizes later answers the size before. Refused for a window \
                     maximized or fullscreen: set it windowed first.",
                ),
                input: SchemaNodes::of(&ResizeWindow::SHAPE),
                output: Output::Value(SchemaNodes::of(&Resized::SHAPE)),
            },
            ToolSheet {
                name: String::from("move_window"),
                description: String::from(
                    "Moves a window shown windowed so its frame's top left is `at`, in logical pixels from the \
                     screen's top left. Refused on Wayland, whose compositor alone places windows, and in a browser.",
                ),
                input: SchemaNodes::of(&MoveWindow::SHAPE),
                output: Output::Value(SchemaNodes::of(&Moved::SHAPE)),
            },
            ToolSheet {
                name: String::from("set_window_mode"),
                description: String::from(
                    "Shows a window `Windowed`, `Maximized`, or `Fullscreen`, and answers its surface as it is drawn \
                     now. Refused in a browser.",
                ),
                input: SchemaNodes::of(&SetMode::SHAPE),
                output: Output::Value(SchemaNodes::of(&ModeSet::SHAPE)),
            },
        ]
    }

    fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling> {
        me.protocol()
    }
}

/// Holds the call, and asks the window root what it was called with: answered once the root has answered.
impl Handles<Call> for WindowTool {
    type Sends = Sends;

    fn handle(&mut self, call: Call, turn: &mut Turn<'_, Self, Sends>) -> Result<(), Fault> {
        let placing = Placing { call: self.next };
        let input = turn.read(&call.input);
        let mut budget = Budget::new(INPUT_BYTES);
        match call.tool.as_str() {
            "list_windows" => {
                let Ok(list) = from_bytes::<ListWindows>(input, &mut budget) else {
                    return untaken(turn);
                };
                turn.to(path::<Windows>(())).ask(&list, &placing)?;
            }
            "resize_window" => {
                let Ok(resize) = from_bytes::<ResizeWindow>(input, &mut budget) else {
                    return untaken(turn);
                };
                turn.to(path::<Windows>(())).ask(&resize, &placing)?;
            }
            "move_window" => {
                let Ok(moving) = from_bytes::<MoveWindow>(input, &mut budget) else {
                    return untaken(turn);
                };
                turn.to(path::<Windows>(())).ask(&moving, &placing)?;
            }
            "set_window_mode" => {
                let Ok(set) = from_bytes::<SetMode>(input, &mut budget) else {
                    return untaken(turn);
                };
                turn.to(path::<Windows>(())).ask(&set, &placing)?;
            }
            _ => {
                let why = "this worker serves list_windows, resize_window, move_window, and set_window_mode";
                turn.reply(&Called::Refused(String::from(why)))?;
                return Ok(());
            }
        }
        self.next += 1;
        self.calls.insert(placing.call, turn.hold()?);

        Ok(())
    }
}

impl Handles<Answer<WindowList, Placing>> for WindowTool {
    type Sends = Sends;

    fn handle(
        &mut self,
        (answered, placing): (Answered<WindowList>, Placing),
        turn: &mut Turn<'_, Self, Sends>,
    ) -> Result<(), Fault> {
        self.answer(placing, answered, turn)
    }
}

impl Handles<Answer<Resized, Placing>> for WindowTool {
    type Sends = Sends;

    fn handle(
        &mut self,
        (answered, placing): (Answered<Resized>, Placing),
        turn: &mut Turn<'_, Self, Sends>,
    ) -> Result<(), Fault> {
        self.answer(placing, answered, turn)
    }
}

impl Handles<Answer<Moved, Placing>> for WindowTool {
    type Sends = Sends;

    fn handle(
        &mut self,
        (answered, placing): (Answered<Moved>, Placing),
        turn: &mut Turn<'_, Self, Sends>,
    ) -> Result<(), Fault> {
        self.answer(placing, answered, turn)
    }
}

impl Handles<Answer<ModeSet, Placing>> for WindowTool {
    type Sends = Sends;

    fn handle(
        &mut self,
        (answered, placing): (Answered<ModeSet>, Placing),
        turn: &mut Turn<'_, Self, Sends>,
    ) -> Result<(), Fault> {
        self.answer(placing, answered, turn)
    }
}

impl WindowTool {
    /// Answers the call `placing` with what the window root answered, or that it did not.
    fn answer<A: Plain>(
        &mut self,
        Placing { call }: Placing,
        answered: Answered<A>,
        turn: &mut Turn<'_, Self, Sends>,
    ) -> Result<(), Fault> {
        let Some(held) = self.calls.remove(&call) else {
            return Ok(());
        };
        let called = match answered {
            Answered::Yes(answer) => Called::output(turn, to_bytes(&answer)?),
            Answered::No(why) => Called::Refused(format!("the window root did not answer: {why:?}")),
        };
        turn.answer(held, &called)?;

        Ok(())
    }
}

/// Refuses a call whose input is not what its tool takes.
fn untaken(turn: &mut Turn<'_, WindowTool, Sends>) -> Result<(), Fault> {
    turn.reply(&Called::Refused(String::from("that is not what the tool takes")))?;

    Ok(())
}
