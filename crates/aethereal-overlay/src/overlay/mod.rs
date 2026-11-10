//! The overlay's actor (ADR-0053): a root on the program's first window, subscribed to it for its size and its keys,
//! and to the engine's tools root for the tools the console calls. It makes what it draws once the window has said
//! its size, and draws it in its own draw list in the window's main pass while it is shown (ADR-0031). As it begins it
//! spawns the worker of its own tools and offers them.

mod console;
mod drawing;
mod making;
mod texts;

pub use console::{Calling, Offering};
pub use making::{
    ChartAsked, ChartInstanced, ChartMade, ChartMeshed, ChartProgrammed, ChartShaped, TextAsked, TextReady,
};

use crate::console::{Alerts, Console};
use crate::frames::{Figures, FrameTimes};
use crate::gpu::{GpuFigures, GpuTiming};
use crate::lines::FrameLines;
use crate::marks::Marks;
use crate::stop::Stop;
use crate::tools::{Marked, OverlayTools, OverlayToolsConfig};
use aethereal_actor::{
    Actor, Answer, Asks, Beginning, ChildOf, Ending, Fault, Level, On, Path, Tells, Tick, Top, path,
};
use aethereal_data::{Bound, Plain};
use aethereal_render::frame::Shaped;
use aethereal_render::instances::{Instanced, InstancesId};
use aethereal_render::mesh::Meshed;
use aethereal_render::placement::{Placed, PlacementId};
use aethereal_render::program::Programmed;
use aethereal_render::stats::{FrameBudget, FrameStats, GpuTimes};
use aethereal_render::text::{TextMade, TextPlaced};
use aethereal_render::uniforms::{Uniformed, UniformsId};
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};
use aethereal_tools::{Called, Offer, Offered, Sheets, Tools};
use aethereal_window::{
    self as window, Event, InitialWindows, KeyFocusGained, KeyFocusLost, Surface, Window, WindowId,
};
use texts::Texts;

/// How often the figures beneath the chart are worked out again while it is shown: four times a second. Their text
/// is written only when what it shows changes.
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
    window: WindowId,
}

/// The dev overlay: it steps through its stops at each backtick its window hears, and while it is shown keeps each
/// cycle's time, each frame's statistics, and the GPU's time over each frame's passes, and draws the band over what
/// the program drew, against the frame budget the renderer's statistics bring. At the console's stop it holds the window's key focus, and its line calls the engine's tools. While it is
/// hidden it draws nothing and hears no `Tick` and no statistics, only its window, its tools, and its marks, so the
/// renderer counts nothing for it.
#[derive(Bound)]
#[kind(name = "aether.overlay")]
pub struct Overlay {
    window: WindowId,
    stop: Stop,
    /// The window's surface as it was last heard: the window sends it as the overlay subscribes, before anything else,
    /// and nothing is made or drawn before then.
    surface: Surface,
    frames: FrameTimes,
    /// The frame time the chart measures each cycle against, as the renderer's statistics last said: the program's own,
    /// which follows its present mode and the display its window is on (ADR-0053: Frame statistics). Until they first
    /// say, the fallback refresh, drawn as a reference.
    budget: FrameBudget,
    /// The GPU's time over each frame's passes as the renderer tells it, or that the device has no timestamps.
    gpu: GpuTiming,
    /// The figures beneath the GPU's graph, as they were last worked out.
    gpu_figures: GpuFigures,
    /// Each frame's draws, triangles, bytes uploaded, and GPU memory, as its statistics tell them.
    lines: FrameLines,
    chart: Chart,
    /// The overlay's texts: the chart's figures, the band's count of warnings and errors, and the console's scrollback
    /// and prompt.
    texts: Texts,
    /// The figures beneath the chart, as they were last worked out.
    figures: Figures,
    /// The program's time from which the figures are next worked out.
    figures_due_nanos: u64,
    console: Console,
    marks: Marks,
    /// The warnings and errors logged since the console was last opened.
    alerts: Alerts,
    /// The step the engine's recent log is next read from (ADR-0053: Logs in the console).
    logs_since: u64,
    /// The program's time from which the log is next read.
    logs_due_nanos: u64,
}

/// What the overlay has made of its one placement: the panels, the chart, the marks' ticks, and the cursor.
#[derive(Bound)]
enum Chart {
    /// Not asked for: the window has not said its size.
    Unasked,
    /// Its asks are under way, one after another.
    Making,
    /// Placed: the records it is drawn from, how many of them were last written, the surface's size it reads, and the
    /// placement its list names.
    Made { instances: InstancesId, written: u32, uniforms: UniformsId, placement: PlacementId },
}

impl ChildOf for Overlay {
    type Parent = Top;
}

impl Actor for Overlay {
    type Config = OverlayConfig;
    type Key = ();
    type Handles = (
        Event,
        On<Tick>,
        FrameStats,
        GpuTimes,
        Answer<Shaped, ChartAsked>,
        Answer<Meshed, ChartShaped>,
        Answer<Programmed, ChartMeshed>,
        Answer<Instanced, ChartProgrammed>,
        Answer<Uniformed, ChartInstanced>,
        Answer<Placed, ChartMade>,
        Answer<TextMade, TextAsked>,
        Answer<TextPlaced, TextReady>,
        KeyFocusGained,
        KeyFocusLost,
        Sheets,
        Answer<Called, Calling>,
        Answer<Offered, Offering>,
        Marked,
    );
    type BeginSends = (
        Tells<Path<Window>, window::Subscribe>,
        Tells<Path<Tools>, aethereal_tools::Subscribe>,
        Asks<Path<Tools>, Offer, Offering>,
    );
    type EndSends = ();

    /// Subscribes to its window and to the engine's tools, and spawns the worker of its own tools and offers them.
    fn begin(config: OverlayConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.to(path::<Window>(config.window)).send(&window::Subscribe)?;
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
            window: config.window,
            stop: Stop::Hidden,
            surface: Surface { width: 1, height: 1, scale: 1.0 },
            frames: FrameTimes::default(),
            budget: FrameBudget::UNTOLD,
            gpu: GpuTiming::default(),
            gpu_figures: GpuFigures::default(),
            lines: FrameLines::default(),
            chart: Chart::Unasked,
            texts: Texts::default(),
            figures: Figures::default(),
            figures_due_nanos: 0,
            console: Console::default(),
            marks: Marks::default(),
            alerts: Alerts::default(),
            logs_since: 0,
            logs_due_nanos: 0,
        })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}
