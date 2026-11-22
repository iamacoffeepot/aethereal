//! The console's chat, as its state: what the hub shows it, built into rows, and what the owner does, come to the chat
//! the link is told. Claude's events are written as Claude Code 2.1 writes them, cut to the members the console reads.

use super::{Chats, Icon, Look, Open, Row, Status, Tone, Typed, Which, typed};
use crate::console::Mode;
use aethereal_data::{Budget, Json};
use aethereal_sessions::{Chat, Connection, Entry, Grew, Numbered, Refusal, SessionRow, ShowAsk, Speaker, State};

/// A session's row, `session`, named so, in `state`, its container as `state` has it, with `asking` asks waiting.
fn tab(session: u32, state: State, asking: u32) -> SessionRow {
    row(session, format!("s{session}"), state, asking)
}

/// A session's row, `session`, named `name`, in `state`, its container as `state` has it, with `asking` asks waiting.
fn row(session: u32, name: String, state: State, asking: u32) -> SessionRow {
    let container = match state {
        State::Hibernating | State::Archived => Connection::None,
        State::Idle | State::Working | State::Waiting | State::Compacting => Connection::Attached,
    };

    SessionRow { session, name, state, container, grew: Grew::Unseen, asking }
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
            Look::Line { text, .. } | Look::Fold { text, .. } | Look::Divider { text } => text.clone(),
            Look::Turn { tag, mode, text, .. } => format!("{tag} {} {text}", mode.mark()),
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
fn a_compaction_is_a_divider_with_its_tokens() {
    // Catches a compaction left out of the transcript or shown without its counts, and Claude Code's status moving
    // past compacting shown as a line of its own.
    let mut chats = opened();
    let boundary = r#"{"type":"system","subtype":"compact_boundary","compact_metadata":{"trigger":"manual","pre_tokens":52000,"post_tokens":3100}}"#;
    chats.shown(
        1,
        &[
            claude(1, r#"{"type":"system","subtype":"status","status":"compacting"}"#),
            claude(2, boundary),
            claude(3, r#"{"type":"system","subtype":"status","status":null}"#),
        ],
    );

    assert_eq!(texts(&chats.rows()), ["· compacting", "compacted · 52k → 3k tokens"]);
}

#[test]
fn a_turn_that_begins_with_no_line_says_so_and_one_begun_by_a_line_does_not() {
    // Catches a turn a background subagent started shown as though the owner's line began it, and every turn marked as
    // begun by itself.
    let mut chats = opened();
    let init = r#"{"type":"system","subtype":"init"}"#;
    let done = r#"{"type":"result","subtype":"success","is_error":false}"#;
    chats.shown(1, &[said(1, 4, "go"), claude(2, init), claude(3, done), claude(4, init), claude(5, done)]);

    assert_eq!(texts(&chats.rows()), ["mellow-snipe > go", "· done", "· a turn began by itself", "· done"]);
}

#[test]
fn the_engine_learns_its_id_from_its_own_line_echoed_and_dims_every_other_engine_s() {
    // Catches an engine that inks every line as its own, or none; one that takes another engine's line of other text
    // as its own; lines shown before the echo left inked as others' once it is learned; and its own lines tagged with
    // its name rather than `local`, or another engine's with `local`.
    let mut chats = opened();
    chats.shown(1, &[said(1, 12, "from another engine")]);
    let sent = chats.say(String::from("what do you see?"));

    assert_eq!(sent, [Chat::Say { session: 1, text: String::from("what do you see?") }]);

    chats.shown(1, &[said(2, 12, "something else"), said(3, 37, "what do you see?")]);
    let tones: Vec<Tone> = chats
        .rows()
        .iter()
        .map(|row| match row.look {
            Look::Line { tone, .. } | Look::Turn { tone, .. } => tone,
            Look::Reply { .. } | Look::Fold { .. } | Look::Divider { .. } => Tone::Faint,
        })
        .collect();

    assert_eq!(tones, [Tone::Other, Tone::Other, Tone::Own]);
    assert_eq!(
        texts(&chats.rows()),
        ["wary-owl > from another engine", "wary-owl > something else", "local > what do you see?"]
    );
}

#[test]
fn a_chat_line_goes_to_claude_whole_and_a_bang_line_runs_here_the_chat_s_words_among_them() {
    // Catches a `/` line in chat mode run here or cut short rather than passed whole to Claude Code, a line in `!` mode
    // said to Claude, the chat's own words given a way of their own, and a blank line sent anywhere.
    assert_eq!(typed("what do you see?", Mode::Chat), Typed::Say(String::from("what do you see?")));
    assert_eq!(typed("/compact", Mode::Chat), Typed::Say(String::from("/compact")));
    assert_eq!(typed("/clear", Mode::Chat), Typed::Say(String::from("/clear")));
    assert_eq!(typed(" capture_frame --window 1 ", Mode::Bang), Typed::Run(String::from("capture_frame --window 1")));
    assert_eq!(typed("new_session work", Mode::Bang), Typed::Run(String::from("new_session work")));
    assert_eq!(typed("clear", Mode::Bang), Typed::Run(String::from("clear")));
    assert_eq!(typed("   ", Mode::Chat), Typed::Blank);
    assert_eq!(typed("", Mode::Bang), Typed::Blank);
}

#[test]
fn each_session_s_tab_keeps_its_own_mode_and_the_console_s_own_is_always_in_bang_mode() {
    // Catches one tab's `!` mode carried to another, a tab's mode lost as it is closed and opened again, and the
    // console's own tab put in chat mode, so its lines would be said to no session.
    let mut chats = Chats::default();
    let _ = chats.listed(vec![tab(1, State::Idle, 0), tab(2, State::Idle, 0)]);

    assert_eq!(chats.mode(), Mode::Chat);

    chats.switch(Mode::Bang);
    let _ = chats.choose(1);

    assert_eq!(chats.mode(), Mode::Chat);

    let _ = chats.choose(0);
    chats.switch(Mode::Chat);

    assert_eq!(chats.mode(), Mode::Bang);

    let _ = chats.choose(2);

    assert_eq!(chats.mode(), Mode::Bang);
}

#[test]
fn a_command_run_in_bang_mode_is_marked_so_with_what_it_printed_under_it_and_a_clear_is_a_divider() {
    // Catches a command run shown with chat mode's mark, so it reads as a line said to Claude, what it printed lost,
    // the turn it starts marked as begun by itself, and Claude Code's fresh conversation after `/clear` shown as
    // nothing.
    let mut chats = opened();
    let ran = Entry::Ran { engine: 12, line: String::from("memory"), printed: String::from("4 MiB") };
    chats.shown(
        1,
        &[
            Numbered { number: 1, entry: ran },
            claude(2, r#"{"type":"system","subtype":"init"}"#),
            said(3, 12, "/clear"),
            claude(4, r#"{"type":"conversation_reset","trigger":"clear"}"#),
        ],
    );

    assert_eq!(texts(&chats.rows()), ["wary-owl ! memory", "4 MiB", "wary-owl > /clear", "cleared"]);
}

#[test]
fn an_owner_s_line_is_a_turn_and_a_line_the_hub_or_a_caller_wrote_is_not() {
    // Catches Claude's lines, or the hub's and its callers', set apart as the owner's turns, and an owner's line at
    // another engine not set apart.
    let mut chats = opened();
    let written = Numbered { number: 2, entry: Entry::Said { by: Speaker::Hub, text: String::from("winding down") } };
    chats.shown(1, &[said(1, 12, "at another engine"), written, claude(3, REPLY)]);
    let turns: Vec<bool> = chats.rows().iter().map(|row| row.is_turn()).collect();

    assert_eq!(turns, [true, false, false]);
}

#[test]
fn a_word_names_the_open_session_or_one_by_its_name_and_says_why_none_is() {
    // Catches a word that interrupts, hibernates, wakes, or archives a session other than the one named, one that
    // guesses among sessions of one name, and one from the console's own tab that names none but acts.
    let mut chats = Chats::default();
    let _ = chats.listed(vec![tab(1, State::Idle, 0), tab(2, State::Idle, 0), tab(3, State::Idle, 0)]);
    let _ =
        chats.listed(vec![tab(1, State::Idle, 0), tab(2, State::Idle, 0), row(3, String::from("s2"), State::Idle, 0)]);

    assert_eq!(chats.interrupt(&Which::Open), Ok(Chat::Interrupt { session: 3 }));
    assert_eq!(chats.hibernate(&Which::Named(String::from("s1"))), Ok(Chat::Hibernate { session: 1 }));
    assert_eq!(chats.wake(&Which::Open), Ok(Chat::Wake { session: 3 }));
    assert_eq!(chats.archive(&Which::Named(String::from("s1"))), Ok(Chat::Archive { session: 1 }));
    assert!(chats.archive(&Which::Named(String::from("s2"))).is_err(), "two sessions are named s2");
    assert!(chats.interrupt(&Which::Named(String::from("s9"))).is_err(), "none is named s9");

    let _ = chats.choose(0);

    assert!(chats.interrupt(&Which::Open).is_err(), "the console's own tab is no session");
}

#[test]
fn a_session_made_here_opens_once_the_hub_lists_it_and_not_one_of_its_name_listed_before() {
    // Catches a session made here left closed once it lists, and an older session of the same name opened in its
    // place.
    let mut chats = Chats::default();
    let named = |session: u32| row(session, String::from("work"), State::Idle, 0);
    let _ = chats.listed(vec![named(1), tab(2, State::Idle, 0)]);
    let _ = chats.choose(0);

    assert_eq!(chats.make(String::from("work")), Chat::New { name: String::from("work") });
    assert_eq!(chats.listed(vec![named(1), tab(2, State::Idle, 0)]), []);
    assert_eq!(chats.listed(vec![named(1), tab(2, State::Idle, 0), named(3)]), [Chat::Open { session: 3, after: 0 }]);
    assert_eq!(chats.listed(vec![named(1), tab(2, State::Idle, 0), named(3), named(4)]), []);
}

#[test]
fn the_next_and_the_tab_before_go_round_from_the_last_to_the_first() {
    // Catches a step past the last tab, or back before the console's own, that opens nothing or a tab that is not
    // there.
    let mut chats = Chats::default();
    let _ = chats.listed(vec![tab(1, State::Idle, 0), tab(2, State::Idle, 0)]);

    assert_eq!(chats.step(false), [Chat::Close { session: 2 }]);
    assert_eq!(chats.step(true), [Chat::Open { session: 2, after: 0 }]);
    assert_eq!(chats.step(true), [Chat::Close { session: 2 }, Chat::Open { session: 1, after: 0 }]);
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

    assert_eq!(texts(&chats.rows()), ["mellow-snipe > a", "mellow-snipe > b", "mellow-snipe > c"]);
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

#[test]
fn a_session_s_status_is_its_ask_first_then_its_container_then_its_state() {
    // Catches an ask hidden behind a running turn or a compaction, a container starting or lost, or a start that
    // failed, hidden behind the conversation's state, so a failed start reads as an ordinary hibernation; a compaction
    // the hub lists shown as anything else; and a session the hub wound down, or is archiving, shown lost.
    let status = Status::of;
    let (none, attached, lost) = (&Connection::None, &Connection::Attached, &Connection::Lost);
    let unstarted = &Connection::Unstarted { why: String::from("the start script failed"), holding: 2 };

    assert_eq!(status(State::Compacting, attached, 1), Status::Waiting);
    assert_eq!(status(State::Hibernating, lost, 2), Status::Waiting);
    assert_eq!(status(State::Compacting, attached, 0), Status::Compacting);
    assert_eq!(status(State::Idle, &Connection::Starting, 0), Status::Starting);
    assert_eq!(status(State::Hibernating, lost, 0), Status::Lost);
    assert_eq!(status(State::Hibernating, unstarted, 0), Status::Unstarted);
    assert_eq!(status(State::Waiting, attached, 0), Status::Working);
    assert_eq!(status(State::Idle, attached, 0), Status::Idle);
    assert_eq!(status(State::Hibernating, none, 0), Status::Hibernating);
    assert_eq!(status(State::Archived, attached, 0), Status::Hibernating);
}

#[test]
fn a_tab_is_marked_lost_as_the_hub_lists_its_container_and_the_console_s_own_is_unmarked() {
    // Catches a tab marked by anything but the hub's row of its container, so a session the hub lists as lost shows
    // merely hibernating, and the console's own tab given a status.
    let mut chats = Chats::default();
    let _ = chats.listed(vec![SessionRow { container: Connection::Lost, ..tab(1, State::Hibernating, 0) }]);

    assert_eq!(chats.titles()[0].icon, Icon::Unmarked);
    assert_eq!(chats.titles()[1].icon, Icon::Marked(Status::Lost));
}

#[test]
fn an_aside_is_shown_after_the_entry_held_as_it_was_printed_and_a_decline_names_the_session_listed() {
    // Catches a frame the hub declined printed at the transcript's top or bottom, wherever entries come after it, so
    // it reads as an answer to something else; one whose key repeats an entry's, so the console draws one row for both;
    // and a decline that names a session by its number though the hub lists its name.
    let mut chats = opened();
    chats.shown(1, &[claude(1, REPLY)]);
    let why = Refusal::Working;
    let declined = chats.declined(&Chat::Hibernate { session: 1 }, &why);
    chats.aside(declined);
    chats.shown(1, &[claude(2, REPLY)]);

    let rows = chats.rows();
    assert_eq!(
        texts(&rows),
        [
            "<reply>",
            "· the hub could not hibernate `s1`: a turn or a background task runs in the session: interrupt it first",
            "<reply>",
        ]
    );
    let keys: Vec<u64> = rows.iter().map(|row| row.key).collect();
    assert!(keys.windows(2).all(|pair| pair[0] < pair[1]), "each row's key is its own, in order: {keys:?}");
}
