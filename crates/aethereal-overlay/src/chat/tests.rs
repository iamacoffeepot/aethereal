//! The console's chat, as its state: what the hub shows it, built into rows, and what the owner does, come to the chat
//! the link is told. Claude's events are written as Claude Code 2.1 writes them, cut to the members the console reads.

use super::{Chats, Ink, Look, Open, Row, Typed, typed};
use aethereal_data::{Budget, Json};
use aethereal_sessions::{Chat, Entry, Numbered, ShowAsk, Speaker, State, Tab};

/// A session's tab, `session`, named so, in `state`, with `asking` asks waiting.
fn tab(session: u32, state: State, asking: u32) -> Tab {
    Tab { session, name: format!("s{session}"), state, asking }
}

/// The entry numbered `number`: Claude's event `json`.
fn claude(number: u64, json: &str) -> Numbered {
    let json = Json::parse(json, &mut Budget::new(1 << 16)).expect("the test's event is JSON");

    Numbered { number, entry: Entry::Claude(json) }
}

/// The entry numbered `number`: `text`, said at the engine `engine`.
fn said(number: u64, engine: u32, text: &str) -> Numbered {
    Numbered { number, entry: Entry::Said { by: Speaker::Engine { engine }, text: String::from(text) } }
}

/// The text of each line of `rows`, a reply as `<reply>`.
fn texts(rows: &[&Row]) -> Vec<String> {
    rows.iter()
        .map(|row| match &row.look {
            Look::Line { text, .. } | Look::Fold { text, .. } => text.clone(),
            Look::Reply { .. } => String::from("<reply>"),
        })
        .collect()
}

/// Chats with the session 1 listed, idle, and its tab open.
fn opened() -> Chats {
    let mut chats = Chats::default();
    let _ = chats.listed(vec![tab(1, State::Idle, 0)]);

    chats
}

const TOOL: &str = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Agent","input":{"description":"Run echo sub"}}]},"parent_tool_use_id":null}"#;
const SUB_CALL: &str = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_2","name":"Bash","input":{"command":"echo sub"}}]},"parent_tool_use_id":"toolu_1"}"#;
const SUB_RESULT: &str = r#"{"type":"user","message":{"content":[{"tool_use_id":"toolu_2","type":"tool_result","content":"sub","is_error":false}]},"parent_tool_use_id":"toolu_1"}"#;
const RESULT: &str = r#"{"type":"user","message":{"content":[{"tool_use_id":"toolu_1","type":"tool_result","content":[{"type":"text","text":"sub\nmore"}]}]},"parent_tool_use_id":null}"#;
const REPLY: &str = r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":""},{"type":"text","text":"It printed **sub**."}]},"parent_tool_use_id":null}"#;

#[test]
fn a_subagents_events_fold_under_the_call_that_started_it_closed_until_opened() {
    // Catches a subagent's events shown inline in the main conversation, a fold that does not open, one that opens
    // somewhere other than beneath its call, and thinking shown as a reply.
    let mut chats = opened();
    let events = [claude(1, TOOL), claude(2, SUB_CALL), claude(3, SUB_RESULT), claude(4, RESULT), claude(5, REPLY)];
    chats.shown(1, &events);

    assert_eq!(
        texts(&chats.rows()),
        ["* Agent  Run echo sub", "  + subagent: 2 events (open)", "  = sub (+1 line)", "<reply>"]
    );

    let fold = chats.rows()[1].key;
    chats.fold(fold);

    assert_eq!(
        texts(&chats.rows()),
        [
            "* Agent  Run echo sub",
            "  - subagent: 2 events (close)",
            "  * Bash  echo sub",
            "    = sub",
            "  = sub (+1 line)",
            "<reply>",
        ]
    );
}

#[test]
fn a_compaction_is_a_divider_with_its_tokens_and_the_tab_is_busy_while_it_runs() {
    // Catches a compaction left out of the transcript or shown without its counts, and a tab that shows the session
    // idle while it compacts, or busy once it has.
    let mut chats = opened();
    chats.shown(1, &[claude(1, r#"{"type":"system","subtype":"status","status":"compacting"}"#)]);

    assert_eq!(chats.titles()[1].name, "s1 *");

    let boundary = r#"{"type":"system","subtype":"compact_boundary","compact_metadata":{"trigger":"manual","pre_tokens":52000,"post_tokens":3100}}"#;
    chats.shown(1, &[claude(2, boundary)]);

    assert_eq!(chats.titles()[1].name, "s1");
    assert_eq!(
        texts(&chats.rows()).last().map(String::as_str),
        Some("--- compacted (manual, 52000 -> 3100 tokens) ---")
    );
}

#[test]
fn a_turn_that_begins_with_no_line_says_so_and_one_begun_by_a_line_does_not() {
    // Catches a turn a background subagent started shown as though the owner's line began it, and every turn marked as
    // begun by itself.
    let mut chats = opened();
    let init = r#"{"type":"system","subtype":"init"}"#;
    let done = r#"{"type":"result","subtype":"success","is_error":false}"#;
    chats.shown(1, &[said(1, 4, "go"), claude(2, init), claude(3, done), claude(4, init), claude(5, done)]);

    assert_eq!(texts(&chats.rows()), ["[4] go", "· done", "· a turn began by itself", "· done"]);
}

#[test]
fn the_engine_learns_its_id_from_its_own_line_echoed_and_dims_every_other_engine_s() {
    // Catches an engine that inks every line as its own, or none; one that takes another engine's line of other text
    // as its own; and lines shown before the echo left inked as others' once it is learned.
    let mut chats = opened();
    chats.shown(1, &[said(1, 12, "from another engine")]);
    let sent = chats.say(String::from("what do you see?"));

    assert_eq!(sent, [Chat::Say { session: 1, text: String::from("what do you see?") }]);

    chats.shown(1, &[said(2, 12, "something else"), said(3, 37, "what do you see?")]);
    let inks: Vec<Ink> = chats
        .rows()
        .iter()
        .map(|row| match row.look {
            Look::Line { ink, .. } => ink,
            Look::Reply { .. } | Look::Fold { .. } => Ink::Faint,
        })
        .collect();

    assert_eq!(inks, [Ink::Other, Ink::Other, Ink::Own]);
}

#[test]
fn a_line_goes_to_claude_a_slash_line_runs_here_and_claude_codes_own_words_are_said() {
    // Catches `/compact` run as one of the engine's tools rather than sent, a `/` command sent to Claude, and the
    // chat's own words, `/stop`, `/new`, and `/archive`, missed.
    assert_eq!(typed("what do you see?"), Typed::Say(String::from("what do you see?")));
    assert_eq!(typed("/capture_frame --window 1"), Typed::Run(String::from("capture_frame --window 1")));
    assert_eq!(typed("/compact"), Typed::Say(String::from("/compact")));
    assert_eq!(typed("/stop"), Typed::Stop);
    assert_eq!(typed("/new  overlay work"), Typed::New(String::from("overlay work")));
    assert_eq!(typed("/archive"), Typed::Archive);
    assert_eq!(typed("   "), Typed::Blank);
}

#[test]
fn the_first_listing_opens_the_newest_session_and_a_tab_opens_again_from_what_it_holds() {
    // Catches a console that never opens a session's tab, one that opens an archived session, a tab chosen without
    // the one open closed, and a tab opened again from the start rather than from the last entry it holds.
    let mut chats = Chats::default();
    let first = chats.listed(vec![tab(1, State::Idle, 0), tab(2, State::Working, 0), tab(3, State::Archived, 0)]);

    assert_eq!(first, [Chat::Open { session: 2, after: 0 }]);
    assert_eq!(chats.open(), Open::Session(2));

    chats.shown(2, &[said(1, 4, "a"), said(2, 4, "b")]);

    assert_eq!(chats.choose(1), [Chat::Close { session: 2 }, Chat::Open { session: 1, after: 0 }]);
    assert_eq!(chats.choose(2), [Chat::Close { session: 1 }, Chat::Open { session: 2, after: 2 }]);
    assert_eq!(chats.choose(0), [Chat::Close { session: 2 }]);
    assert_eq!(chats.choose(0), []);
}

#[test]
fn an_open_session_archived_leaves_the_console_s_own_tab_open() {
    // Catches a console left showing a session that no longer lists as a tab, so its lines would go to an archived
    // session.
    let mut chats = opened();

    assert_eq!(chats.listed(vec![tab(1, State::Archived, 0)]), [Chat::Close { session: 1 }]);
    assert_eq!(chats.open(), Open::Console);
    assert_eq!(chats.say(String::from("anyone?")), []);
}

#[test]
fn events_shown_twice_or_out_of_order_are_held_once_in_order() {
    // Catches a tab opened again doubling the events it held, and a backlog the hub split shown in the order its parts
    // came rather than by number.
    let mut chats = opened();
    chats.shown(1, &[said(3, 4, "c"), said(1, 4, "a")]);
    chats.shown(1, &[said(1, 4, "a"), said(2, 4, "b"), said(3, 4, "c")]);

    assert_eq!(texts(&chats.rows()), ["[4] a", "[4] b", "[4] c"]);
}

#[test]
fn an_ask_waits_until_answered_here_or_its_session_waits_on_none() {
    // Catches an ask answered here still shown, one answered at another engine shown forever, and the answer's chat.
    let mut chats = opened();
    chats.asked(ShowAsk { ask: 7, session: 1, what: String::from("Bash {}") });
    chats.asked(ShowAsk { ask: 8, session: 1, what: String::from("Edit {}") });

    assert_eq!(chats.answer(7, true), Chat::Answer { ask: 7, allow: true });
    assert_eq!(chats.asks().len(), 1);

    let _ = chats.listed(vec![tab(1, State::Working, 0)]);

    assert!(chats.asks().is_empty());
}
