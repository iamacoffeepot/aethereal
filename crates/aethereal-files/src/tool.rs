//! The file capability's tools (ADR-0037, ADR-0039).

use crate::{FileRead, FileWritten, Files, Namespace, ReadFile, WriteFile};
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anywhere, Asks, Beginning, Bytes, ChildOf, Ending, Fault, Handles, Held, Path,
    ProtocolRef, ToSender, Turn, path,
};
use aethereal_data::{
    Bare, Bound, Budget, Encode, EncodeError, FilePath, Plain, Schema, SchemaNodes, Writer, from_bytes, to_bytes,
};
use aethereal_tools::{Call, Called, Done, Output, Tool, ToolSheet, Tooling};
use std::collections::BTreeMap;

/// The most bytes of a file the tool answers with: past it, the tool refuses. Its answer holds the text in a blob,
/// which no mail's `read_limit` weighs (ADR-0042).
const MOST_BYTES: usize = 256 * 1024;
/// The most bytes reading an input may take. A path is short.
const INPUT_BYTES: usize = 8 * 1024;
/// The most bytes of text the write tool writes, which bounds its input as a file's answer bounds the read tool's.
const MOST_WRITE_BYTES: usize = MOST_BYTES;
/// The most bytes reading the write tool's input may take: its text and its path.
const WRITE_INPUT_BYTES: usize = MOST_WRITE_BYTES + INPUT_BYTES;
/// The bytes of a text's count, written before it (ADR-0002).
const COUNT_BYTES: usize = 4;

/// What an agent reads of a file: its text. A `FileRead` holds a blob, which has no JSON form.
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.files.file_contents")]
pub struct FileContents {
    pub text: String,
}

/// A file of text at `path` in `namespace`, as the agent writes it: a `WriteFile`'s bytes have a hexadecimal JSON form,
/// and an agent writes text.
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.files.text_file")]
pub struct TextFile {
    /// The namespace the file is in, one that is written.
    #[flag(position)]
    pub namespace: Namespace,
    /// Where it is in the namespace.
    #[flag(position)]
    pub path: FilePath,
    /// The whole file.
    #[flag(position)]
    pub text: String,
}

/// What a tool keeps for a request it asked: which it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.files.tool_calling")]
pub struct Calling {
    number: u64,
}

/// What the tool is made from: nothing.
#[derive(Debug, Default, PartialEq, Eq, Plain)]
#[kind(name = "aether.files.read_tool_config")]
pub struct ReadToolConfig;

/// The tool `read_file`: asks [`Files`] the [`ReadFile`] its input is, and answers the file's text. A file that is not
/// UTF-8 is refused, as an agent reads text.
#[derive(Bound)]
#[kind(name = "aether.files.read_tool")]
pub struct ReadTool {
    /// The calls it holds, each by the number of the read it asked for it.
    calls: BTreeMap<u64, Held<Called>>,
    next: u64,
}

impl ChildOf for ReadTool {
    type Parent = Anywhere;
}

impl Actor for ReadTool {
    type Config = ReadToolConfig;
    type Key = ();
    type Handles = (Call, Answer<FileRead, Calling>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: ReadToolConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { calls: BTreeMap::new(), next: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Tool for ReadTool {
    fn sheets() -> Vec<ToolSheet> {
        vec![ToolSheet::of::<ReadFile>(
            "read_file",
            &format!("Reads a text file of the program's own, up to {MOST_BYTES} bytes."),
            Output::Value(SchemaNodes::of(&FileContents::SHAPE)),
        )]
    }

    fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling> {
        me.protocol()
    }
}

impl Handles<Call> for ReadTool {
    type Sends = (Asks<Path<Files>, ReadFile, Calling>, ToSender<Called>);

    fn handle(&mut self, call: Call, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Ok(read) = from_bytes::<ReadFile>(turn.read(&call.input), &mut Budget::new(INPUT_BYTES)) else {
            // The schema says a string of any spelling, and a path is narrower.
            turn.reply(&Called::Refused(String::from("that is no path")))?;
            return Ok(());
        };
        let held = turn.hold()?;
        let number = self.next;
        self.next += 1;
        turn.to(path::<Files>(())).ask(&read, &Calling { number })?;
        self.calls.insert(number, held);

        Ok(())
    }
}

impl Handles<Answer<FileRead, Calling>> for ReadTool {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, Calling { number }): (Answered<FileRead>, Calling),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Some(held) = self.calls.remove(&number) else {
            return Ok(());
        };
        let called = match answered {
            Answered::Yes(FileRead::Read { bytes }) => contents(turn, &bytes)?,
            Answered::Yes(FileRead::Missing) => refused("no file is at that path"),
            Answered::Yes(FileRead::Unavailable) => refused("this host has no files"),
            Answered::Yes(FileRead::Failed) => refused("the file could not be read"),
            Answered::No(_) => refused("`Files` did not answer"),
        };
        turn.answer(held, &called)?;

        Ok(())
    }
}

/// A refusal saying `why`.
fn refused(why: &str) -> Called {
    Called::Refused(String::from(why))
}

/// What the tool answers of a file whose bytes are in the blob `bytes`: its text, as [`FileContents`], built in a buffer
/// the turn hands out and put (ADR-0042).
fn contents<S>(turn: &mut Turn<'_, ReadTool, S>, bytes: &Bytes) -> Result<Called, Fault> {
    let length = turn.read(bytes).len();
    if length > MOST_BYTES {
        return Ok(refused("the file is too large"));
    }
    let buffer = turn.buffer(COUNT_BYTES + length);
    let Ok(text) = str::from_utf8(turn.read(bytes)) else {
        return Ok(refused("the file is not text"));
    };
    let written = written(text, buffer)?;

    Ok(Called::output(turn, written))
}

/// `text` written into `buffer` as the [`FileContents`] that holds it, with no `String` made of it: a struct is its
/// fields' bytes in order, and a `str` is written as the `String` it would be (ADR-0002).
fn written(text: &str, buffer: Vec<u8>) -> Result<Vec<u8>, EncodeError> {
    let mut out = Writer::<Bare>::appending(buffer, ());
    text.encode(&mut out)?;

    Ok(out.into_bytes())
}

/// What the write tool is made from: nothing.
#[derive(Debug, Default, PartialEq, Eq, Plain)]
#[kind(name = "aether.files.write_tool_config")]
pub struct WriteToolConfig;

/// The tool `write_file`: asks [`Files`] to write its input's text as the whole file at its path, and answers that it
/// did. A write to a namespace that is not written, as `Assets` is not, is refused, as is each write the host could
/// not do, with the reason.
#[derive(Bound)]
#[kind(name = "aether.files.write_tool")]
pub struct WriteTool {
    /// The calls it holds, each by the number of the write it asked for it.
    calls: BTreeMap<u64, Held<Called>>,
    next: u64,
}

impl ChildOf for WriteTool {
    type Parent = Anywhere;
}

impl Actor for WriteTool {
    type Config = WriteToolConfig;
    type Key = ();
    type Handles = (Call, Answer<FileWritten, Calling>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: WriteToolConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { calls: BTreeMap::new(), next: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Tool for WriteTool {
    fn sheets() -> Vec<ToolSheet> {
        vec![ToolSheet::of::<TextFile>(
            "write_file",
            &format!(
                "Writes a text file of the program's own, up to {MOST_WRITE_BYTES} bytes, whole, in a namespace that is \
                 written: `Save`."
            ),
            Output::Value(SchemaNodes::of(&Done::SHAPE)),
        )]
    }

    fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling> {
        me.protocol()
    }
}

impl Handles<Call> for WriteTool {
    type Sends = (Asks<Path<Files>, WriteFile, Calling>, ToSender<Called>);

    fn handle(&mut self, call: Call, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let input = turn.read(&call.input);
        if input.len() > WRITE_INPUT_BYTES {
            turn.reply(&refused("the text is too large"))?;
            return Ok(());
        }
        let text = match from_bytes::<TextFile>(input, &mut Budget::new(WRITE_INPUT_BYTES)) {
            Ok(text) => text,
            // The schema says a string of any spelling, and a path is narrower: the fault says which rule it broke.
            Err(fault) => {
                turn.reply(&refused(&format!("the input of `write_file`: {fault}")))?;
                return Ok(());
            }
        };
        let Ok(bytes) = turn.put(text.text.into_bytes()) else {
            turn.reply(&refused("the engine had no room to hold the text"))?;
            return Ok(());
        };
        let held = turn.hold()?;
        let number = self.next;
        self.next += 1;
        let write = WriteFile { namespace: text.namespace, path: text.path, bytes };
        turn.to(path::<Files>(())).ask(&write, &Calling { number })?;
        self.calls.insert(number, held);

        Ok(())
    }
}

impl Handles<Answer<FileWritten, Calling>> for WriteTool {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, Calling { number }): (Answered<FileWritten>, Calling),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Some(held) = self.calls.remove(&number) else {
            return Ok(());
        };
        let called = match answered {
            Answered::Yes(FileWritten::Written) => Called::output(turn, to_bytes(&Done {})?),
            Answered::Yes(FileWritten::ReadOnly) => refused("that namespace is read-only"),
            Answered::Yes(FileWritten::Unavailable) => refused("this host has nowhere to write files"),
            Answered::Yes(FileWritten::Failed) => refused("the file could not be written"),
            Answered::No(_) => refused("`Files` did not answer"),
        };
        turn.answer(held, &called)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{FileContents, written};
    use aethereal_data::to_bytes;

    #[test]
    fn a_file_is_written_as_the_contents_that_hold_it() {
        // Catches the read tool's output drifting from `FileContents`, the schema its sheet names, as when the kind
        // gains a field the tool's own writing of it leaves out.
        let text = "a file, with é in it";

        assert_eq!(written(text, Vec::new()), to_bytes(&FileContents { text: String::from(text) }));
    }
}
