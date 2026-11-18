//! One session's transcript as an engine holds it (ADR-0057: The console is the chat): every entry the hub has shown it,
//! each read once into what it tells, and the rows the console shows of them, built again whenever an entry comes, a
//! fold opens or closes, or the engine learns which lines are its own.
//!
//! ```text
//! Numbered ──> read ──> by number ──> rows: each owner's line a turn, tagged `local >`, or `name >` at another
//!                                     engine, on the raised band, another engine's tag dimmed; a command run in
//!                                     `!` mode tagged `local !`, what it printed under it
//!                                     a subagent's events under the tool call that started it, folded until opened
//!                                     compact_boundary a centred divider with its tokens; conversation_reset one
//!                                     saying it was cleared; a turn begun with no line marked
//! ```
//!
//! The entries are kept whole, so scrolling back builds the earlier rows from what the engine holds: the scroll builds
//! only the rows in view.

use super::read::{Container, Read, Told, Under, read};
use crate::colour::{Ink, Palette};
use crate::console::{LOCAL, Mode, Shade};
use aethereal_data::Plain;
use aethereal_render::text::Run;
use aethereal_sessions::{Numbered, Speaker, engine_words};
use aethereal_ui::markdown::{Blocks, Document};
use std::collections::{BTreeMap, BTreeSet};

/// How opaque the tag of a line typed at another engine is drawn: dimmed, as the line beside it is.
const OTHER_TAG_OPACITY: f32 = 0.7;

/// How many rows one entry may show: past it, a part's key would run into the next entry's.
const PARTS_PER_ENTRY: u64 = 1 << 16;

/// Whose a row's text is, and so how it is inked.
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// A line this engine's owner typed here: a turn of the owner's.
    Own,
    /// A line the owner typed at another engine, its tag dimmed: a turn of the owner's too.
    Other,
    /// A line written by a caller of the hub's tools, or the hub: dimmed, and not the owner's turn.
    Written,
    /// A tool Claude called, or a command run.
    Tool,
    /// What a tool answered, a note of the hub's, a turn's end: dim.
    Faint,
    /// Something that went wrong: an interrupt, a failure.
    Alert,
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
    /// A line of text, wrapped to the console, and whose it is.
    Line { text: String, tone: Tone },
    /// A line the owner typed, at this engine or another, `text`, after `tag`, who typed it and the mark of the `mode`
    /// it was typed in, which is drawn as a label in an ink of its own; [`Tone::Own`] or [`Tone::Other`].
    Turn { tag: String, mode: Mode, text: String, tone: Tone },
    /// A divider across the transcript, `text` centred on it: the session was compacted, or its conversation cleared.
    Divider { text: String },
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

    /// Whether it is a divider.
    pub const fn is_divider(&self) -> bool {
        matches!(self.look, Look::Divider { .. })
    }

    /// Whether it is a subagent's fold.
    pub const fn is_fold(&self) -> bool {
        matches!(self.look, Look::Fold { .. })
    }

    /// Its text: none for a reply, whose blocks are its own.
    pub fn text(&self) -> String {
        match &self.look {
            Look::Line { text, .. } | Look::Turn { text, .. } | Look::Fold { text, .. } | Look::Divider { text } => {
                text.clone()
            }
            Look::Reply { .. } => String::new(),
        }
    }

    /// The ink its text is drawn in: the owner's lines, at this engine or another, on their raised band, and a tool's
    /// call, in lichen; what went wrong in the error's; a divider in the mark's; and the rest dim.
    pub const fn ink(&self) -> Ink {
        match self.look {
            Look::Line { tone: Tone::Own | Tone::Tool, .. } | Look::Turn { .. } => Ink::Lichen,
            Look::Line { tone: Tone::Alert, .. } => Ink::Error,
            Look::Divider { .. } => Ink::Mark,
            Look::Line { tone: Tone::Other | Tone::Written | Tone::Faint, .. }
            | Look::Reply { .. }
            | Look::Fold { .. } => Ink::Dim,
        }
    }

    /// Whether it is a line the owner typed at this engine, tagged `local`.
    pub const fn is_own_turn(&self) -> bool {
        matches!(self.look, Look::Turn { tone: Tone::Own, .. })
    }

    /// Whether it is a line the owner typed at another engine, tagged with that engine's name.
    pub const fn is_other_turn(&self) -> bool {
        matches!(
            self.look,
            Look::Turn { tone: Tone::Other | Tone::Written | Tone::Tool | Tone::Faint | Tone::Alert, .. }
        )
    }

    /// Whether it is a line the owner typed, at this engine or another: set apart from Claude's on a shade of its own.
    pub const fn is_turn(&self) -> bool {
        matches!(self.look, Look::Turn { .. })
    }

    /// Its tag in `palette`, who typed it then its mode's mark: who in the label's ink, apart from the lichen of the
    /// text beside it, and dimmed for a line typed at another engine; the mark in the text's own ink, as it is
    /// punctuation, not the label. None but for one of the owner's lines.
    pub fn tag_runs(&self, palette: &Palette) -> Vec<Run> {
        let (who, mode) = match &self.look {
            Look::Turn { tag, mode, tone: Tone::Own, .. } => (palette.run(tag, Ink::Label), *mode),
            Look::Turn { tag, mode, .. } => (Run::new(tag, palette.faded(Ink::Label, OTHER_TAG_OPACITY)), *mode),
            Look::Line { .. } | Look::Reply { .. } | Look::Fold { .. } | Look::Divider { .. } => return Vec::new(),
        };

        vec![who, palette.run(mode.tag_mark(), self.ink())]
    }

    /// What it is drawn on: the owner's lines on the raised band, as a prompt's lines are in a terminal, and the rest
    /// on the transcript's own sheet.
    pub const fn shade(&self) -> Shade {
        if self.is_turn() {
            Shade::Raised
        } else {
            Shade::Sheet
        }
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

    /// What the latest note that says anything of the session's container says: unsaid when none does.
    pub fn container(&self) -> Container {
        let latest = self.entries.values().flatten().rev().find_map(|read| match read.told {
            Told::Note { container: Container::Unsaid, .. } => None,
            Told::Note { container, .. } => Some(container),
            _ => None,
        });

        latest.unwrap_or(Container::Unsaid)
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
            (_, Told::Said { .. } | Told::Ran { .. }) => Self::Said,
            (_, Told::Ended { .. }) => Self::Ended,
            (Self::Ended, Told::Began) => Self::ByItself,
            (Self::ByItself, Told::Began) | (_, _) => self,
        }
    }
}

/// The rows `told` shows, keyed from `key`, indented by `indent`, the lines said at `mine` toned as its own.
fn shown(told: &Told, mine: Mine, key: u64, indent: &str, number: u64, part: u32, said: Turned) -> Vec<Row> {
    let line = |text: String, tone: Tone| Row { key, look: Look::Line { text: format!("{indent}{text}"), tone } };
    let turn = |by: Speaker, mode: Mode, text: String| {
        let tag = format!("{indent}{}", who(by, mine));
        Row { key, look: Look::Turn { tag, mode, text, tone: toned(by, mine) } }
    };
    match told {
        Told::Said { by: by @ Speaker::Engine { .. }, text } => vec![turn(*by, Mode::Chat, text.clone())],
        Told::Said { by, text } => vec![line(format!("{} {text}", who(*by, mine)), toned(*by, mine))],
        Told::Ran { engine, line: typed, printed } => {
            let by = Speaker::Engine { engine: *engine };
            let mut rows = vec![turn(by, Mode::Bang, typed.clone())];
            let blank = printed.trim().is_empty();
            if !blank {
                let text = format!("{indent}{printed}");
                rows.push(Row { key: key + 1, look: Look::Line { text, tone: Tone::Faint } });
            }
            rows
        }
        Told::Interrupted { by } => vec![line(format!("{} interrupted the turn", who(*by, mine)), Tone::Alert)],
        Told::Answered { ask, allow, by } => {
            let verb = if *allow {
                "allowed"
            } else {
                "denied"
            };
            vec![line(format!("{} {verb} ask {ask}", who(*by, mine)), Tone::Faint)]
        }
        Told::Note { text, alert, .. } => {
            let tone = if *alert {
                Tone::Alert
            } else {
                Tone::Faint
            };
            vec![line(format!("· {text}"), tone)]
        }
        Told::Began if said == Turned::ByItself => vec![line(String::from("· a turn began by itself"), Tone::Faint)],
        Told::Began | Told::Status => Vec::new(),
        Told::Reply(_) => vec![Row { key, look: Look::Reply { number, part } }],
        Told::Tool { name, summary, .. } => vec![line(format!("* {name}  {summary}"), Tone::Tool)],
        Told::Result { summary, error } => {
            let tone = if *error {
                Tone::Alert
            } else {
                Tone::Faint
            };
            vec![line(format!("  = {summary}"), tone)]
        }
        Told::Ended { subtype, error } => {
            let failed = *error || subtype != "success";
            let (text, tone) = if failed {
                (format!("· the turn ended: {subtype}"), Tone::Alert)
            } else {
                (String::from("· done"), Tone::Faint)
            };
            vec![line(text, tone)]
        }
        Told::Compacting => vec![line(String::from("· compacting"), Tone::Faint)],
        Told::Compacted { pre, post, .. } => {
            let tokens = match (pre, post) {
                (Some(pre), Some(post)) => format!(" · {} → {} tokens", thousands(*pre), thousands(*post)),
                (Some(pre), None) => format!(" · {} tokens before", thousands(*pre)),
                (None, Some(post)) => format!(" · {} tokens after", thousands(*post)),
                (None, None) => String::new(),
            };
            vec![Row { key, look: Look::Divider { text: format!("compacted{tokens}") } }]
        }
        Told::Cleared => vec![Row { key, look: Look::Divider { text: String::from("cleared") } }],
    }
}

/// A count of tokens as a divider says it: whole under a thousand, and else in thousands, to the nearest: `840`, `31k`,
/// `182k`.
fn thousands(tokens: u64) -> String {
    if tokens < 1_000 {
        return tokens.to_string();
    }

    format!("{}k", tokens.saturating_add(500) / 1_000)
}

/// Who `by` is, as the engine that knows itself as `mine` names it: `local` for itself, another engine by its name's
/// words alone, `mellow-snipe`, its id kept out of the transcript, or who else wrote to the session.
fn who(by: Speaker, mine: Mine) -> String {
    match (by, mine) {
        (Speaker::Engine { engine }, Mine::Engine(own)) if engine == own => String::from(LOCAL),
        (Speaker::Engine { engine }, Mine::Engine(_) | Mine::Unknown) => engine_words(engine),
        (Speaker::Caller, _) => String::from("[tools]"),
        (Speaker::Hub, _) => String::from("[hub]"),
    }
}

/// Whose a line said by `by` is: the owner's here when `mine` said it, the owner's at another engine, and not the
/// owner's turn when written by a caller of the hub's tools or the hub.
fn toned(by: Speaker, mine: Mine) -> Tone {
    match (by, mine) {
        (Speaker::Engine { engine }, Mine::Engine(own)) if engine == own => Tone::Own,
        (Speaker::Engine { .. }, Mine::Engine(_) | Mine::Unknown) => Tone::Other,
        (Speaker::Caller | Speaker::Hub, _) => Tone::Written,
    }
}
