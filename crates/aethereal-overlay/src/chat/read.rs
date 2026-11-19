//! What one entry of a session's transcript says to the console (ADR-0057: The console is the chat): an owner's line,
//! a command run at an engine, what the hub did, or what one of Claude's stream-json events shows, read once as the
//! entry arrives.
//!
//! ```text
//! Said, Ran, Interrupted, Answered, Hub ──> one Told each
//! Claude: system init ──> Began;  status compacting ──> Compacting;  compact_boundary ──> Compacted, its token counts
//!         conversation_reset, as `/clear` brings ──> Cleared
//!         assistant ──> a Reply of each text, a Tool of each tool_use;  user ──> a Result of each tool_result
//!         result ──> Ended;  anything else, thinking and rate limits among them ──> nothing
//! each of Claude's carries what it is under: the main conversation, or the tool call that started its subagent
//! ```

use aethereal_data::{Budget, JsonTree, JsonValue, Plain, read_json};
use aethereal_sessions::{Ended, Entry, Note, Speaker};
use aethereal_ui::markdown::Document;

/// The bytes of budget each byte of an event may cost as it is read.
const BUDGET_PER_BYTE: usize = 64;

/// The most characters of a tool's input or result one line shows.
const MOST_SUMMARY_CHARACTERS: usize = 160;

/// The members of a tool's input that say best what it does, in the order they are looked for.
const TELLING_INPUTS: &[&str] = &["command", "file_path", "path", "pattern", "url", "query", "description", "prompt"];

/// What a part of a transcript's entry is under: the main conversation, or the subagent the tool call of this id
/// started, whose events Claude Code marks with `parent_tool_use_id`.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
pub enum Under {
    Main,
    Tool(String),
}

/// One part of a transcript's entry, as the console shows it, and what it is under.
#[derive(Plain, Debug, Clone)]
pub struct Read {
    pub under: Under,
    pub told: Told,
}

/// What a part of an entry tells.
#[derive(Plain, Debug, Clone)]
pub enum Told {
    /// A line said to the session, and who said it.
    Said { by: Speaker, text: String },
    /// A command the owner ran in `!` mode at the engine `engine`, and what it printed.
    Ran { engine: u32, line: String, printed: String },
    /// The session's turn was interrupted, at `by`'s word.
    Interrupted { by: Speaker },
    /// An ask answered, and by whom.
    Answered { ask: u64, allow: bool, by: Speaker },
    /// What the hub did, or a line of the container's own output, shown faint, or as an alert.
    Note { text: String, alert: bool },
    /// A turn began, asked for or not.
    Began,
    /// Text Claude wrote, read as markdown.
    Reply(Document),
    /// A tool Claude called: its call's id, its name, and what its input says best.
    Tool { id: String, name: String, summary: String },
    /// What a tool answered, its first line, and whether it failed.
    Result { summary: String, error: bool },
    /// A turn ended, as its result's subtype says, and whether it failed.
    Ended { subtype: String, error: bool },
    /// The session is being compacted.
    Compacting,
    /// Claude Code's status moved past compacting.
    Status,
    /// The session was compacted: what triggered it, and its tokens before and after, each as Claude Code wrote it or
    /// none when it wrote none.
    Compacted { trigger: String, pre: Option<u64>, post: Option<u64> },
    /// Claude Code began a fresh conversation, as `/clear` has it: the transcript keeps what came before.
    Cleared,
}

impl Read {
    /// `told`, under the main conversation.
    const fn main(told: Told) -> Self {
        Self { under: Under::Main, told }
    }
}

/// What `entry` shows, in order: none for an event of Claude's the console does not show.
pub fn read(entry: &Entry) -> Vec<Read> {
    match entry {
        Entry::Claude(json) => claude(json.as_str()),
        Entry::Stdout { text, .. } => {
            vec![Read::main(Told::Note { text: text.clone(), alert: false })]
        }
        Entry::Stderr { text, .. } => {
            vec![Read::main(Told::Note { text: text.clone(), alert: true })]
        }
        Entry::Said { by, text } => vec![Read::main(Told::Said { by: *by, text: text.clone() })],
        Entry::Interrupted { by } => vec![Read::main(Told::Interrupted { by: *by })],
        Entry::Hub(note) => vec![Read::main(noted(note))],
        Entry::Ran { engine, line, printed } => {
            vec![Read::main(Told::Ran { engine: *engine, line: line.clone(), printed: printed.clone() })]
        }
        Entry::Answered { ask, allow, by } => vec![Read::main(Told::Answered { ask: *ask, allow: *allow, by: *by })],
    }
}

/// What the hub's note says, as a line.
fn noted(note: &Note) -> Told {
    let (text, alert) = match note {
        Note::Started { run, resumed: false } => (format!("run {run} started"), false),
        Note::Started { run, resumed: true } => (format!("run {run} started, resuming"), false),
        Note::Stopped { run, why } => (format!("run {run} stopped: {}", ended(why)), false),
        Note::Unstarted { run, why } => (format!("run {run} did not start: {why}"), true),
        Note::Overdue { step } => (format!("winding down overdue at {step:?}"), true),
        Note::Unwritten { why } => (format!("a line was not written: {why}"), true),
    };

    Told::Note { text, alert }
}

/// How a container stopped, in words.
fn ended(why: &Ended) -> String {
    match why {
        Ended::Idle => String::from("idle"),
        Ended::Archived => String::from("archived"),
        Ended::Exited { code } => format!("exited with {code}"),
        Ended::OutOfMemory => String::from("out of memory"),
        Ended::Lost => String::from("lost"),
    }
}

/// What one of Claude's stream-json events, `line`, shows: nothing for one that does not read.
fn claude(line: &str) -> Vec<Read> {
    let mut budget = Budget::new(line.len().saturating_mul(BUDGET_PER_BYTE));
    let Ok(tree) = read_json(line, &mut budget) else {
        return Vec::new();
    };
    let root = tree.root();
    let under = text(&tree, root, "parent_tool_use_id").map_or(Under::Main, |parent| Under::Tool(String::from(parent)));
    let told = match (text(&tree, root, "type"), text(&tree, root, "subtype")) {
        (Some("system"), Some("init")) => vec![Told::Began],
        (Some("system"), Some("status")) => vec![status(&tree, root)],
        (Some("system"), Some("compact_boundary")) => vec![compacted(&tree, root)],
        (Some("conversation_reset"), _) => vec![Told::Cleared],
        (Some("assistant"), _) => contents(&tree, root).into_iter().filter_map(|part| said(&tree, part)).collect(),
        (Some("user"), _) => contents(&tree, root).into_iter().filter_map(|part| answered(&tree, part)).collect(),
        (Some("result"), subtype) => {
            vec![Told::Ended { subtype: String::from(subtype.unwrap_or("")), error: flag(&tree, root, "is_error") }]
        }
        _ => Vec::new(),
    };

    told.into_iter().map(|told| Read { under: under.clone(), told }).collect()
}

/// A `status` event: compacting, or moved past it.
fn status(tree: &JsonTree<'_>, root: &JsonValue<'_>) -> Told {
    match text(tree, root, "status") {
        Some("compacting") => Told::Compacting,
        _ => Told::Status,
    }
}

/// A `compact_boundary` event, its counts read from its `compact_metadata`, or from the event itself.
fn compacted(tree: &JsonTree<'_>, root: &JsonValue<'_>) -> Told {
    let metadata = member(tree, root, "compact_metadata").unwrap_or(root);
    let count = |name: &str| number(tree, metadata, name).or_else(|| number(tree, root, name));
    let trigger = text(tree, metadata, "trigger").or_else(|| text(tree, root, "trigger")).unwrap_or("");

    Told::Compacted { trigger: String::from(trigger), pre: count("pre_tokens"), post: count("post_tokens") }
}

/// The parts of a message's `content`: none when it is text alone, as a prompt replayed is.
fn contents<'t>(tree: &'t JsonTree<'_>, root: &'t JsonValue<'_>) -> Vec<&'t JsonValue<'t>> {
    let Some(message) = member(tree, root, "message") else {
        return Vec::new();
    };
    let Some(JsonValue::Array(span)) = member(tree, message, "content") else {
        return Vec::new();
    };

    tree.elements(*span).iter().collect()
}

/// A part of an assistant's message: text, which is markdown, or a tool's call. Thinking is not shown.
fn said(tree: &JsonTree<'_>, part: &JsonValue<'_>) -> Option<Told> {
    match text(tree, part, "type")? {
        "text" => {
            let written = text(tree, part, "text")?;
            let blank = written.trim().is_empty();
            if blank {
                return None;
            }
            Some(Told::Reply(Document::new(written)))
        }
        "tool_use" => {
            let id = text(tree, part, "id")?;
            let name = text(tree, part, "name")?;
            let summary = member(tree, part, "input").map_or_else(String::new, |input| summarised(tree, input));
            Some(Told::Tool { id: String::from(id), name: String::from(name), summary })
        }
        _ => None,
    }
}

/// A part of a user's message: what a tool answered. A subagent's prompt, as text, is shown by its tool's call.
fn answered(tree: &JsonTree<'_>, part: &JsonValue<'_>) -> Option<Told> {
    let is_result = text(tree, part, "type")? == "tool_result";
    if !is_result {
        return None;
    }
    let mut written = String::new();
    match member(tree, part, "content") {
        Some(JsonValue::String(content)) => written.push_str(content),
        Some(JsonValue::Array(span)) => {
            for element in tree.elements(*span) {
                if let Some(content) = text(tree, element, "text") {
                    written.push_str(content);
                    written.push('\n');
                }
            }
        }
        Some(_) | None => {}
    }

    Some(Told::Result { summary: first_line(&written), error: flag(tree, part, "is_error") })
}

/// What a tool's input says best: the first of its telling members that is text, or else the input as JSON, on one
/// line, cut short.
fn summarised(tree: &JsonTree<'_>, input: &JsonValue<'_>) -> String {
    let telling = TELLING_INPUTS.iter().find_map(|name| text(tree, input, name));
    telling.map_or_else(
        || {
            let mut json = String::new();
            tree.write(input, &mut json);
            cut(&json)
        },
        first_line,
    )
}

/// The first line of `text`, cut short, and how many lines follow it.
fn first_line(text: &str) -> String {
    let trimmed = text.trim();
    let mut lines = trimmed.lines();
    let first = cut(lines.next().unwrap_or(""));
    let more = lines.count();
    match more {
        0 => first,
        1 => format!("{first} (+1 line)"),
        _ => format!("{first} (+{more} lines)"),
    }
}

/// `text`, cut to the most characters a summary shows.
fn cut(text: &str) -> String {
    let long = text.chars().count() > MOST_SUMMARY_CHARACTERS;
    if !long {
        return String::from(text);
    }
    let kept: String = text.chars().take(MOST_SUMMARY_CHARACTERS - 3).collect();

    format!("{kept}...")
}

/// The member `name` of `object`: none when it has none, or it is not an object.
fn member<'t>(tree: &'t JsonTree<'_>, object: &JsonValue<'_>, name: &str) -> Option<&'t JsonValue<'t>> {
    let JsonValue::Object(span) = object else {
        return None;
    };

    tree.member(*span, name)
}

/// The text of the member `name` of `object`: none when it has none, or it is not text.
fn text<'t>(tree: &'t JsonTree<'_>, object: &JsonValue<'_>, name: &str) -> Option<&'t str> {
    let Some(JsonValue::String(text)) = member(tree, object, name) else {
        return None;
    };

    Some(text)
}

/// The whole number of the member `name` of `object`: none when it has none, or it is not one.
fn number(tree: &JsonTree<'_>, object: &JsonValue<'_>, name: &str) -> Option<u64> {
    let Some(JsonValue::Number(digits)) = member(tree, object, name) else {
        return None;
    };

    digits.parse().ok()
}

/// Whether the member `name` of `object` is `true`.
fn flag(tree: &JsonTree<'_>, object: &JsonValue<'_>, name: &str) -> bool {
    matches!(member(tree, object, name), Some(JsonValue::Boolean(true)))
}
