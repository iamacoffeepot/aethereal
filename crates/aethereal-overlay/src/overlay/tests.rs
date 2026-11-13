//! The overlay run as a program runs it, in an engine the test steps (ADR-0036): a window, the renderer on a device that
//! draws nothing, the tools root, and the overlay, each of its keys delivered through the keys its screen declared, as
//! the window delivers them. What it did is read from its state.

use super::{Overlay, OverlayConfig};
use crate::stop::Stop;
use aethereal_actor::{Prepare, Render, Requests, Tick};
use aethereal_render::mapping::{Mapped, Mapping};
use aethereal_render::shape::ColourFormat;
use aethereal_render::stats::FrameBudget;
use aethereal_render::surface::{Front, Fronting, Present, Surfaced, WindowFormat};
use aethereal_render::{Gpu, Renderer, RendererConfig};
use aethereal_substrate::harness::{Bench, Harness, Unheard};
use aethereal_substrate::{Engine, Feed};
use aethereal_tools::{Tools, ToolsSettings};
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
