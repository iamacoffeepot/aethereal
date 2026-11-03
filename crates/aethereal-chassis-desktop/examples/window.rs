//! A ring of triangles in a window, turning, following the pointer, and turning the other way while a key is held,
//! with the frames it was handed timed.
//!
//! Run with `cargo run --release -p aethereal-chassis-desktop --example window -- --runs for:3`. With `--runs for:N`
//! the program quits itself after N seconds, and prints the time between the frames it was handed, from one `Tick` to
//! the next, as `key=value` in milliseconds: their mean, median, 99th percentile, and longest. With `--runs forever`,
//! as without it, it runs until its window is closed or Escape is pressed. `--size 1280x720` sizes its window, and
//! `--mode fullscreen` or `--mode maximized` shows it so as it opens. Each key, button, and turn of the wheel it hears
//! is printed as it comes, and each resize and mode. `--help` lists every flag the program reads, each also read from
//! its `WINDOW_` variable.

#![allow(clippy::print_stdout, clippy::print_stderr)] // An example prints what it heard and how its frames were paced.

use aethereal_actor::{Render, Tick};
use aethereal_chassis_desktop::{Desktop, WindowsSettings};
use aethereal_chassis_headless::Exit;
use aethereal_chassis_headless::native::{Launched, NativeLayers};
use aethereal_render::Renderer;
use aethereal_substrate::{EngineSettings, Host};
use aethereal_window::Windows;
use core::error::Error;
use core::num::{NonZeroU32, NonZeroUsize};
use ring::Scene;
use std::process::ExitCode;
use std::thread;

#[path = "shared/ring.rs"]
mod ring;

/// The most workers the program runs on.
const MOST_WORKERS: NonZeroUsize = NonZeroUsize::new(4).expect("4 is not zero");

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

/// Launches the program of the window root, the renderer, which draws on the window it opened, and the scene, on as
/// many workers as the machine has cores, at most four, the engine's views in a part for each, with what its command
/// line and environment set over that.
fn run() -> Result<Launched<Exit>, Box<dyn Error>> {
    let workers = NonZeroU32::try_from(
        thread::available_parallelism().map_or(NonZeroUsize::MIN, |cores| cores.min(MOST_WORKERS)),
    )?;

    Ok(Desktop::program()
        .with_cycle::<(Tick, Render)>()
        .with_engine(EngineSettings { workers, parts: workers, ..Default::default() })
        .root::<Windows>()
        .root::<Renderer>()
        .root::<Scene>()
        .flags(|flags| {
            flags.flag("size", 's', WindowsSettings::FIRST_SIZE).flag("mode", 'm', WindowsSettings::FIRST_MODE)
        })
        .launch("window")?)
}
