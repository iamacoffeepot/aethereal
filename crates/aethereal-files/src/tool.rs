//! The file capability's tool (ADR-0037).

use crate::{FileRead, Files, ReadFile};
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anywhere, Asks, Beginning, ChildOf, Ending, Fault, Handles, Held, Path,
    ProtocolRef, ToSender, Turn, path,
};
use aethereal_data::{Bound, Budget, Plain, Schema, SchemaNodes, from_bytes, to_bytes};
use aethereal_mcp::{Call, Called, Tool, ToolSheet, Tooling};
use std::collections::BTreeMap;

/// The most bytes of a file the tool answers with. Its answer is one piece of mail, and mail its reader cannot read
/// ends it, so a program sets `read_limit` above this; past it, the tool refuses.
const MOST_BYTES: usize = 256 * 1024;
/// The most bytes reading an input may take. A path is short.
const INPUT_BYTES: usize = 8 * 1024;

/// What an agent reads of a file: its text. A `FileRead` holds a blob, which has no JSON form.
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.files.file_contents")]
pub struct FileContents {
    pub text: String,
}

/// What the tool keeps for a read it asked: which it was.
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
        vec![ToolSheet {
            name: String::from("read_file"),
            description: format!("Reads a text file of the program's own, up to {MOST_BYTES} bytes."),
            input: SchemaNodes::of(&ReadFile::SHAPE),
            output: SchemaNodes::of(&FileContents::SHAPE),
        }]
    }

    fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling> {
        me.protocol()
    }
}

impl Handles<Call> for ReadTool {
    type Sends = (Asks<Path<Files>, ReadFile, Calling>, ToSender<Called>);

    fn handle(&mut self, call: Call, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Ok(read) = from_bytes::<ReadFile>(&call.input, &mut Budget::new(INPUT_BYTES)) else {
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
            Answered::Yes(FileRead::Read { bytes }) => contents(turn.read(&bytes))?,
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

/// What the tool answers of a file of `bytes`.
fn contents(bytes: &[u8]) -> Result<Called, Fault> {
    if bytes.len() > MOST_BYTES {
        return Ok(refused("the file is too large"));
    }
    let Ok(text) = str::from_utf8(bytes) else {
        return Ok(refused("the file is not text"));
    };

    Ok(Called::Output(to_bytes(&FileContents { text: String::from(text) })?))
}
