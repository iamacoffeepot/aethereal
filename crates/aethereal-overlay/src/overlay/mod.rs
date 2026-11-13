//! The overlay's actor (ADR-0053): a root on the program's first window, and a screen of the widget library (ADR-0056),
//! which lays out and draws its band and console in its own draw list in the window's interface pass, declares the keys
//! it hears, and takes in what its list reaches. It subscribes to its window for its size, and to the engine's tools
//! root for the tools the console calls; as it begins it spawns the worker of its own tools and offers them.
//!
//! ```text
//! Tick, FrameStats, GpuTimes, the engine's figures and log ──> the band's and the console's state
//! Prepare ──> view (`view`): the band's primitives and the console's widgets ──> the library lays out and draws
//! the backtick, Escape, Tab, Up, Down, Page Up, Page Down ──> Controls::key;  the line's edit and submit ──> events
//! the line submitted ──> CallTool to the tools root ──> Called ──> printed into the scrollback
//! ```

mod console;
mod controls;
mod stats;
#[cfg(test)]
mod tests;
mod view;

pub use console::{Calling, Offering};

use crate::console::{Alerts, Console};
use crate::frames::{Figures, FrameTimes};
use crate::gpu::{GpuFigures, GpuTiming};
use crate::lines::FrameLines;
use crate::marks::Marks;
use crate::meters::{EngineMemory, MeterHistories};
use crate::stop::Stop;
use crate::tools::{Marked, OverlayTools, OverlayToolsConfig};
use aethereal_actor::{
    Actor, Answer, Asks, Beginning, ChildOf, Ending, Fault, Level, On, Path, Tells, Tick, Top, path,
};
use aethereal_data::{Bound, Plain};
use aethereal_render::Renderer;
use aethereal_render::frame::{Frame, ShapeOf};
use aethereal_render::pass::Interface;
use aethereal_render::stats::{FrameBudget, FrameStats, GpuTimes};
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};
use aethereal_tools::{Called, Offer, Offered, Sheets, Tools};
use aethereal_ui::{Screen, Shaping, Target, Viewport};
use aethereal_window::{self as window, Event, InitialWindows, Window, WindowId};

/// How often the figures beneath the chart, and the engine's memory for the meters, are worked out again while the
/// overlay is shown: four times a second. The library writes a text again only when what it shows changes.
const FIGURES_EVERY_NANOS: u64 = 250_000_000;

/// How often the engine's recent log is read while the overlay is shown: about once a second (ADR-0053).
const LOGS_EVERY_NANOS: u64 = 1_000_000_000;

/// The overlay draws on the first window the window root opened.
impl<H: Host> Bootstrap<H> for Overlay {
    const SECTION: &'static str = "overlay";
    type Settings = ();
    type Needs = (InitialWindows,);
    type Provides = ();

    async fn bootstrap(
        (): (),
        (windows,): (InitialWindows,),
        cx: &mut Bootstrapping<'_, H, Self>,
    ) -> Result<(), Unbuilt> {
        cx.begin(&(), OverlayConfig { window: windows.first() })
    }
}

/// The overlay's config: the window it draws on.
#[derive(Plain)]
#[kind(name = "aether.overlay.config")]
pub struct OverlayConfig {
    pub window: WindowId,
}

/// The dev overlay: it steps through its stops at each backtick its window hears, and while it is shown keeps each
/// cycle's time, each frame's statistics, and the GPU's time over each frame's passes, which its band shows against
/// the frame budget the renderer's statistics bring. At the console's stop its line takes every key of the window, and
/// its panel the pointer and the wheel over it, and its line calls the engine's tools. While it is hidden it draws
/// nothing and hears no `Tick` and no statistics, only its window, the backtick, its tools, its marks, and the library's
/// `Prepare`, whose tree is then the same each frame, so nothing is laid out or sent and the renderer counts nothing for
/// it.
#[derive(Bound)]
#[kind(name = "aether.overlay")]
pub struct Overlay {
    stop: Stop,
    /// What the widget library keeps of the overlay's screen: its size, its widgets' state, and what it drew.
    ui: Screen,
    frames: FrameTimes,
    /// The frame time the chart measures each cycle against, as the renderer's statistics last said: the program's own,
    /// which follows its present mode and the display its window is on (ADR-0053: Frame statistics). Until they first
    /// say, the fallback refresh, drawn as a reference.
    budget: FrameBudget,
    /// The GPU's time over each frame's passes as the renderer tells it, or that the device has no timestamps.
    gpu: GpuTiming,
    /// The figures beneath the GPU's graph, as they were last worked out.
    gpu_figures: GpuFigures,
    /// Each frame's draws, triangles, and bytes uploaded, as its statistics tell them.
    lines: FrameLines,
    /// The figures beneath the chart, as they were last worked out.
    figures: Figures,
    /// The program's time from which the figures are next worked out.
    figures_due_nanos: u64,
    console: Console,
    marks: Marks,
    /// The warnings and errors logged since the console was last opened.
    alerts: Alerts,
    /// The engine's memory as the band's meters last read it, four times a second while it is shown.
    memory: EngineMemory,
    /// The last minute of the meters with no limit, process, guests, and GPU, which their rows graph.
    histories: MeterHistories,
    /// The step the engine's recent log is next read from (ADR-0053: Logs in the console).
    logs_since: u64,
    /// The program's time from which the log is next read.
    logs_due_nanos: u64,
}

impl ChildOf for Overlay {
    type Parent = Top;
}

impl Actor for Overlay {
    type Config = OverlayConfig;
    type Key = ();
    type Handles = aethereal_ui::handles![
        Event,
        On<Tick>,
        FrameStats,
        GpuTimes,
        Sheets,
        Answer<Called, Calling>,
        Answer<Offered, Offering>,
        Marked,
    ];
    type BeginSends = (
        Tells<Path<Window>, window::Subscribe>,
        Asks<Path<Renderer>, ShapeOf, Shaping>,
        Tells<Path<Tools>, aethereal_tools::Subscribe>,
        Asks<Path<Tools>, Offer, Offering>,
    );
    type EndSends = ();

    /// Subscribes to its window and to the engine's tools, begins its screen in the window's interface pass, hidden,
    /// which hears the backtick, and spawns the worker of its own tools and offers them.
    fn begin(config: OverlayConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.to(path::<Window>(config.window)).send(&window::Subscribe)?;
        let target = Target::of::<Interface>(Frame::Main(config.window));
        let ui = Screen::begin(target, Viewport::window(1, 1, 1.0), turn)?;
        turn.to(path::<Tools>(())).send(&aethereal_tools::Subscribe)?;
        match turn.spawn::<OverlayTools>(&(), OverlayToolsConfig) {
            Ok(worker) => {
                let offer = Offer { tools: OverlayTools::sheets(), worker: OverlayTools::worker(worker) };
                turn.to(path::<Tools>(())).ask(&offer, &Offering)?;
            }
            Err(refused) => {
                turn.log(Level::Error, format_args!("the overlay's tools were not spawned: {refused:?}"));
            }
        }

        Ok(Self {
            stop: Stop::Hidden,
            ui,
            frames: FrameTimes::default(),
            budget: FrameBudget::UNTOLD,
            gpu: GpuTiming::default(),
            gpu_figures: GpuFigures::default(),
            lines: FrameLines::default(),
            figures: Figures::default(),
            figures_due_nanos: 0,
            console: Console::default(),
            marks: Marks::default(),
            alerts: Alerts::default(),
            memory: EngineMemory::Unread,
            histories: MeterHistories::default(),
            logs_since: 0,
            logs_due_nanos: 0,
        })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}
