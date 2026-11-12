//! `send_input` (ADR-0046): input performed on a window for an agent, as a person at it would.

#[cfg(test)]
mod tests;

use crate::perform::{Input, MOST_INPUTS, Perform, Performed, Unperformed, unperformable};
use crate::window::{Window, WindowId};
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anywhere, Asks, Beginning, ChildOf, Ending, Fault, Handles, Held, Path,
    ProtocolRef, ToSender, Turn, Unanswered, path,
};
use aethereal_data::{Bound, Budget, Plain, Schema, SchemaNodes, from_bytes, to_bytes};
use aethereal_tools::{Call, Called, Output, Tool, ToolSheet, Tooling};
use std::collections::BTreeMap;

/// The most bytes reading an input may take: the most inputs, each given room for a key's names or the most text.
const INPUT_BYTES: usize = MOST_INPUTS as usize * 512;

/// What the agent asks `send_input` for: `inputs` performed on `window`, in order.
#[derive(Debug, Clone, PartialEq, Plain)]
#[kind(name = "aether.window.send_input")]
pub struct SendInput {
    /// The window, by its number.
    #[flag(position)]
    pub window: WindowId,
    /// What is performed, in order: `text:<text>`, or a JSON array of inputs.
    #[flag(position)]
    pub inputs: Vec<Input>,
}

/// What the tool keeps for its ask of a window's actor to perform one input: which call it answers, and which input
/// of the call's it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.window.sending")]
pub struct Sending {
    call: u64,
    at: u32,
}

/// What the tool is made from: nothing.
#[derive(Debug, Default, PartialEq, Eq, Plain)]
#[kind(name = "aether.window.input_tool_config")]
pub struct InputToolConfig;

/// The tool `send_input`: checks the inputs, then asks the actor of the window named to perform each, one a request, in
/// order, so each is read within an engine's default `read_limit`; and answers once the last is performed. A window's
/// actor handles one sender's requests in the order sent, and answers them in that order, so the last answer comes
/// after every other.
#[derive(Bound)]
#[kind(name = "aether.window.input_tool")]
pub struct InputTool {
    /// The calls asked of a window's actor and not yet answered, by number.
    calls: BTreeMap<u64, Sent>,
    /// The number of the next call.
    next: u64,
}

/// A call whose inputs were asked of a window's actor: answered once the input numbered `last` is performed, or once
/// any is refused.
#[derive(Bound)]
struct Sent {
    held: Held<Called>,
    last: u32,
}

/// What every turn of the tool may send: an ask of a window's actor, and an answer to its caller.
type Sends = (Asks<Path<Window>, Perform, Sending>, ToSender<Called>);

impl ChildOf for InputTool {
    type Parent = Anywhere;
}

impl Actor for InputTool {
    type Config = InputToolConfig;
    type Key = ();
    type Handles = (Call, Answer<Performed, Sending>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: InputToolConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { calls: BTreeMap::new(), next: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Tool for InputTool {
    fn sheets() -> Vec<ToolSheet> {
        vec![ToolSheet::of::<SendInput>(
            "send_input",
            "Performs `inputs` on a window, in order, as a person at it would. They are pointer moves, button \
             presses and releases, wheel turns, keys down and up, and text typed. A position is in the window's \
             surface pixels from its top left; a key is named as the W3C UI Events name it, `code` its place \
             (`KeyW`, `ShiftLeft`) and `key` what it means (`w`, `Shift`). The program cannot tell them from a \
             person's. A click is PointerMoved, then PointerDown and PointerUp there. A performed KeyDown sends no \
             Text with it: a key that types, as a person's does, is KeyDown, then Text of what it typed (`w`), then \
             KeyUp, and a text field reads the Text. They are sent whether or not the window has the keyboard, the \
             keys and Text to the actors that declare them, as a person's are. Every actor sent them has them before \
             anything after the answer, so a key pressed, then the clock advanced, is down for the advance's first \
             Tick; release it with KeyUp in a later call. At most 1024 inputs a call; a key's names at most 64 \
             bytes each; a Text at most 256 bytes, not empty, and with no control character, as Enter, Backspace \
             and Tab are keys. None is sent when any is refused.",
            Output::Value(SchemaNodes::of(&Performed::SHAPE)),
        )]
    }

    fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling> {
        me.protocol()
    }
}

/// Checks the inputs, holds the call, and asks the window's actor to perform each, in order: answered once the last
/// is. A call of no inputs is answered at once, as there is nothing to perform.
impl Handles<Call> for InputTool {
    type Sends = Sends;

    fn handle(&mut self, call: Call, turn: &mut Turn<'_, Self, Sends>) -> Result<(), Fault> {
        let Ok(asked) = from_bytes::<SendInput>(turn.read(&call.input), &mut Budget::new(INPUT_BYTES)) else {
            turn.reply(&Called::Refused(String::from("that is no input to send")))?;
            return Ok(());
        };
        let SendInput { window, inputs } = asked;
        let checked = unperformable(&inputs).map_or(Performed::Performed, Performed::Refused);
        let is_empty = inputs.is_empty();
        let is_refused = checked != Performed::Performed;
        if is_empty || is_refused {
            let called = Called::output(turn, to_bytes(&checked)?);
            turn.reply(&called)?;
            return Ok(());
        }
        let call = self.next;
        self.next += 1;
        let mut last = 0;
        for (input, at) in inputs.into_iter().zip(0..) {
            turn.to(path::<Window>(window)).ask(&Perform { inputs: vec![input] }, &Sending { call, at })?;
            last = at;
        }
        self.calls.insert(call, Sent { held: turn.hold()?, last });

        Ok(())
    }
}

/// The window's actor performed an input, refused it, or was not there: the call is answered once its last input is
/// performed, or with the first refusal, and the answers after that are dropped. A window whose actor had ended, or
/// never was, is not open.
impl Handles<Answer<Performed, Sending>> for InputTool {
    type Sends = Sends;

    fn handle(
        &mut self,
        (answered, Sending { call, at }): (Answered<Performed>, Sending),
        turn: &mut Turn<'_, Self, Sends>,
    ) -> Result<(), Fault> {
        let Some(sent) = self.calls.get(&call) else {
            return Ok(());
        };
        let called = match answered {
            Answered::Yes(Performed::Performed) if at < sent.last => return Ok(()),
            Answered::Yes(Performed::Performed) => Called::output(turn, to_bytes(&Performed::Performed)?),
            Answered::Yes(Performed::Refused(why)) => {
                Called::output(turn, to_bytes(&Performed::Refused(counted(why, at)))?)
            }
            Answered::No(Unanswered::Ended) => {
                Called::output(turn, to_bytes(&Performed::Refused(Unperformed::NotOpen))?)
            }
            Answered::No(why) => Called::Refused(format!("input {at} was not performed: {why:?}")),
        };
        let Some(sent) = self.calls.remove(&call) else {
            return Ok(());
        };
        turn.answer(sent.held, &called)?;

        Ok(())
    }
}

/// `why`, refused of the one input asked, with the input it names counted within the call, as input `at`.
const fn counted(why: Unperformed, at: u32) -> Unperformed {
    match why {
        Unperformed::NotFinite { .. } => Unperformed::NotFinite { at },
        Unperformed::NameTooLong { most, .. } => Unperformed::NameTooLong { at, most },
        Unperformed::TextTooLong { most, .. } => Unperformed::TextTooLong { at, most },
        Unperformed::NotText { .. } => Unperformed::NotText { at },
        Unperformed::NotOpen | Unperformed::TooMany { .. } => why,
    }
}
