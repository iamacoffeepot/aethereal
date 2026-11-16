//! A session's transcript (ADR-0057): every line its container wrote and every line the hub wrote to it, numbered in
//! order within the session, kept on disk as frames appended one after another.

#[cfg(test)]
mod tests;

use super::heard::{self, Heard};
use aethereal_data::{Budget, Json, Plain, from_bytes, to_bytes};

/// The most bytes one entry's frame holds as it is read back: past any line a container writes.
const MOST_ENTRY_BYTES: usize = 16 << 20;

/// One entry of a transcript, with its number: the first is 1.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.numbered")]
pub struct Numbered {
    pub number: u64,
    pub entry: Entry,
}

/// What one entry of a transcript is.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.entry")]
pub enum Entry {
    /// A line of the container's stdout that is one JSON value: an event of Claude's.
    Claude(Json),
    /// A line of the container's stdout that is not JSON, as text, and how many bytes it was.
    Stdout { text: String, bytes: u64 },
    /// A line of the container's stderr, as text, and how many bytes it was.
    Stderr { text: String, bytes: u64 },
    /// A line written to the session as a user's message, and who wrote it.
    Said { by: Speaker, text: String },
    /// An interrupt written to the session's running turn, and who asked for it.
    Interrupted { by: Speaker },
    /// Something the hub did with the session's container, or saw become of it.
    Hub(Note),
    /// A `/` command the owner ran at the trusted engine `engine`, with the session's tab open, and what it printed:
    /// written to the session at the head of the owner's next line, and starting no turn (ADR-0057).
    Ran { engine: u32, line: String, printed: String },
    /// The owner's answer to the ask numbered `ask`, written to the session as Claude Code's control response, and
    /// who gave it.
    Answered { ask: u64, allow: bool, by: Speaker },
}

/// Who wrote a line to a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.speaker")]
pub enum Speaker {
    /// A caller of the hub's tools.
    Caller,
    /// The hub itself, as it winds an idle session down.
    Hub,
    /// The owner, at the trusted engine `engine`, whose line Claude reads tagged `[engine]` (ADR-0057).
    Engine { engine: u32 },
}

/// A `/` command the owner ran at an engine, as the transcript holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran<'a> {
    pub engine: u32,
    pub line: &'a str,
    pub printed: &'a str,
}

/// Something the hub did with a session's container, or saw become of it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.note")]
pub enum Note {
    /// The container of `run` started, continuing the conversation when `resumed`.
    Started { run: u32, resumed: bool },
    /// The container of `run` was stopped or ended, as `why` says.
    Stopped { run: u32, why: Ended },
    /// The container of `run` could not be made or started, as `why` says, and a line said meanwhile went nowhere.
    Unstarted { run: u32, why: String },
    /// A step of winding an idle session down took past its deadline: the session is stopped anyway.
    Overdue { step: WindDown },
    /// A line to the session's stdin was not written, as `why` says.
    Unwritten { why: String },
}

/// How a session's container came to stop.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.ended")]
pub enum Ended {
    /// The hub stopped it, as nothing ran in it for the idle time.
    Idle,
    /// The hub stopped it, as the session was archived.
    Archived,
    /// Its process ended by itself, with `code`.
    Exited { code: i64 },
    /// The kernel killed it for its memory.
    OutOfMemory,
    /// It was gone, or not running, when the hub came back to it.
    Lost,
}

/// A step of winding an idle session down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.wind_down")]
pub enum WindDown {
    /// The turn that answers the notice.
    Notice,
    /// The compaction.
    Compaction,
}

/// Whether a session has had a turn since it was last compacted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Since {
    /// It has not: there is nothing new to compact.
    Compacted,
    /// It has.
    Turned,
}

/// A session's transcript, held whole, with what it says of the session's container's output and its compaction.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub struct Transcript {
    entries: Vec<Numbered>,
    /// The bytes of the latest container's stdout and stderr the transcript holds, which a re-attach skips.
    stdout_bytes: u64,
    stderr_bytes: u64,
    since: Since,
    turn: Turning,
}

/// Whether the latest container's session has a turn running, as far as the transcript shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Turning {
    /// No turn runs: none began since the last ended, and no line was written since.
    Ended,
    /// A line was written, or a turn began, and no turn has ended since.
    Running,
}

impl Transcript {
    /// A transcript of nothing.
    #[must_use]
    pub const fn new() -> Self {
        Self { entries: Vec::new(), stdout_bytes: 0, stderr_bytes: 0, since: Since::Compacted, turn: Turning::Ended }
    }

    /// The transcript `bytes` keep, its frames read in order: a frame cut short at the end, as a crash mid-append
    /// leaves, is left out, and so is everything from a frame that does not read.
    #[must_use]
    pub fn read(bytes: &[u8]) -> Self {
        let mut transcript = Self::new();
        let mut rest = bytes;
        while let Some((length, body)) = rest.split_first_chunk::<4>() {
            let length = usize::try_from(u32::from_le_bytes(*length)).unwrap_or(usize::MAX);
            let Some((frame, after)) = body.split_at_checked(length) else {
                break;
            };
            let Ok(numbered) = from_bytes::<Numbered>(frame, &mut Budget::new(MOST_ENTRY_BYTES)) else {
                break;
            };
            transcript.take(numbered);
            rest = after;
        }

        transcript
    }

    /// Adds `entry` as the next, and answers it numbered.
    pub fn add(&mut self, entry: Entry) -> Numbered {
        let numbered = Numbered { number: self.last() + 1, entry };
        self.take(numbered.clone());

        numbered
    }

    /// Takes `numbered` in, as the next, learning from it.
    fn take(&mut self, numbered: Numbered) {
        match &numbered.entry {
            Entry::Claude(json) => {
                self.stdout_bytes += line_bytes(json.as_str().len());
                match heard::read(json.as_str()).0 {
                    Heard::Result => {
                        self.since = Since::Turned;
                        self.turn = Turning::Ended;
                    }
                    Heard::Init => self.turn = Turning::Running,
                    Heard::CompactBoundary => self.since = Since::Compacted,
                    Heard::TaskStarted(_) | Heard::TaskEnded(_) | Heard::Asked { .. } | Heard::Other => {}
                }
            }
            Entry::Stdout { bytes, .. } => self.stdout_bytes += bytes + 1,
            Entry::Stderr { bytes, .. } => self.stderr_bytes += bytes + 1,
            Entry::Hub(Note::Started { .. }) => {
                self.stdout_bytes = 0;
                self.stderr_bytes = 0;
                self.turn = Turning::Ended;
            }
            Entry::Said { .. } => self.turn = Turning::Running,
            Entry::Interrupted { .. } | Entry::Hub(_) | Entry::Ran { .. } | Entry::Answered { .. } => {}
        }
        self.entries.push(numbered);
    }

    /// The number of the last entry: 0 for none.
    #[must_use]
    pub fn last(&self) -> u64 {
        self.entries.last().map_or(0, |numbered| numbered.number)
    }

    /// At most `most` entries after the number `after`, in order.
    #[must_use]
    pub fn after(&self, after: u64, most: usize) -> Vec<Numbered> {
        let start = self.entries.partition_point(|numbered| numbered.number <= after);

        self.entries[start..].iter().take(most).cloned().collect()
    }

    /// Each `/` command an owner ran since the last line an owner said, in order: what the next line folds in at its
    /// head. A line of the hub's own is not an owner's.
    #[must_use]
    pub fn ran_since_said(&self) -> Vec<Ran<'_>> {
        let said = self.entries.iter().rposition(|numbered| numbered.entry.is_owner_s_line());
        let start = said.map_or(0, |place| place + 1);

        self.entries[start..]
            .iter()
            .filter_map(|numbered| match &numbered.entry {
                Entry::Ran { engine, line, printed } => Some(Ran { engine: *engine, line, printed }),
                _ => None,
            })
            .collect()
    }

    /// The bytes of the latest container's stdout and of its stderr the transcript holds.
    #[must_use]
    pub const fn output_bytes(&self) -> (u64, u64) {
        (self.stdout_bytes, self.stderr_bytes)
    }

    /// Whether the latest container's session has a turn running, as far as the transcript shows.
    #[must_use]
    pub const fn turning(&self) -> Turning {
        self.turn
    }

    /// Whether the session has had a turn since its last compaction.
    #[must_use]
    pub const fn since(&self) -> Since {
        self.since
    }
}

impl Default for Transcript {
    fn default() -> Self {
        Self::new()
    }
}

impl Entry {
    /// Whether it is a line an owner said: a caller's of the hub's tools, or one typed at an engine, that is not one of
    /// Claude Code's own commands, which folds nothing in.
    fn is_owner_s_line(&self) -> bool {
        match self {
            Self::Said { by: Speaker::Caller | Speaker::Engine { .. }, text } => !is_command(text),
            _ => false,
        }
    }
}

/// Whether `text` is one of Claude Code's own commands, `/compact` and the like: a line that starts with `/`, which
/// Claude Code reads as its command only when it is the whole message, untagged (ADR-0057: The console is the chat).
#[must_use]
pub fn is_command(text: &str) -> bool {
    text.starts_with('/')
}

impl Numbered {
    /// The entry as one frame of the transcript's file: its length, then its bytes.
    ///
    /// # Errors
    ///
    /// The entry would not encode.
    pub fn frame(&self) -> Result<Vec<u8>, aethereal_data::EncodeError> {
        let bytes = to_bytes(self)?;
        let length = u32::try_from(bytes.len()).unwrap_or(u32::MAX).to_le_bytes();

        Ok([&length[..], &bytes].concat())
    }
}

/// The bytes a line of `length` took in the output, with its newline.
fn line_bytes(length: usize) -> u64 {
    u64::try_from(length).unwrap_or(u64::MAX) + 1
}
