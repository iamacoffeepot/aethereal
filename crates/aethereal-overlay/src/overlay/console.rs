//! How the overlay's actor runs the console's tools (ADR-0053: Commands are tools): a command that names a tool is asked
//! of the engine's tools root and its answer printed, the tools root's sheets are kept for the next line, the overlay's
//! own tools are offered, and marks pinned through them are kept to be stamped at the next `Tick`.

use super::Overlay;
use crate::console::Command;
use crate::tools::Marked;
use aethereal_actor::{Allows, Answer, Answered, Asks, Fault, Handles, Level, Path, Turn, path};
use aethereal_data::Plain;
use aethereal_tools::{CallTool, Called, Offered, Output, Sheets, Tools};

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
    /// Asks the engine's tools root for `command` when it names a tool, shown as running until it is answered; any
    /// other command was printed as the console took it.
    pub(super) fn call<S, I>(&mut self, command: Command, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<Asks<Path<Tools>, CallTool, Calling>, I>,
    {
        let Command::Call { tool, revision, input, output } = command else {
            return Ok(());
        };
        match turn.put(input) {
            Ok(input) => {
                self.console.called(&tool);
                let calling = Calling { tool: tool.clone(), output };
                turn.to(path::<Tools>(())).ask(&CallTool { tool, revision, input }, &calling)?;
            }
            Err(refused) => self.console.refused(&format!("the input was not put: {refused:?}"), self.columns()),
        }

        Ok(())
    }
}

/// What the tools root holds, kept for the console's next line: sent as the overlay subscribes, and again after each
/// change.
impl Handles<Sheets> for Overlay {
    type Sends = ();

    fn handle(&mut self, sheets: Sheets, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.console.hear(sheets);

        Ok(())
    }
}

/// A tool's answer to the console's line, printed: a value in the forms its flags read, an image as one line, and a
/// refusal in brick. The call stops showing as running, whichever came.
impl Handles<Answer<Called, Calling>> for Overlay {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, Calling { tool, output }): (Answered<Called>, Calling),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        self.console.settled(&tool);
        let columns = self.columns();
        match answered {
            Answered::Yes(Called::Output(bytes)) => self.console.answered(&output, turn.read(&bytes), columns),
            Answered::Yes(Called::Image { png, text }) => self.console.answered_image(turn.read(&png), &text, columns),
            Answered::Yes(Called::Refused(why)) => self.console.refused(&why, columns),
            Answered::No(why) => self.console.refused(&format!("the tools root did not answer: {why:?}"), columns),
        }

        Ok(())
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
