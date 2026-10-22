//! The desktop as a program's host: its own section, the screen its roots bootstrap on, the ends it serves, and the
//! window root and the renderer bootstrapped on it (ADR-0029).

mod renderer;
mod windows;

pub use renderer::{Presenting, RendererSettings};
pub use windows::{WindowSettings, WindowsSettings};

use crate::gpu::Screen;
use crate::running::{Desktop, Failed};
use crate::serving::Drawing;
use aethereal_chassis_headless::Exit;
use aethereal_data::Plain;
use aethereal_substrate::{Host, ProgramError, Serves, Settled};
use aethereal_window::Hosting;

/// How the desktop runs a program: its section, `running` (ADR-0028).
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.desktop.running")]
#[settings]
pub struct RunningSettings {
    /// The milliseconds `Stop` is given to finish once the program is asked to stop.
    #[flag]
    pub stop_within_millis: u64,
}

impl Default for RunningSettings {
    fn default() -> Self {
        Self { stop_within_millis: 1000 }
    }
}

impl Host for Desktop {
    type Running = RunningSettings;
    type Hosting<'host> = Screen<'host>;
    type Exit = Result<Exit, Failed>;

    /// Runs the program on winit's event loop, on this thread, through its lifecycle, then drops it (ADR-0020). Once
    /// the loop begins it bootstraps the program on the [`Screen`], which opens each window the window root's settings
    /// list, and on which the renderer opens the GPU; this thread is then the first of the engine's workers, and
    /// `Start` is sent. Each frame the display frees it says a cycle is due, at the time the frames were acquired, and
    /// steps until no actor has anything to run: one cycle draws every window shown. Once the program is asked to
    /// stop, by its last window closing or an actor's [`quit`](aethereal_actor::Turn::quit), it waits at most
    /// `stop_within_millis` for `Stop` to finish.
    ///
    /// A program that does not bootstrap is answered its error. Inside it, [`Failed`] when the event loop does not
    /// run, the program serves no drawing or no window root's end, or the loop ends before the program does.
    ///
    /// # Panics
    ///
    /// Panics with a turn's panic, and when a turn makes an actor under an id the table already holds (ADR-0008).
    fn run(settled: Settled<Self>) -> Result<Result<Exit, Failed>, ProgramError> {
        Self::run_settled(settled)
    }
}

impl Serves<Hosting> for Desktop {
    fn serve(screen: &mut Screen<'_>, hosting: Hosting) {
        screen.serve_windowing(hosting);
    }
}

impl Serves<Drawing> for Desktop {
    fn serve(screen: &mut Screen<'_>, drawing: Drawing) {
        screen.serve_drawing(drawing);
    }
}
