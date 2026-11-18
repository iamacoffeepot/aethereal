//! How the overlay's actor runs the console's tools (ADR-0053: Commands are tools): a command that names a tool is asked
//! of the engine's tools root and its answer printed, the tools root's sheets are kept for the next line, and an offer
//! of the overlay's own tools refused is logged.

use super::Overlay;
use super::chat::Reports;
use crate::console::Command;
use aethereal_actor::{Answered, Fault, Level, Turn, path};
use aethereal_data::Plain;
use aethereal_tools::{CallTool, Called, Offered, Output, Sheets, Tools};

/// A tool was called from the console's line: its name, so the call stops showing as running once it is answered,
/// what it answers with, so its answer is printed in its forms, and where what it printed is reported.
#[derive(Plain)]
#[kind(name = "aether.overlay.calling")]
pub struct Calling {
    tool: String,
    output: Output,
    reports: Reports,
}

/// The overlay's own tools, or the chat's words, were offered to the engine's tools root.
#[derive(Plain)]
#[kind(name = "aether.overlay.offering")]
pub struct Offering;

impl Overlay {
    /// Asks the engine's tools root for `command` when it names a tool, shown as running until it is answered, and
    /// what it prints reported as `reports` says once it is; any other command was printed as the console took it, and
    /// is reported at once.
    pub(super) fn call(&mut self, command: Command, reports: Reports, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let (tool, revision, input, output) = match command {
            Command::Call { tool, revision, input, output } => (tool, revision, input, output),
            Command::Blank => return Ok(()),
            Command::Shown(text) | Command::Refused(text) => return self.report(turn, reports, &text),
            Command::Clear => return self.report(turn, reports, ""),
        };
        match turn.put(input) {
            Ok(input) => {
                self.console.called(&tool);
                let calling = Calling { tool: tool.clone(), output, reports };
                turn.to(path::<Tools>(())).ask(&CallTool { tool, revision, input }, &calling)?;
            }
            Err(refused) => {
                let why = format!("the input was not put: {refused:?}");
                self.console.refused(&why);
                self.report(turn, reports, &why)?;
            }
        }

        Ok(())
    }
}

impl Overlay {
    /// What the tools root holds, kept for the console's next line: sent as the overlay subscribes, and again after
    /// each change.
    #[allow(clippy::unnecessary_wraps, reason = "its handler's body, which answers a fault as every handler does")]
    pub(super) fn handle_sheets(&mut self, sheets: Sheets) -> Result<(), Fault> {
        self.console.hear(sheets);

        Ok(())
    }

    /// A tool's answer to the console's line, printed: a value in the forms its flags read, an image as one line, and a
    /// refusal in the error's ink, and reported to the session whose tab was open as it was typed. The call stops
    /// showing as running, whichever came.
    pub(super) fn handle_called(
        &mut self,
        (answered, Calling { tool, output, reports }): (Answered<Called>, Calling),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        self.console.settled(&tool);
        let printed = match answered {
            Answered::Yes(Called::Output(bytes)) => self.console.answered(&output, turn.read(&bytes)),
            Answered::Yes(Called::Image { png, text }) => self.console.answered_image(turn.read(&png), &text),
            Answered::Yes(Called::Refused(why)) => {
                self.console.refused(&why);
                why
            }
            Answered::No(why) => {
                let why = format!("the tools root did not answer: {why:?}");
                self.console.refused(&why);
                why
            }
        };

        self.report(turn, reports, &printed)
    }

    /// An offer of the overlay's own tools, or the chat's words, that the tools root refused is logged: the console
    /// works on without them.
    #[allow(clippy::unnecessary_wraps, reason = "its handler's body, which answers a fault as every handler does")]
    #[allow(clippy::unused_self, reason = "its handler's body, which takes the actor as every handler does")]
    pub(super) fn handle_offered(
        &mut self,
        (answered, Offering): (Answered<Offered>, Offering),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let held = answered == Answered::Yes(Offered::Yes);
        if !held {
            turn.log(Level::Error, format_args!("tools the overlay offered were not held: {answered:?}"));
        }

        Ok(())
    }
}
