//! `list_windows`, `resize_window`, `move_window`, and `set_window_mode` (ADR-0047): the window root asked for an
//! agent, so it names windows it has listed, and lays them out where the platform lets a program.

#[cfg(test)]
mod tests;

use crate::windows::{ListWindows, ModeSet, MoveWindow, Moved, ResizeWindow, Resized, SetMode, WindowList, Windows};
use aethereal_actor::{ActorRef, Answered, Anywhere, Fault, Held, ProtocolRef, Reply, Turn, actor, path};
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

#[actor(parent = Anywhere, depends(Windows))]
impl WindowTool {
    fn begin(_: WindowToolConfig) -> Result<Self, Fault> {
        Ok(Self { calls: BTreeMap::new(), next: 0 })
    }

    /// Holds the call, and asks the window root what it was called with: answered once the root has answered.
    #[request]
    fn call(&mut self, call: Call, turn: &mut Turn<'_, Self>) -> Result<Reply<Called>, Fault> {
        let placing = Placing { call: self.next };
        let input = turn.read(&call.input);
        let mut budget = Budget::new(INPUT_BYTES);
        match call.tool.as_str() {
            "list_windows" => {
                let Ok(list) = from_bytes::<ListWindows>(input, &mut budget) else {
                    return Ok(untaken());
                };
                turn.to(path::<Windows>(())).ask(&list, &placing)?;
            }
            "resize_window" => {
                let Ok(resize) = from_bytes::<ResizeWindow>(input, &mut budget) else {
                    return Ok(untaken());
                };
                turn.to(path::<Windows>(())).ask(&resize, &placing)?;
            }
            "move_window" => {
                let Ok(moving) = from_bytes::<MoveWindow>(input, &mut budget) else {
                    return Ok(untaken());
                };
                turn.to(path::<Windows>(())).ask(&moving, &placing)?;
            }
            "set_window_mode" => {
                let Ok(set) = from_bytes::<SetMode>(input, &mut budget) else {
                    return Ok(untaken());
                };
                turn.to(path::<Windows>(())).ask(&set, &placing)?;
            }
            _ => {
                let why = "this worker serves list_windows, resize_window, move_window, and set_window_mode";
                return Ok(Reply::Now(Called::Refused(String::from(why))));
            }
        }
        self.next += 1;
        let (held, later) = turn.hold();
        self.calls.insert(placing.call, held);

        Ok(Reply::Later(later))
    }

    #[answer]
    fn window_list(
        &mut self,
        (answered, placing): (Answered<WindowList>, Placing),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        self.answer(placing, answered, turn)
    }

    #[answer]
    fn resized(
        &mut self,
        (answered, placing): (Answered<Resized>, Placing),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        self.answer(placing, answered, turn)
    }

    #[answer]
    fn moved(
        &mut self,
        (answered, placing): (Answered<Moved>, Placing),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        self.answer(placing, answered, turn)
    }

    #[answer]
    fn mode_set(
        &mut self,
        (answered, placing): (Answered<ModeSet>, Placing),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        self.answer(placing, answered, turn)
    }
}

impl Tool for WindowTool {
    fn sheets() -> Vec<ToolSheet> {
        vec![
            ToolSheet::json::<ListWindows>(
                "list_windows",
                "Lists every open window, in the order of their ids. Each is listed with its id, which names it to \
                 capture_frame, record_clip, send_input, and the tools here; its title; its surface, in its pixels, \
                 and the screen's scale; the mode it is shown in; and whether it has the keyboard. A window still \
                 opening is not listed.",
                Output::Value(SchemaNodes::of(&WindowList::SHAPE)),
            ),
            ToolSheet::of::<ResizeWindow>(
                "resize_window",
                "Resizes a window shown windowed to `size` in logical pixels. It answers its surface as it is drawn \
                 now; a windowing system that resizes later answers the size before. Refused for a window maximized \
                 or fullscreen: set it windowed first.",
                Output::Value(SchemaNodes::of(&Resized::SHAPE)),
            ),
            ToolSheet::of::<MoveWindow>(
                "move_window",
                "Moves a window shown windowed to `at`. `at` is where its frame's top left goes, in logical pixels \
                 from the screen's top left. Refused on Wayland, whose compositor alone places windows, and in a \
                 browser.",
                Output::Value(SchemaNodes::of(&Moved::SHAPE)),
            ),
            ToolSheet::of::<SetMode>(
                "set_window_mode",
                "Shows a window `Windowed`, `Maximized`, or `Fullscreen`, and answers its surface as it is drawn \
                 now. Refused in a browser.",
                Output::Value(SchemaNodes::of(&ModeSet::SHAPE)),
            ),
        ]
    }

    fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling> {
        me.protocol()
    }
}

impl WindowTool {
    /// Answers the call `placing` with what the window root answered, or that it did not.
    fn answer<A: Plain>(
        &mut self,
        Placing { call }: Placing,
        answered: Answered<A>,
        turn: &mut Turn<'_, Self>,
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

/// A call whose input is not what its tool takes, refused at once.
fn untaken() -> Reply<Called> {
    Reply::Now(Called::Refused(String::from("that is not what the tool takes")))
}
