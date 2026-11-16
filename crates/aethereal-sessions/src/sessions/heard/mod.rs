//! What a line of Claude Code's stream-json output says to the hub (ADR-0057): whether a turn began or ended, the
//! session was compacted, or a background task started or ended, and the id of Claude's own session it names.

#[cfg(test)]
mod tests;

use aethereal_data::{Budget, JsonTree, JsonValue, read_json};

/// The bytes of budget each byte of a line may cost as it is read.
const BUDGET_PER_BYTE: usize = 64;

/// The letters of an id Claude Code names its sessions by: a UUID's.
const ID_LETTERS: usize = 36;

/// What an event of Claude's means to the hub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    /// `system` `init`: a turn began, asked for or not.
    Init,
    /// `result`: a turn ended.
    Result,
    /// `system` `compact_boundary`: the session was compacted.
    CompactBoundary,
    /// `system` `task_started`: a background task of this id began.
    TaskStarted(String),
    /// `system` `task_notification`, or `task_updated` to a state it does not leave: the task of this id ended.
    TaskEnded(String),
    /// `control_request` `can_use_tool`: Claude Code asks, by the request's id, whether it may use `tool` with
    /// `input`, the JSON of the input it asked with, and waits for the answer (ADR-0057).
    Asked { request: String, tool: String, input: String },
    /// Anything else.
    Other,
}

/// What the line `line` says, and the id of Claude's session it names: none when it names none, or one that is not a
/// UUID, which is never passed on to a container's command.
#[must_use]
pub fn read(line: &str) -> (Heard, Option<String>) {
    let mut budget = Budget::new(line.len().saturating_mul(BUDGET_PER_BYTE));
    let Ok(tree) = read_json(line, &mut budget) else {
        return (Heard::Other, None);
    };
    let root = tree.root();
    let session = text(&tree, root, "session_id").filter(|id| is_uuid(id)).map(String::from);
    let kind = text(&tree, root, "type");
    let subtype = text(&tree, root, "subtype");
    let task = text(&tree, root, "task_id").map(String::from);
    let is_done = text(&tree, root, "status").is_some_and(is_final);
    if kind == Some("control_request") {
        return (asked(&tree, root).unwrap_or(Heard::Other), session);
    }
    let heard = match (kind, subtype, task) {
        (Some("result"), _, _) => Heard::Result,
        (Some("system"), Some("init"), _) => Heard::Init,
        (Some("system"), Some("compact_boundary"), _) => Heard::CompactBoundary,
        (Some("system"), Some("task_started"), Some(task)) => Heard::TaskStarted(task),
        (Some("system"), Some("task_notification"), Some(task)) => Heard::TaskEnded(task),
        (Some("system"), Some("task_updated"), Some(task)) if is_done => Heard::TaskEnded(task),
        _ => Heard::Other,
    };

    (heard, session)
}

/// What a control request of Claude Code's, `root`, asks the owner: none for a request that asks them nothing, as
/// only `can_use_tool` does.
fn asked(tree: &JsonTree<'_>, root: &JsonValue<'_>) -> Option<Heard> {
    let request = text(tree, root, "request_id")?;
    let JsonValue::Object(span) = root else {
        return None;
    };
    let asking = tree.member(*span, "request")?;
    let subtype = text(tree, asking, "subtype");
    if subtype != Some("can_use_tool") {
        return None;
    }
    let tool = text(tree, asking, "tool_name")?;
    let JsonValue::Object(asked) = asking else {
        return None;
    };
    let mut input = String::new();
    match tree.member(*asked, "input") {
        Some(value) => tree.write(value, &mut input),
        None => input.push_str("{}"),
    }

    Some(Heard::Asked { request: String::from(request), tool: String::from(tool), input })
}

/// The text of the member `name` of `object`: none when it has none, or it is not text.
fn text<'t>(tree: &'t JsonTree<'_>, object: &JsonValue<'_>, name: &str) -> Option<&'t str> {
    let JsonValue::Object(span) = object else {
        return None;
    };
    let Some(JsonValue::String(text)) = tree.member(*span, name) else {
        return None;
    };

    Some(text)
}

/// Whether a task's status is one it does not leave.
fn is_final(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "stopped" | "killed")
}

/// Whether `id` is a UUID, as Claude Code writes one: 36 of lowercase hex digits and dashes, the dashes where a
/// UUID's are.
fn is_uuid(id: &str) -> bool {
    let is_long = id.len() == ID_LETTERS;
    let is_spelled = id.bytes().enumerate().all(|(place, byte)| match place {
        8 | 13 | 18 | 23 => byte == b'-',
        _ => matches!(byte, b'0'..=b'9' | b'a'..=b'f'),
    });

    is_long && is_spelled
}
