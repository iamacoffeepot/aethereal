//! The overlay run as a program runs it, in an engine the test steps (ADR-0036): a window, the renderer on a device that
//! draws nothing, the tools root, and the overlay, each of its keys delivered through the keys its screen declared, as
//! the window delivers them. What it did is read from its state.

use super::{Overlay, OverlayConfig};
use crate::chat::Open;
use crate::stop::Stop;
use aethereal_actor::{Prepare, Render, Requests, Tick};
use aethereal_data::{Budget, Json};
use aethereal_render::mapping::{Mapped, Mapping};
use aethereal_render::shape::ColourFormat;
use aethereal_render::stats::FrameBudget;
use aethereal_render::surface::{Front, Fronting, Present, Surfaced, WindowFormat};
use aethereal_render::{Gpu, Renderer, RendererConfig};
use aethereal_sessions::{Entry, Numbered, ShowEvents, ShowTabs, Speaker, State, Tab};
use aethereal_substrate::harness::{Bench, Harness, Unheard};
use aethereal_substrate::{Engine, Feed};
use aethereal_tools::{Tools, ToolsSettings};
use aethereal_ui::Stand;
use aethereal_ui::widgets::Scroll;
use aethereal_window::{Hosting, KeyEvent, Keyed, Mode, Opening, Surface, WindowFeed, WindowId, Windows};
use core::num::NonZeroUsize;
use core::time::Duration;

/// The window the overlay draws on, as big as one at 720.
const SURFACE: Surface = Surface { width: 1280, height: 720, scale: 1.0 };

/// A cycle of the program's time: a frame at 60 Hz.
const CYCLE: Duration = Duration::from_nanos(16_666_667);

/// What the program is run with that the test holds, so the chassis's ends of the window and the renderer stay open
/// while it runs: the window's feed, which names the window, and the rest, none of which is read.
struct Held {
    window: WindowFeed,
    _hosting: Hosting,
    _fronting: Fronting,
    _presents: Requests<Present>,
    _mapping: Requests<Mapping>,
    _mapped: Feed<Mapped>,
    _surfaced: Feed<Surfaced>,
}

/// The program: a window as big as [`SURFACE`], the renderer on a device that draws nothing, drawing that window, the
/// tools root, and the overlay on the window, in a cycle of `Tick`, `Prepare`, and `Render`.
fn program() -> Result<(Harness<Bench>, Held), Unheard> {
    let mut setup = Engine::new(NonZeroUsize::MIN, 1 << 24, 0, 1 << 28, 0).with_cycle::<(Tick, Prepare, Render)>();
    let mut opening = Opening::new(setup.source(), setup.controller());
    let window = opening.open("overlay", SURFACE, Mode::Windowed, true);
    let (windows, hosting) = opening.config();
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let gpu = setup.hand(Gpu::new(device, queue));
    let (front, fronting) = Front::new(FrameBudget::UNTOLD);
    let front = setup.hand(front);
    let (presenting, presents) = setup.controller();
    let (mapping, mapping_requests) = setup.controller();
    let (mapped, mapped_feed) = setup.source();
    let (surfaced, surfaced_feed) = setup.source();
    let on = window.window();
    let drawn = vec![WindowFormat { window: on, format: ColourFormat::Bgra8 }];
    let renderer =
        RendererConfig { gpu, front, presenting, windows: drawn, mapping, mapped, surfaced, roots: Vec::new() };
    let engine = setup
        .add_root::<Windows>(&(), windows)?
        .add_root::<Renderer>(&(), renderer)?
        .add_root::<Tools>(&(), ToolsSettings::default())?
        .add_root::<Overlay>(&(), OverlayConfig { window: on })?
        .ready();
    let held = Held {
        window,
        _hosting: hosting,
        _fronting: fronting,
        _presents: presents,
        _mapping: mapping_requests,
        _mapped: mapped_feed,
        _surfaced: surfaced_feed,
    };

    Ok((Harness::bare(engine)?, held))
}

/// Runs `cycles` cycles of the program, each settled before the next: what each `Prepare` laid out is made by the
/// renderer, and its list sent, before the next cycle.
fn run(harness: &mut Harness<Bench>, cycles: u32) {
    for _ in 0..cycles {
        harness.advance(CYCLE);
        harness.settle();
    }
}

/// Sends the overlay `event` on `window` through the first keys its screen declares, its own, as the window sends a key
/// that reaches them, and settles what it set going.
fn key(harness: &mut Harness<Bench>, window: WindowId, event: KeyEvent) -> Result<(), Unheard> {
    let overlay = harness.root::<Overlay>();
    harness.tell(overlay, &Keyed { window, entry: 0, event })?;
    harness.settle();

    Ok(())
}

/// The key at `code`, meaning `key`, pressed.
fn down(code: &str, key: &str) -> KeyEvent {
    KeyEvent::KeyDown { code: String::from(code), key: String::from(key), repeat: false }
}

/// The text `text`, typed.
fn typed(text: &str) -> KeyEvent {
    KeyEvent::Text { text: String::from(text) }
}

#[test]
fn a_line_typed_at_the_console_and_entered_calls_its_tool_and_prints_the_answer() -> Result<(), Unheard> {
    // Catches a backtick typed into the line it opens, or one that types into the line rather than closing it; a
    // console whose line is not focused as it opens, so what is typed reaches the program beneath; a line whose text
    // input's edits are not kept, or whose Enter runs nothing; a call never asked of the tools root; and an answer
    // never printed into the scrollback.
    let (mut harness, held) = program()?;
    let on = held.window.window();
    run(&mut harness, 3);
    key(&mut harness, on, down("Backquote", "`"))?;
    key(&mut harness, on, typed("`"))?;
    run(&mut harness, 3);
    key(&mut harness, on, down("Backquote", "`"))?;
    key(&mut harness, on, typed("`"))?;
    run(&mut harness, 3);

    key(&mut harness, on, down("KeyM", "m"))?;
    key(&mut harness, on, typed("memory"))?;
    let overlay = harness.root::<Overlay>();
    let typed_line = String::from(harness.state(overlay)?.console.line());
    key(&mut harness, on, down("Enter", "Enter"))?;
    run(&mut harness, 3);

    let state = harness.state(overlay)?;
    let rows: Vec<String> = state.console.rows().map(super::super::console::Row::text).collect();
    assert_eq!(state.stop, Stop::Console);
    assert_eq!(typed_line, "memory");
    assert_eq!(state.console.line(), "", "the line is left for the next");
    assert!(rows.iter().any(|row| row == "> memory"), "the line is echoed: {rows:?}");
    let answered = rows.iter().any(|row| row.starts_with("process: "));
    assert!(answered, "the answer is printed: {rows:?}");

    key(&mut harness, on, down("KeyM", "m"))?;
    key(&mut harness, on, typed("me"))?;
    key(&mut harness, on, down("Backquote", "`"))?;
    key(&mut harness, on, typed("`"))?;
    let closed = harness.state(overlay)?;
    assert_eq!(closed.stop, Stop::Hidden);
    assert_eq!(closed.console.line(), "me", "the backtick that closed the console was typed into its line");

    Ok(())
}

/// Where the console's scrollback stands, as its screen's table keeps it.
fn scrollback(harness: &mut Harness<Bench>) -> Result<Stand, Unheard> {
    let overlay = harness.root::<Overlay>();
    let state = harness.state(overlay)?.ui.table().state::<Scroll>(&super::view::scrollback_path());

    Ok(state.unwrap_or_default().stand)
}

/// Enters `memory` at the open console `times` times, each answer printed before the next.
fn printed(harness: &mut Harness<Bench>, on: WindowId, times: u32) -> Result<(), Unheard> {
    for _ in 0..times {
        key(harness, on, down("KeyM", "m"))?;
        key(harness, on, typed("memory"))?;
        key(harness, on, down("Enter", "Enter"))?;
        run(harness, 2);
    }

    Ok(())
}

#[test]
fn the_scrollback_keeps_to_its_end_as_rows_are_printed_and_pages_back_and_down_to_it() -> Result<(), Unheard> {
    // Catches a scrollback, a scroll that builds only the rows that can show, that stays where it stood as answers are
    // printed beneath it, so the newest never shows; Page Up handed from the line to it that does not slide it back;
    // and one paged back down to its end that does not keep to it again.
    let (mut harness, held) = program()?;
    let on = held.window.window();
    run(&mut harness, 3);
    for _ in 0..2 {
        key(&mut harness, on, down("Backquote", "`"))?;
        key(&mut harness, on, typed("`"))?;
        run(&mut harness, 3);
    }
    printed(&mut harness, on, 30)?;
    let kept_to_end = scrollback(&mut harness)?;

    key(&mut harness, on, down("PageUp", "PageUp"))?;
    run(&mut harness, 3);
    let paged = scrollback(&mut harness)?;
    for _ in 0..3 {
        key(&mut harness, on, down("PageDown", "PageDown"))?;
        run(&mut harness, 3);
    }
    let paged_down = scrollback(&mut harness)?;

    assert!(matches!(kept_to_end, Stand::End { .. }), "{kept_to_end:?}");
    assert!(matches!(paged, Stand::At(_)), "{paged:?}");
    assert!(matches!(paged_down, Stand::End { .. }), "paged down to its end, it keeps to it again: {paged_down:?}");

    Ok(())
}

#[test]
fn with_a_session_listed_a_line_goes_to_it_a_slash_line_runs_here_and_escape_keeps_the_console() -> Result<(), Unheard>
{
    // Catches a console that shows no chat once the hub lists the sessions, a line typed at a session's tab run as one
    // of the engine's tools, a `/` line not run here, a transcript that does not build in the console's view, and
    // Escape that closes the console rather than emptying the line or stopping the turn.
    let (mut harness, held) = program()?;
    let on = held.window.window();
    run(&mut harness, 3);
    for _ in 0..2 {
        key(&mut harness, on, down("Backquote", "`"))?;
        key(&mut harness, on, typed("`"))?;
        run(&mut harness, 3);
    }
    let overlay = harness.root::<Overlay>();
    let tab = Tab { session: 1, name: String::from("work"), state: State::Idle, asking: 0 };
    harness.tell(overlay, &ShowTabs { tabs: vec![tab] })?;
    let reply = Json::parse(
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"A **reply**."}]},"parent_tool_use_id":null}"#,
        &mut Budget::new(1 << 12),
    )
    .expect("the reply is JSON");
    let said = Entry::Said { by: Speaker::Engine { engine: 4 }, text: String::from("hi") };
    let events = vec![Numbered { number: 1, entry: said }, Numbered { number: 2, entry: Entry::Claude(reply) }];
    harness.tell(overlay, &ShowEvents { session: 1, events })?;
    run(&mut harness, 3);

    key(&mut harness, on, typed("hello"))?;
    key(&mut harness, on, down("Enter", "Enter"))?;
    key(&mut harness, on, typed("/memory"))?;
    key(&mut harness, on, down("Enter", "Enter"))?;
    run(&mut harness, 3);
    key(&mut harness, on, typed("half"))?;
    key(&mut harness, on, down("Escape", "Escape"))?;
    let emptied = String::from(harness.state(overlay)?.console.line());
    key(&mut harness, on, down("Escape", "Escape"))?;
    run(&mut harness, 3);

    let state = harness.state(overlay)?;
    let rows: Vec<String> = state.console.rows().map(super::super::console::Row::text).collect();
    assert_eq!(state.chats.open(), Open::Session(1));
    assert_eq!(state.chats.rows().len(), 2, "the line and the reply are rows");
    assert!(!rows.iter().any(|row| row == "> hello"), "a line said is not run here: {rows:?}");
    assert!(rows.iter().any(|row| row == "> memory"), "a `/` line runs here: {rows:?}");
    assert_eq!(emptied, "", "Escape empties the line typed");
    assert_eq!(state.stop, Stop::Console, "Escape keeps the console open with a session's tab open");

    Ok(())
}

/// The key at `code`, meaning `key`, let up.
fn up(code: &str, key: &str) -> KeyEvent {
    KeyEvent::KeyUp { code: String::from(code), key: String::from(key) }
}

/// Types `line` at the open console and enters it, its answer settled.
fn entered(harness: &mut Harness<Bench>, on: WindowId, line: &str) -> Result<(), Unheard> {
    key(harness, on, down("KeyN", "n"))?;
    key(harness, on, typed(line))?;
    key(harness, on, down("Enter", "Enter"))?;
    run(harness, 3);

    Ok(())
}

#[test]
fn the_chat_s_words_are_tools_once_the_hub_lists_the_sessions_and_a_session_made_opens_as_it_lists()
-> Result<(), Unheard> {
    // Catches the chat's words served by an engine the hub has not shown the chat; words that are not the engine's
    // tools, so the console's own tab cannot run them; Alt-1 that does not open the console's own tab, or types into
    // the line; and a session made at the console left closed once it lists.
    let (mut harness, held) = program()?;
    let on = held.window.window();
    run(&mut harness, 3);
    for _ in 0..2 {
        key(&mut harness, on, down("Backquote", "`"))?;
        key(&mut harness, on, typed("`"))?;
        run(&mut harness, 3);
    }
    let overlay = harness.root::<Overlay>();
    let unknown = |harness: &mut Harness<Bench>| -> Result<usize, Unheard> {
        let rows = harness.state(overlay)?.console.rows().filter(|row| row.text().contains("no tool is named `new`"));
        Ok(rows.count())
    };

    entered(&mut harness, on, "new work")?;
    let untrusted = unknown(&mut harness)?;
    let old = Tab { session: 1, name: String::from("old"), state: State::Idle, asking: 0 };
    harness.tell(overlay, &ShowTabs { tabs: vec![old.clone()] })?;
    run(&mut harness, 3);
    key(&mut harness, on, down("AltLeft", "Alt"))?;
    key(&mut harness, on, down("Digit1", "1"))?;
    key(&mut harness, on, typed("1"))?;
    key(&mut harness, on, up("AltLeft", "Alt"))?;
    let chosen = harness.state(overlay)?.chats.open();
    let line = String::from(harness.state(overlay)?.console.line());
    entered(&mut harness, on, "new work")?;
    let trusted = unknown(&mut harness)?;
    let made = Tab { session: 2, name: String::from("work"), state: State::Starting, asking: 0 };
    harness.tell(overlay, &ShowTabs { tabs: vec![old, made] })?;
    run(&mut harness, 3);

    assert_eq!(untrusted, 1, "an engine the hub shows no chat has no `new`");
    assert_eq!(chosen, Open::Console, "Alt-1 opens the console's own tab");
    assert_eq!(line, "", "Alt-1 types nothing into the line");
    assert_eq!(trusted, 1, "once the hub lists the sessions, `new` is a tool the console's own tab runs");
    assert_eq!(harness.state(overlay)?.chats.open(), Open::Session(2), "the session made opens as it lists");

    Ok(())
}
