//! One session's transcript as an engine holds it (ADR-0057: The console is the chat): every entry the hub has shown it,
//! each read once into what it tells, and the rows the console shows of them, built again whenever an entry comes, a
//! fold opens or closes, or the engine learns which lines are its own.
//!
//! ```text
//! Numbered ──> read ──> by number ──> rows: each line tagged [engine], its own full and others' dimmed
//!                                     a subagent's events under the tool call that started it, folded until opened
//!                                     compact_boundary a divider with its tokens; a turn begun with no line marked
//! ```
//!
//! The entries are kept whole, so scrolling back builds the earlier rows from what the engine holds: the scroll builds
//! only the rows in view.

use super::read::{Read, Told, Under, read};
use crate::colour::{BONE, BRICK, LICHEN, OCHRE, STONE};
use aethereal_data::Plain;
use aethereal_render::text::Colour;
use aethereal_sessions::{Numbered, Speaker};
use aethereal_ui::markdown::{Blocks, Document};
use std::collections::{BTreeMap, BTreeSet};

/// How many rows one entry may show: past it, a part's key would run into the next entry's.
const PARTS_PER_ENTRY: u64 = 1 << 16;

/// How a row's text is inked.
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    /// A line this engine's owner typed here.
    Own,
    /// A line typed at another engine, or by a caller of the hub's tools: dimmed.
    Other,
    /// A tool Claude called, or a command run.
    Tool,
    /// What a tool answered, a note of the hub's, a turn's end: faint.
    Faint,
    /// Something that went wrong: an interrupt, a failure.
    Alert,
    /// A divider: the session was compacted.
    Divider,
}

/// One row the console shows of a transcript, kept by its key.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub key: u64,
    pub look: Look,
}

/// What a row is.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
pub enum Look {
    /// A line of text, wrapped to the console, and how it is inked.
    Line { text: String, ink: Ink },
    /// Text Claude wrote, as markdown: the part `part` of the entry numbered `number`.
    Reply { number: u64, part: u32 },
    /// The fold of the subagent the tool call `tool` started, saying how many of its events it holds, and whether it
    /// is open.
    Fold { tool: String, text: String },
}

impl Row {
    /// Whether it is a line of text.
    pub const fn is_line(&self) -> bool {
        matches!(self.look, Look::Line { .. })
    }

    /// Whether it is a subagent's fold.
    pub const fn is_fold(&self) -> bool {
        matches!(self.look, Look::Fold { .. })
    }

    /// Its text: none for a reply, whose blocks are its own.
    pub fn text(&self) -> String {
        match &self.look {
            Look::Line { text, .. } | Look::Fold { text, .. } => text.clone(),
            Look::Reply { .. } => String::new(),
        }
    }

    /// The colour its text is drawn in.
    pub fn colour(&self) -> Colour {
        let swatch = match self.look {
            Look::Line { ink: Ink::Own, .. } => BONE,
            Look::Line { ink: Ink::Tool, .. } => LICHEN,
            Look::Line { ink: Ink::Alert, .. } => BRICK,
            Look::Line { ink: Ink::Divider, .. } => OCHRE,
            Look::Line { ink: Ink::Other | Ink::Faint, .. } | Look::Reply { .. } | Look::Fold { .. } => STONE,
        };

        swatch.opaque()
    }
}

/// Whether the session is being compacted, as its transcript shows it.
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compaction {
    Idle,
    Compacting,
}

/// Whose a line said at an engine is, as far as the engine knows.
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mine {
    /// It has not learned its own id yet: no line is shown as its own.
    Unknown,
    /// Its id.
    Engine(u32),
}

/// A session's transcript as the engine holds it: every entry shown it, read, the subagents' folds the owner opened,
/// and the rows built from them.
#[derive(Plain, Debug, Clone, Default)]
pub struct Transcript {
    entries: BTreeMap<u64, Vec<Read>>,
    /// The tool calls whose subagents' folds are open.
    opened: Vec<String>,
    rows: Vec<Row>,
}

impl Transcript {
    /// The number of the last entry held: 0 for none, from which a tab opens.
    pub fn last(&self) -> u64 {
        self.entries.last_key_value().map_or(0, |(number, _)| *number)
    }

    /// Takes in `events`, each read once: one held already is passed over, so events shown twice, as a tab opened
    /// again shows them, are held once, and events shown out of order are held in order.
    pub fn take(&mut self, events: &[Numbered]) {
        for Numbered { number, entry } in events {
            self.entries.entry(*number).or_insert_with(|| read(entry));
        }
    }

    /// Opens the subagent's fold the row keyed `key` is, or closes it when it is open: nothing for any other row.
    pub fn fold(&mut self, key: u64) {
        let row = self.rows.iter().find(|row| row.key == key);
        let Some(Row { look: Look::Fold { tool, .. }, .. }) = row else {
            return;
        };
        match self.opened.iter().position(|opened| opened == tool) {
            Some(at) => {
                self.opened.remove(at);
            }
            None => self.opened.push(tool.clone()),
        }
    }

    /// The rows, as last built.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The part `part` of the entry numbered `number`: none when the engine holds no such part.
    fn part(&self, number: u64, part: u32) -> Option<&Read> {
        let part = usize::try_from(part).unwrap_or(usize::MAX);

        self.entries.get(&number)?.get(part)
    }

    /// The blocks of the reply the part `part` of the entry `number` is: none when it is not one.
    pub fn reply(&self, number: u64, part: u32) -> Blocks {
        match self.part(number, part) {
            Some(Read { told: Told::Reply(document), .. }) => document.blocks(),
            Some(_) | None => Document::default().blocks(),
        }
    }

    /// Whether the session is being compacted: a `status` of compacting came, and no compaction or other status since.
    pub fn compaction(&self) -> Compaction {
        let latest = self.entries.values().flatten().rev().find_map(|read| match read.told {
            Told::Compacting => Some(Compaction::Compacting),
            Told::Compacted { .. } | Told::Status => Some(Compaction::Idle),
            _ => None,
        });

        latest.unwrap_or(Compaction::Idle)
    }

    /// Builds the rows again, the lines said at `mine` inked as its own. Each part under the main conversation is
    /// shown in order; a tool's call with a subagent's events under it is followed by its fold, and, while the fold is
    /// open, by those events, each subagent's in turn, walked with a stack. Events whose call the engine does not hold
    /// are shown where they come.
    pub fn build(&mut self, mine: Mine) {
        let mut children: BTreeMap<&str, Vec<(u64, u32)>> = BTreeMap::new();
        let mut called: BTreeSet<&str> = BTreeSet::new();
        for (number, reads) in &self.entries {
            for (part, read) in parts(reads) {
                if let Told::Tool { id, .. } = &read.told {
                    called.insert(id);
                }
                if let Under::Tool(parent) = &read.under {
                    children.entry(parent).or_default().push((*number, part));
                }
            }
        }

        let mut rows = Vec::new();
        let mut said = Turned::Said;
        let mut stack: Vec<(u64, u32, u32)> = Vec::new();
        for (number, reads) in self.entries.iter().rev() {
            for (part, read) in parts(reads).rev() {
                let orphan = match &read.under {
                    Under::Main => true,
                    Under::Tool(parent) => !called.contains(&parent.as_str()),
                };
                if orphan {
                    stack.push((*number, part, 0));
                }
            }
        }
        while let Some((number, part, depth)) = stack.pop() {
            let Some(read) = self.part(number, part) else {
                continue;
            };
            let key = number * PARTS_PER_ENTRY + u64::from(part);
            let indent = "  ".repeat(usize::try_from(depth).unwrap_or(0));
            let main = depth == 0;
            if main {
                said = said.after(&read.told);
            }
            rows.extend(shown(&read.told, mine, key, &indent, number, part, said));
            let Told::Tool { id, .. } = &read.told else {
                continue;
            };
            let Some(under) = children.get(id.as_str()) else {
                continue;
            };
            let open = self.opened.contains(id);
            let count = under.len();
            let (mark, verb) = if open {
                ("-", "close")
            } else {
                ("+", "open")
            };
            let text = format!("{indent}  {mark} subagent: {count} events ({verb})");
            rows.push(Row { key: key + PARTS_PER_ENTRY / 2, look: Look::Fold { tool: id.clone(), text } });
            if open {
                stack.extend(under.iter().rev().map(|&(number, part)| (number, part, depth + 1)));
            }
        }

        self.rows = rows;
    }
}

/// Each of an entry's parts, by its place.
fn parts(reads: &[Read]) -> impl DoubleEndedIterator<Item = (u32, &Read)> {
    reads.iter().enumerate().map(|(part, read)| (u32::try_from(part).unwrap_or(u32::MAX), read))
}

/// Whether a line was said since the last turn ended: a turn that begins with none began by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Turned {
    /// A line was said, or the transcript has just begun.
    Said,
    /// A turn ended, and no line was said since.
    Ended,
    /// A turn began with no line said.
    ByItself,
}

impl Turned {
    /// Where it stands after `told`.
    const fn after(self, told: &Told) -> Self {
        match (self, told) {
            (_, Told::Said { .. }) => Self::Said,
            (_, Told::Ended { .. }) => Self::Ended,
            (Self::Ended, Told::Began) => Self::ByItself,
            (Self::ByItself, Told::Began) | (_, _) => self,
        }
    }
}

/// The rows `told` shows, keyed from `key`, indented by `indent`, the lines said at `mine` inked as its own.
fn shown(told: &Told, mine: Mine, key: u64, indent: &str, number: u64, part: u32, said: Turned) -> Vec<Row> {
    let line = |text: String, ink: Ink| Row { key, look: Look::Line { text: format!("{indent}{text}"), ink } };
    match told {
        Told::Said { by, text } => vec![line(format!("{} {text}", tag(*by)), inked(*by, mine))],
        Told::Ran { engine, line: typed, printed } => {
            let ink = inked(Speaker::Engine { engine: *engine }, mine);
            let mut rows = vec![line(format!("[{engine}] /{typed}"), ink)];
            let blank = printed.trim().is_empty();
            if !blank {
                let faint = if ink == Ink::Own {
                    Ink::Tool
                } else {
                    Ink::Faint
                };
                rows.push(Row { key: key + 1, look: Look::Line { text: format!("{indent}{printed}"), ink: faint } });
            }
            rows
        }
        Told::Interrupted { by } => vec![line(format!("{} interrupted the turn", tag(*by)), Ink::Alert)],
        Told::Answered { ask, allow, by } => {
            let verb = if *allow {
                "allowed"
            } else {
                "denied"
            };
            vec![line(format!("{} {verb} ask {ask}", tag(*by)), Ink::Faint)]
        }
        Told::Note { text, alert } => {
            let ink = if *alert {
                Ink::Alert
            } else {
                Ink::Faint
            };
            vec![line(format!("· {text}"), ink)]
        }
        Told::Began if said == Turned::ByItself => vec![line(String::from("· a turn began by itself"), Ink::Faint)],
        Told::Began | Told::Status => Vec::new(),
        Told::Reply(_) => vec![Row { key, look: Look::Reply { number, part } }],
        Told::Tool { name, summary, .. } => vec![line(format!("* {name}  {summary}"), Ink::Tool)],
        Told::Result { summary, error } => {
            let ink = if *error {
                Ink::Alert
            } else {
                Ink::Faint
            };
            vec![line(format!("  = {summary}"), ink)]
        }
        Told::Ended { subtype, error } => {
            let failed = *error || subtype != "success";
            let (text, ink) = if failed {
                (format!("· the turn ended: {subtype}"), Ink::Alert)
            } else {
                (String::from("· done"), Ink::Faint)
            };
            vec![line(text, ink)]
        }
        Told::Compacting => vec![line(String::from("· compacting"), Ink::Faint)],
        Told::Compacted { trigger, pre, post } => {
            let tokens = match (pre, post) {
                (Some(pre), Some(post)) => format!(", {pre} -> {post} tokens"),
                (Some(pre), None) => format!(", {pre} tokens before"),
                (None, Some(post)) => format!(", {post} tokens after"),
                (None, None) => String::new(),
            };
            vec![line(format!("--- compacted ({trigger}{tokens}) ---"), Ink::Divider)]
        }
    }
}

/// The tag a line said by `by` is shown with: its engine's id, or who else said it.
fn tag(by: Speaker) -> String {
    match by {
        Speaker::Engine { engine } => format!("[{engine}]"),
        Speaker::Caller => String::from("[tools]"),
        Speaker::Hub => String::from("[hub]"),
    }
}

/// How a line said by `by` is inked: in full when `mine` said it, and dimmed otherwise.
fn inked(by: Speaker, mine: Mine) -> Ink {
    match (by, mine) {
        (Speaker::Engine { engine }, Mine::Engine(own)) if engine == own => Ink::Own,
        _ => Ink::Other,
    }
}
