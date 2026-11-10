//! The dev overlay over a backdrop, in an engine linked to a hub (ADR-0053): the backtick steps it hidden, band, band
//! and console, and hidden again. The backdrop is one triangle over the whole window, so the overlay's panels show
//! what is behind them. The engine hands the hub the tools the `linked_window` example hands it, among them
//! `capture_frame`, which shows the agent what the window draws (ADR-0043), and `send_input`, which presses the
//! backtick for it (ADR-0046).
//!
//! With a hub from the headless `link_hub` example running, ask its `mint_token` tool for a token, which answers the
//! arguments to start this engine with, then run `cargo run --release -p aethereal-overlay --example overlay -- <those
//! arguments> --name overlay`; or spawn it from the hub's store. A link's frames are at most 16 MiB here, so an image
//! of a large frame fits one. `--size 1920x1080` sizes its window, and `--mode` shows it so as it opens; `--help`
//! lists every flag it reads, each also read from its `OVERLAY_` variable.

#![allow(clippy::print_stdout, clippy::print_stderr)] // An example prints how it ended.

use aethereal_actor::{Render, Tick};
use aethereal_chassis_desktop::{Desktop, WindowsSettings};
use aethereal_chassis_headless::Exit;
use aethereal_chassis_headless::native::{Launched, NativeLayers};
use aethereal_files::Files;
use aethereal_link::{Link, LinkSettings};
use aethereal_overlay::Overlay;
use aethereal_pace::{PaceTool, Pacer};
use aethereal_render::{CaptureTool, RecordTool, Renderer, TargetTool};
use aethereal_substrate::{EngineSettings, Host};
use aethereal_tcp::Tcp;
use aethereal_tools::{LogsTool, Toolbox, Tools};
use aethereal_window::{InputTool, WindowTool, Windows};
use backdrop::Backdrop;
use core::error::Error;
use core::num::{NonZeroU32, NonZeroUsize};
use std::process::ExitCode;
use std::thread;

#[path = "shared/backdrop.rs"]
mod backdrop;

/// The most workers the program runs on.
const MOST_WORKERS: NonZeroUsize = NonZeroUsize::new(4).expect("4 is not zero");

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

/// Launches the backdrop with the overlay listed, the link to the hub beside them, and a toolbox of the renderer's,
/// the window's, the pacer's, and the logs' tools offered to the tools the link hands the hub, on as many workers as the machine has cores, at most
/// four, with what its command line and environment set over that.
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

    Ok(Desktop::program()
        .with_cycle::<(Tick, Render)>()
        .with_engine(engine)
        .root::<Windows>()
        .root::<Renderer>()
        .root::<Pacer>()
        .root::<Backdrop>()
        .root::<Overlay>()
        .root::<Files>()
        .root::<Tcp>()
        .root::<Tools>()
        .root_with::<Link>(link)
        .root::<Toolbox<(CaptureTool, RecordTool, TargetTool, LogsTool, InputTool, WindowTool, PaceTool), Tools>>()
        .flags(|flags| {
            flags.flag("size", 's', WindowsSettings::FIRST_SIZE).flag("mode", 'm', WindowsSettings::FIRST_MODE)
        })
        .launch("overlay")?)
}
