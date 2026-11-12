//! How the overlay's actor runs the console (ADR-0053: Commands are tools; ADR-0055): each key at the console's stop
//! edits or enters the line, a command that names a tool is asked of the engine's tools root and its answer printed,
//! the tools root's sheets are kept for the next, and marks pinned through the overlay's own tool are drawn and
//! printed.

use super::Overlay;
use super::drawing::Heard;
use super::texts::TextRole;
use crate::console::{Command, Key};
use crate::tools::Marked;
use aethereal_actor::{Answer, Answered, Fault, Handles, Level, Path, Tells, Turn, path};
use aethereal_data::Plain;
use aethereal_render::Renderer;
use aethereal_render::text::{MoveText, WriteText};
use aethereal_tools::{CallTool, Called, Offered, Output, Sheets, Tools};

/// The key that closes the console back to the band, by what it means.
const CLOSE_KEY: &str = "Escape";

/// A tool was called from the console's line: its name, so the call stops showing as running once it is answered,
/// and what it answers with, so its answer is printed in its forms.
#[derive(Plain)]
#[kind(name = "aether.overlay.calling")]
pub struct Calling {
    tool: String,
    output: Output,
}

/// The overlay's own tools were offered to the engine's tools root.
#[derive(Plain)]
#[kind(name = "aether.overlay.offering")]
pub struct Offering;

impl Overlay {
    /// Takes in a key pressed at the console's stop: Escape closes it; any other edits the line, steps its history,
    /// scrolls, completes, or enters it, a command that names a tool asked of the engine's tools root and shown as
    /// running until it is answered.
    pub(super) fn pressed(&mut self, key: &str, turn: &mut Turn<'_, Self, Heard>) -> Result<(), Fault> {
        if key == CLOSE_KEY {
            return self.close(turn);
        }
        let view = self.view();
        let command = self.console.pressed(Key::of(key), view);
        if let Command::Call { tool, revision, input, output } = command {
            match turn.put(input) {
                Ok(input) => {
                    let calling = Calling { tool: tool.clone(), output };
                    turn.to(path::<Tools>(())).ask(&CallTool { tool, revision, input }, &calling)?;
                    self.console.called(&calling.tool);
                    self.write_and_move(TextRole::Running, turn)?;
                }
                Err(refused) => self.console.refused(&format!("the input was not put: {refused:?}"), view),
            }
        }

        self.write_console(turn)
    }
}

/// What the tools root holds, kept for the console's next line: sent as the overlay subscribes, and again after each
/// change.
impl Handles<Sheets> for Overlay {
    type Sends = (Tells<Path<Renderer>, WriteText>,);

    fn handle(&mut self, sheets: Sheets, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.console.hear(sheets);

        self.write_text(TextRole::Prompt, turn)
    }
}

/// A tool's answer to the console's line, printed: a value in the forms its flags read, an image as one line, and a
/// refusal in brick. The call stops showing as running, whichever came.
impl Handles<Answer<Called, Calling>> for Overlay {
    type Sends = (Tells<Path<Renderer>, WriteText>, Tells<Path<Renderer>, MoveText>);

    fn handle(
        &mut self,
        (answered, Calling { tool, output }): (Answered<Called>, Calling),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        self.console.settled(&tool);
        self.write_and_move(TextRole::Running, turn)?;
        let view = self.view();
        match answered {
            Answered::Yes(Called::Output(bytes)) => self.console.answered(&output, turn.read(&bytes), view),
            Answered::Yes(Called::Image { png, text }) => self.console.answered_image(turn.read(&png), &text, view),
            Answered::Yes(Called::Refused(why)) => self.console.refused(&why, view),
            Answered::No(why) => self.console.refused(&format!("the tools root did not answer: {why:?}"), view),
        }

        self.write_console(turn)
    }
}

/// An offer of the overlay's own tools that the tools root refused is logged: the console works on without them.
impl Handles<Answer<Offered, Offering>> for Overlay {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, Offering): (Answered<Offered>, Offering),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let held = answered == Answered::Yes(Offered::Yes);
        if !held {
            turn.log(Level::Error, format_args!("the overlay's tools were not held: {answered:?}"));
        }

        Ok(())
    }
}

/// A mark, kept to be pinned at the next `Tick`'s time: its tick is drawn on the chart from then, and it is printed
/// in ochre then.
impl Handles<Marked> for Overlay {
    type Sends = ();

    fn handle(&mut self, marked: Marked, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.marks.wait(marked.text);

        Ok(())
    }
}
