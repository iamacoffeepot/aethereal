//! The overlay run as a program runs it, in an engine the test steps (ADR-0036): a window, the renderer on a device that
//! draws nothing, the tools root, and the overlay, each of its keys delivered through the keys its screen declared, as
//! the window delivers them. What it did is read from its state.

use super::Overlay;
use super::stats::{Columns, MOST_COLUMNS};
use super::view::OverlayNode;
use crate::chat::Open;
use crate::colour::{Palette, Theme};
use crate::console::Shade;
use crate::layout::Scale;
use crate::meters::{MOST_METER_COLUMNS, Meter};
use crate::stop::Stop;
use crate::tools::{ChooseTheme, NewSession, OverlayTools};
use aethereal_actor::{Answered, Prepare, Render, Tick};
use aethereal_data::{Budget, Json, Plain, to_bytes};
use aethereal_render::shape::ColourFormat;
use aethereal_render::stats::FrameBudget;
use aethereal_render::surface::{Front, WindowFormat};
use aethereal_render::{Gpu, Renderer, RendererConfig};
use aethereal_sessions::{Entry, Numbered, ShowEvents, ShowTabs, Speaker, State, Tab};
use aethereal_substrate::harness::{Bench, Harness, Unheard};
use aethereal_substrate::{
    Bootstrap, Bootstrapping, Builds, Engine, EngineSettings, Host, ProgramError, Settled, Unbuilt,
};
use aethereal_tools::{Call, Called, Caller, Tool, Tools};
use aethereal_ui::widgets::Scroll;
use aethereal_ui::{ItemKey, Stand};
use aethereal_window::{InitialWindows, KeyEvent, Keyed, Mode, Opening, Surface, WindowFeed, WindowId, Windows};
use core::any::Any;
use core::time::Duration;

/// The window the overlay draws on, as big as one at 720.
const SURFACE: Surface = Surface { width: 1280, height: 720, scale: 1.0 };

/// A cycle of the program's time: a frame at 60 Hz.
const CYCLE: Duration = Duration::from_nanos(16_666_667);

/// A host of the test's own: it keeps the engine its program bootstrapped, and what the roots' bootstraps kept on it,
/// and runs nothing.
struct Desk {
    engine: Engine,
    kept: Kept,
}

/// What the roots' bootstraps kept on the desk, so the chassis's ends of the window and the renderer stay open while the
/// program runs: the window's feed, which names the window, and the rest, none of which is read.
#[derive(Default)]
struct Kept {
    windows: Vec<WindowFeed>,
    ends: Vec<Box<dyn Any>>,
}

/// The desk's own section, which sets nothing.
#[derive(Plain, Debug, Default)]
#[kind(name = "test.overlay.running")]
#[settings]
struct DeskRunning {}

impl Host for Desk {
    type Running = DeskRunning;
    type Hosting<'host> = &'host mut Kept;
    type Exit = Self;
    type Error = ProgramError;

    fn run(settled: Settled<Self>) -> Result<Self, ProgramError> {
        Self::build(settled)
    }
}

impl Builds for Desk {
    fn build(settled: Settled<Self>) -> Result<Self, ProgramError> {
        let mut kept = Kept::default();
        let (engine, _) = pollster::block_on(settled.bootstrap(&mut kept))?;

        Ok(Self { engine, kept })
    }
}

/// Opens one window as big as [`SURFACE`], begins the root with it, and provides it with the actor the root made of it.
impl Bootstrap<Desk> for Windows {
    const SECTION: &'static str = "windows";
    type Settings = ();
    type Needs = ();
    type Provides = (InitialWindows,);

    async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, Desk, Self>) -> Result<(InitialWindows,), Unbuilt> {
        let mut opening = Opening::new(cx.source(), cx.controller());
        let window = opening.open("overlay", SURFACE, Mode::Windowed, true);
        let (config, hosting) = opening.config();
        cx.begin(&(), config)?;

        let windows = InitialWindows::found(cx, window.window(), &[])?;
        cx.host().windows.push(window);
        cx.host().ends.push(Box::new(hosting));

        Ok((windows,))
    }
}

/// The renderer on a device that draws nothing, drawing the window the window root opened.
impl Bootstrap<Desk> for Renderer {
    const SECTION: &'static str = "renderer";
    type Settings = ();
    type Needs = (InitialWindows,);
    type Provides = ();

    async fn bootstrap(
        (): (),
        (windows,): (InitialWindows,),
        cx: &mut Bootstrapping<'_, Desk, Self>,
    ) -> Result<(), Unbuilt> {
        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let gpu = cx.hand(Gpu::new(device, queue));
        let (front, fronting) = Front::new(FrameBudget::UNTOLD);
        let front = cx.hand(front);
        let (presenting, presents) = cx.controller();
        let (mapping, mapping_requests) = cx.controller();
        let (mapped, mapped_feed) = cx.source();
        let (surfaced, surfaced_feed) = cx.source();
        let drawn = vec![WindowFormat { window: windows.first().id(), format: ColourFormat::Bgra8 }];
        let renderer =
            RendererConfig { gpu, front, presenting, windows: drawn, mapping, mapped, surfaced, roots: Vec::new() };
        cx.begin(&(), renderer)?;

        let ends: [Box<dyn Any>; 5] = [
            Box::new(fronting),
            Box::new(presents),
            Box::new(mapping_requests),
            Box::new(mapped_feed),
            Box::new(surfaced_feed),
        ];
        cx.host().ends.extend(ends);

        Ok(())
    }
}

/// The program: a window as big as [`SURFACE`], the renderer on a device that draws nothing, drawing that window, the
/// tools root, and the overlay on the window, in a cycle of `Tick`, `Prepare`, and `Render`, bootstrapped as a host
/// bootstraps it, so the overlay is handed the window as a program hands it. Answers the window, and what the desk kept,
/// which the test holds while the program runs.
fn program() -> Result<(Harness<Bench>, WindowId, Kept), Unheard> {
    let settings = EngineSettings { read_limit: 1 << 24, mail_bytes: 0, blob_bytes: 1 << 28, ..Default::default() };
    let Desk { engine, kept } = Desk::program()
        .with_cycle::<(Tick, Prepare, Render)>()
        .with_engine(settings)
        .root::<Windows>()
        .root::<Renderer>()
        .root::<Tools>()
        .root::<Overlay>()
        .build()
        .map_err(|error| Unheard::Program(error.to_string()))?;
    let Some(on) = kept.windows.first().map(WindowFeed::window) else {
        return Err(Unheard::Program(String::from("the window root opened no window")));
    };

    Ok((Harness::bare(engine)?, on, kept))
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
    let (mut harness, on, _kept) = program()?;
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
    assert!(rows.iter().any(|row| row == "local ! memory"), "the line is echoed: {rows:?}");
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
    let state = harness.state(overlay)?.ui.table().state::<Scroll>(&OverlayNode::Scrollback.path());

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
    let (mut harness, on, _kept) = program()?;
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
fn with_a_session_listed_a_line_goes_to_it_and_in_bang_mode_runs_here_and_escape_keeps_the_console()
-> Result<(), Unheard> {
    // Catches a console that shows no chat once the hub lists the sessions; a line typed in chat mode run as one of the
    // engine's tools; `!` on an empty line typed into it rather than putting the tab in `!` mode, or a mode that does
    // not show as the prompt's mark or does not last past a line; a line in `!` mode not run here; a transcript that
    // does not build in the console's view; Escape that closes the console rather than emptying the line, leaving
    // `!` mode, or stopping the turn; and Backspace, which the overlay takes first, that no longer deletes, or that
    // leaves `!` mode while the line holds text.
    let (mut harness, on, _kept) = program()?;
    run(&mut harness, 3);
    for _ in 0..2 {
        key(&mut harness, on, down("Backquote", "`"))?;
        key(&mut harness, on, typed("`"))?;
        run(&mut harness, 3);
    }
    let overlay = harness.root::<Overlay>();
    let console_sign = harness.state(overlay)?.sign();
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
    let chat_sign = harness.state(overlay)?.sign();

    key(&mut harness, on, typed("hello"))?;
    key(&mut harness, on, down("Enter", "Enter"))?;
    key(&mut harness, on, typed("!"))?;
    let bang_line = String::from(harness.state(overlay)?.console.line());
    let bang_sign = harness.state(overlay)?.sign();
    key(&mut harness, on, typed("memory"))?;
    key(&mut harness, on, down("Enter", "Enter"))?;
    run(&mut harness, 3);
    let kept_sign = harness.state(overlay)?.sign();
    key(&mut harness, on, typed("half"))?;
    key(&mut harness, on, down("Escape", "Escape"))?;
    let emptied = String::from(harness.state(overlay)?.console.line());
    let emptied_sign = harness.state(overlay)?.sign();
    key(&mut harness, on, down("Escape", "Escape"))?;
    let left_sign = harness.state(overlay)?.sign();
    key(&mut harness, on, down("Escape", "Escape"))?;
    run(&mut harness, 3);

    let state = harness.state(overlay)?;
    let rows: Vec<String> = state.console.rows().map(super::super::console::Row::text).collect();
    assert_eq!((console_sign, chat_sign, bang_sign, kept_sign), ("!", ">", "!", "!"));
    assert_eq!(bang_line, "", "the `!` that puts the tab in `!` mode is not typed");
    assert_eq!(state.chats.open(), Open::Session(1));
    assert_eq!(state.chats.rows().len(), 2, "the line and the reply are rows");
    assert!(!rows.iter().any(|row| row.ends_with(" hello")), "a line said is not run here: {rows:?}");
    assert!(rows.iter().any(|row| row == "local ! memory"), "a line in `!` mode runs here: {rows:?}");
    assert_eq!((emptied.as_str(), emptied_sign), ("", "!"), "Escape empties the line typed, and keeps the mode");
    assert_eq!(left_sign, ">", "Escape on an empty line leaves `!` mode");
    assert_eq!(state.stop, Stop::Console, "Escape keeps the console open with a session's tab open");

    key(&mut harness, on, down("KeyN", "n"))?;
    key(&mut harness, on, typed("!"))?;
    key(&mut harness, on, typed("ab"))?;
    key(&mut harness, on, down("Backspace", "Backspace"))?;
    let erased = String::from(harness.state(overlay)?.console.line());
    let erased_sign = harness.state(overlay)?.sign();
    key(&mut harness, on, down("Backspace", "Backspace"))?;
    key(&mut harness, on, down("Backspace", "Backspace"))?;
    let back_sign = harness.state(overlay)?.sign();

    assert_eq!((erased.as_str(), erased_sign), ("a", "!"), "Backspace deletes while the line holds text");
    assert_eq!(back_sign, ">", "Backspace on an empty line leaves `!` mode");

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
    let (mut harness, on, _kept) = program()?;
    run(&mut harness, 3);
    for _ in 0..2 {
        key(&mut harness, on, down("Backquote", "`"))?;
        key(&mut harness, on, typed("`"))?;
        run(&mut harness, 3);
    }
    let overlay = harness.root::<Overlay>();
    let unknown = |harness: &mut Harness<Bench>| -> Result<usize, Unheard> {
        let rows =
            harness.state(overlay)?.console.rows().filter(|row| row.text().contains("no tool is named `new_session`"));
        Ok(rows.count())
    };

    entered(&mut harness, on, "new_session work")?;
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
    entered(&mut harness, on, "new_session work")?;
    let trusted = unknown(&mut harness)?;
    let made = Tab { session: 2, name: String::from("work"), state: State::Starting, asking: 0 };
    harness.tell(overlay, &ShowTabs { tabs: vec![old, made] })?;
    run(&mut harness, 3);

    assert_eq!(untrusted, 1, "an engine the hub shows no chat has no `new_session`");
    assert_eq!(chosen, Open::Console, "Alt-1 opens the console's own tab");
    assert_eq!(line, "", "Alt-1 types nothing into the line");
    assert_eq!(trusted, 1, "once the hub lists the sessions, `new_session` is a tool the console's own tab runs");
    assert_eq!(harness.state(overlay)?.chats.open(), Open::Session(2), "the session made opens as it lists");

    Ok(())
}

#[test]
fn a_long_line_wraps_at_the_scrollback_s_width_and_the_charts_fill_the_boxes_laid_out() -> Result<(), Unheard> {
    // Catches a scrollback row drawn on one line and cut at the console's edge, or laid out wider than the scrollback,
    // now that the library wraps it rather than the console cutting it into rows of so many characters; and charts cut
    // into another count of columns than the boxes the library last laid them out in hold, as a path that names
    // another node than the frame chart or a meter's graph, or counts never read from the layout, would cut them.
    let (mut harness, on, _kept) = program()?;
    run(&mut harness, 3);
    for _ in 0..2 {
        key(&mut harness, on, down("Backquote", "`"))?;
        key(&mut harness, on, typed("`"))?;
        run(&mut harness, 3);
    }
    let long = "a long line of words ".repeat(30);
    entered(&mut harness, on, long.trim_end())?;

    let overlay = harness.root::<Overlay>();
    let state = harness.state(overlay)?;
    let echo = state.console.rows().find(|row| row.shade == Shade::Raised).map(|row| row.number);
    let Some(echo) = echo else {
        panic!("the line entered is echoed");
    };
    let laid = |named: OverlayNode| state.ui.laid(&named.path()).expect("laid out");
    let row = laid(OverlayNode::Printed(ItemKey::from(echo)));
    let (view, line) = (laid(OverlayNode::Scrollback), laid(OverlayNode::Sign));
    let (chart, meter) =
        (laid(OverlayNode::Chart), laid(OverlayNode::MeterGraph(ItemKey::from(Meter::ALL[0].index()))));
    let across =
        |box_laid: aethereal_ui::Laid, most| Scale(box_laid.scale).columns_across(box_laid.content.width(), most);

    assert!(row.rect.height() >= 2.0 * line.content.height(), "the line wraps onto several: {row:?}");
    assert!(row.rect.width() <= view.content.width(), "the row stands within the scrollback: {row:?} {view:?}");
    let expected = Columns { chart: across(chart, MOST_COLUMNS), meters: across(meter, MOST_METER_COLUMNS) };
    assert_eq!(state.columns, expected);
    assert!(expected.chart > expected.meters, "the frame chart is the band's widest graph: {expected:?}");

    Ok(())
}

/// Calls the overlay's tool `tool` of its tools' worker as `caller`, with `input`, and answers whether it answered a
/// value or why it refused.
fn called<K: Plain>(
    harness: &mut Harness<Bench>,
    caller: Caller,
    tool: &str,
    input: &K,
) -> Result<Result<(), String>, Unheard> {
    let overlay = harness.root::<Overlay>();
    let worker = OverlayTools::worker(harness.state(overlay)?.tools);
    let input = harness.put(to_bytes(input)?)?;
    let asked = harness.ask(worker, &Call { tool: String::from(tool), input, caller })?;
    let answered = match harness.answer(asked)? {
        Answered::Yes(Called::Output(_)) => Ok(()),
        Answered::Yes(Called::Refused(why)) => Err(why),
        Answered::Yes(Called::Image { .. }) | Answered::No(_) => Err(String::from("not an answer the worker gives")),
    };

    Ok(answered)
}

#[test]
fn the_worker_asks_the_overlay_for_a_session_s_theme_and_refuses_it_the_chat_s_words() -> Result<(), Unheard> {
    // Catches a theme the worker asks of the overlay never answered to its caller, or never switching the palette; a
    // `set_theme` refused to a session, though it changes only how the overlay looks; and a chat's word a session calls
    // asked of the overlay, where the sessions are the owner's.
    let (mut harness, _, _kept) = program()?;
    run(&mut harness, 3);
    let overlay = harness.root::<Overlay>();
    let tab = Tab { session: 1, name: String::from("work"), state: State::Idle, asking: 0 };
    harness.tell(overlay, &ShowTabs { tabs: vec![tab] })?;
    run(&mut harness, 3);
    let session = Caller::Session { session: 1 };

    let themed = called(&mut harness, session, "set_theme", &ChooseTheme::naming(Theme::Stone))?;
    let palette = harness.state(overlay)?.palette;
    let worded = called(&mut harness, session, "new_session", &NewSession { name: String::from("more") })?;

    assert_eq!(themed, Ok(()));
    assert_eq!(palette, Palette::new(Theme::Stone));
    assert_eq!(worded, Err(String::from("the chat's words are the owner's, not session 1's")));

    Ok(())
}
