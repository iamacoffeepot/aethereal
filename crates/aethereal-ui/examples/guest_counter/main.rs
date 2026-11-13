//! A screen of the widget library run as a guest, in an engine linked to a hub (ADR-0056, ADR-0054): the counter, a
//! count and a button that adds to it, is built for wasm32 into a module in `Code`, which the library publishes as the
//! program starts. The stage, a native root screen listed in the window's draw list, spawns the counter from its
//! kind's head and places it over the whole window. The engine hands the hub the library's `publish` and `upgrades`,
//! which publish a rebuilt module over the running counter and show that it moved, beside `capture_frame`, which shows
//! what the window draws (ADR-0043), `send_input`, which clicks the button (ADR-0046), and the logs.
//!
//! Build the counter, and put its module in `Code`, `code` unless `--code` says otherwise:
//!
//! ```text
//! cargo build --release --target wasm32-unknown-unknown -p aethereal-guest-counter
//! cp target/wasm32-unknown-unknown/release/aethereal_guest_counter.wasm code/counter.wasm
//! ```
//!
//! With a hub from the headless `link_hub` example running, ask its `mint_token` tool for a token, which answers the
//! arguments to start this engine with, then run `cargo run --release -p aethereal-ui --example guest_counter -- <those
//! arguments> --name counter`. To move it onto new code, change the counter, build it again, copy its module over
//! `code/counter.wasm`, and call `publish` with `counter.wasm`: the count is kept. `--size` sizes its window and
//! `--title` names it; `--help` lists every flag it reads, each also read from its `GUEST_COUNTER_` variable.

#![allow(clippy::print_stdout, clippy::print_stderr)] // An example prints how it ended.

mod stage;

use aethereal_actor::{Prepare, Render, Tick};
use aethereal_chassis_desktop::{Desktop, WindowSettings, WindowsSettings};
use aethereal_chassis_headless::Exit;
use aethereal_chassis_headless::guest::Runtime;
use aethereal_chassis_headless::native::{Launched, NativeLayers};
use aethereal_files::Files;
use aethereal_library::{Library, LibraryTool};
use aethereal_link::{Link, LinkSettings};
use aethereal_pace::{PaceTool, Pacer};
use aethereal_render::{CaptureTool, Renderer, TargetTool};
use aethereal_substrate::{EngineSettings, Host};
use aethereal_tcp::Tcp;
use aethereal_tools::{LogsTool, Toolbox, Tools};
use aethereal_window::{InputTool, Mode, WindowTool, Windows};
use core::error::Error;
use core::num::{NonZeroU32, NonZeroUsize};
use stage::Stage;
use std::process::ExitCode;
use std::sync::Arc;
use std::thread;

/// The most workers the program runs on.
const MOST_WORKERS: NonZeroUsize = NonZeroUsize::new(4).expect("4 is not zero");

/// The window's title unless `--title` names another.
const TITLE: &str = "Aethereal guest counter";

/// The most bytes of a frame on the link: room for an image of a large frame.
const MOST_LINK_BYTES: u32 = 16 << 20;

fn main() -> ExitCode {
    match run() {
        Ok(Launched::Ran(exit)) => {
            println!("exit={exit:?}");
            ExitCode::SUCCESS
        }
        Ok(Launched::Printed) => ExitCode::SUCCESS,
        Err(failed) => {
            eprintln!("{failed}");
            ExitCode::FAILURE
        }
    }
}

/// Launches the stage, which places the counter, as the window's draw list, with the library publishing `Code` after
/// `Files`, the link to the hub beside them, and a toolbox of the renderer's, the window's, the pacer's, the logs', and
/// the library's tools offered to the tools the link hands the hub, on as many workers as the machine has cores, at
/// most four, running guests, with what its command line and environment set over that.
fn run() -> Result<Launched<Exit>, Box<dyn Error>> {
    let workers = NonZeroU32::try_from(
        thread::available_parallelism().map_or(NonZeroUsize::MIN, |cores| cores.min(MOST_WORKERS)),
    )?;
    let link = LinkSettings { most_bytes: MOST_LINK_BYTES, ..LinkSettings::default() };
    let engine = EngineSettings {
        workers,
        parts: workers,
        read_limit: MOST_LINK_BYTES,
        mail_bytes: 1 << 24,
        ..Default::default()
    };
    let window = WindowSettings { title: String::from(TITLE), size: [480, 320], mode: Mode::Windowed };

    Ok(Desktop::program()
        .with_cycle::<(Tick, Prepare, Render)>()
        .with_engine(engine)
        .with_guests(Arc::new(Runtime::default()))
        .root_with::<Windows>(WindowsSettings(vec![window]))
        .root::<Renderer>()
        .root::<Pacer>()
        .root::<Files>()
        .root::<Library>()
        .root::<Stage>()
        .root::<Tools>()
        .root::<Tcp>()
        .root_with::<Link>(link)
        .root::<Toolbox<(CaptureTool, TargetTool, LogsTool, InputTool, WindowTool, PaceTool, LibraryTool), Tools>>()
        .draws::<(Stage,)>()
        .flags(|flags| {
            flags.flag("size", 's', WindowsSettings::FIRST_SIZE).flag(
                "title",
                't',
                WindowSettings::TITLE.in_element::<WindowsSettings>(0),
            )
        })
        .launch("guest_counter")?)
}
