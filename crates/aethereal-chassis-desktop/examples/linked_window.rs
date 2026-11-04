//! The window example's ring, in an engine linked to a hub, handing it three tools: `capture_frame`, which shows the
//! agent what the window draws (ADR-0043); `record_clip`, which holds the program's clock through its pacer and writes
//! each frame of a clip into its saves (ADR-0045); and `actor_logs`, which reads what its actors logged (ADR-0044). It
//! dials the hub once, as it starts, with the token the hub minted for it, and ends when its link ends, or when its
//! window is closed or Escape is pressed (ADR-0040). Its saves are written under `--files-save`, `save` unless set.
//!
//! With a hub from the headless `link_hub` example running, ask its `mint_token` tool for a token, which answers the
//! arguments to start this engine with, then run `cargo run --release -p aethereal-chassis-desktop --example
//! linked_window -- <those arguments> --name window`. A link's frames are at most 16 MiB here, so an image of a large
//! frame fits one; start the hub with `--link-most-bytes 16777216 --set engine.read_limit=16777216` to read them. Its
//! other flags are the window example's (`--size`, `--mode`, `--runs`) and the link's; `--help` lists every flag it
//! reads, each also read from its `LINKED_WINDOW_` variable.
//!
//! On a Linux desktop reached over SSH, name its display: `WAYLAND_DISPLAY=wayland-0 XDG_RUNTIME_DIR=/run/user/$(id
//! -u)`.

#![allow(clippy::print_stdout, clippy::print_stderr)] // An example prints what it heard and how its frames were paced.

use aethereal_actor::{Render, Tick};
use aethereal_chassis_desktop::{Desktop, WindowsSettings};
use aethereal_chassis_headless::Exit;
use aethereal_chassis_headless::native::{Launched, NativeLayers};
use aethereal_files::Files;
use aethereal_link::{Link, LinkSettings};
use aethereal_pace::{PaceTool, Pacer};
use aethereal_render::{CaptureTool, RecordTool, Renderer};
use aethereal_substrate::{EngineSettings, Host};
use aethereal_tcp::Tcp;
use aethereal_tools::{LogsTool, Toolbox};
use aethereal_window::{InputTool, WindowTool, Windows};
use core::error::Error;
use core::num::{NonZeroU32, NonZeroUsize};
use ring::Scene;
use std::process::ExitCode;
use std::thread;

#[path = "shared/ring.rs"]
mod ring;

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

/// Launches the window example's program with the link to the hub beside it, and a toolbox holding `capture_frame` for
/// the link, on as many workers as the machine has cores, at most four, with what its command line and environment
/// set over that.
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
        .root::<Scene>()
        .root::<Files>()
        .root::<Tcp>()
        .root_with::<Link>(link)
        .root::<Toolbox<(CaptureTool, RecordTool, LogsTool, InputTool, WindowTool, PaceTool), Link>>()
        .flags(|flags| {
            flags.flag("size", 's', WindowsSettings::FIRST_SIZE).flag("mode", 'm', WindowsSettings::FIRST_MODE)
        })
        .launch("linked_window")?)
}
