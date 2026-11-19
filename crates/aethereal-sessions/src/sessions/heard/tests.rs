use super::{Heard, read};

const ID: &str = "0f4c2a9e-5b1d-4e3f-8a7b-6c5d4e3f2a1b";

#[test]
fn the_events_that_move_a_session_s_turns_are_told_apart() {
    // Catches a turn's start or end, a compaction or its start or end, or a task's start or end read as something else,
    // so the hub never hibernates an idle session, hibernates one with a task running, never finishes winding one down,
    // or lists a compaction the owner asked for as working, or one ended as compacting; and Claude Code's ask
    // to use a tool missed, or another control request taken for one, so a turn waits on an answer no owner is asked
    // for, or the owner is asked what nothing waits on.
    let lines = [
        (format!(r#"{{"type":"system","subtype":"init","session_id":"{ID}"}}"#), Heard::Init),
        (String::from(r#"{"type":"result","subtype":"error_during_execution"}"#), Heard::Result),
        (
            String::from(r#"{"type":"system","subtype":"compact_boundary","compact_metadata":{}}"#),
            Heard::CompactBoundary,
        ),
        (String::from(r#"{"type":"system","subtype":"status","status":"compacting"}"#), Heard::Compacting),
        (String::from(r#"{"type":"system","subtype":"status","status":null}"#), Heard::Status),
        (
            String::from(r#"{"type":"system","subtype":"task_started","task_id":"t1"}"#),
            Heard::TaskStarted(String::from("t1")),
        ),
        (
            String::from(r#"{"type":"system","subtype":"task_notification","task_id":"t1"}"#),
            Heard::TaskEnded(String::from("t1")),
        ),
        (String::from(r#"{"type":"system","subtype":"task_updated","task_id":"t1","status":"running"}"#), Heard::Other),
        (
            String::from(r#"{"type":"system","subtype":"task_updated","task_id":"t1","status":"completed"}"#),
            Heard::TaskEnded(String::from("t1")),
        ),
        (String::from(r#"{"type":"assistant","message":{}}"#), Heard::Other),
        (
            String::from(
                r#"{"type":"control_request","request_id":"r1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls -l"}}}"#,
            ),
            Heard::Asked {
                request: String::from("r1"),
                tool: String::from("Bash"),
                input: String::from(r#"{"command":"ls -l"}"#),
            },
        ),
        (
            String::from(r#"{"type":"control_request","request_id":"r2","request":{"subtype":"interrupt"}}"#),
            Heard::Other,
        ),
    ];
    for (line, expected) in lines {
        assert_eq!(read(&line).0, expected, "{line}");
    }
}

#[test]
fn only_a_uuid_is_taken_as_claude_s_session_id() {
    // Catches an id Claude printed taken as is into the next container's `--resume`: only a UUID is passed on.
    assert_eq!(read(&format!(r#"{{"type":"assistant","session_id":"{ID}"}}"#)).1.as_deref(), Some(ID));
    assert_eq!(read(r#"{"type":"assistant","session_id":"x --mcp-config y"}"#).1, None);
    assert_eq!(read("not json").1, None);
}
