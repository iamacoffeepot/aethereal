//! The overlay's actor (ADR-0053): a root on the program's first window, subscribed to it for its size and the
//! backtick. It makes what it draws once the window has said its size, and draws it in its own draw list in the
//! window's main pass while it is shown (ADR-0031).

mod drawing;
mod making;

pub use making::{
    ChartAsked, ChartInstanced, ChartMade, ChartMeshed, ChartProgrammed, ChartShaped, ReadoutAsked, ReadoutMade,
};

use crate::frames::{Figures, FrameTimes};
use crate::stop::Stop;
use aethereal_actor::{Actor, Answer, Beginning, ChildOf, Ending, Fault, On, Path, Tells, Tick, Top, path};
use aethereal_data::{Bound, Plain};
use aethereal_render::frame::Shaped;
use aethereal_render::instances::{Instanced, InstancesId};
use aethereal_render::mesh::Meshed;
use aethereal_render::placement::{Placed, PlacementId};
use aethereal_render::program::Programmed;
use aethereal_render::text::{TextId, TextMade, TextPlaced};
use aethereal_render::uniforms::{Uniformed, UniformsId};
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};
use aethereal_window::{self as window, Event, InitialWindows, Surface, Window, WindowId};

/// The frame time the chart measures each cycle against, 16.7 milliseconds: a constant until the renderer's frame
/// statistics bring the program's own budget, which follows its present mode and the monitor its window is on
/// (ADR-0053: Frame statistics). It is not a flag, which would be wrong the moment either changed (ADR-0053:
/// Alternatives considered).
pub const BUDGET_NANOS: u64 = 16_700_000;

/// How often the figures beneath the chart are worked out again while it is shown: four times a second. Their text
/// is written only when what it shows changes.
const FIGURES_EVERY_NANOS: u64 = 250_000_000;

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
/// cycle's time and draws the band over what the program drew. While it is hidden it draws nothing and hears no
/// `Tick`, only its window.
#[derive(Bound)]
#[kind(name = "aether.overlay")]
pub struct Overlay {
    window: WindowId,
    stop: Stop,
    /// The window's surface as it was last heard: the window sends it as the overlay subscribes, before anything else,
    /// and nothing is made or drawn before then.
    surface: Surface,
    frames: FrameTimes,
    chart: Chart,
    readout: Readout,
    /// The program's time from which the figures are next worked out.
    figures_due_nanos: u64,
}

/// What the overlay has made of its one placement: the panels, the chart, and the budget line.
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

/// What the overlay has made of the text of the chart's figures.
#[derive(Bound)]
enum Readout {
    /// Not asked for: the window has not said its size, which the text is sized by.
    Unasked,
    /// Its asks are under way.
    Making,
    /// Placed: the text, its placement, and the figures it shows.
    Made { text: TextId, placement: PlacementId, shown: Figures },
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
        Answer<Shaped, ChartAsked>,
        Answer<Meshed, ChartShaped>,
        Answer<Programmed, ChartMeshed>,
        Answer<Instanced, ChartProgrammed>,
        Answer<Uniformed, ChartInstanced>,
        Answer<Placed, ChartMade>,
        Answer<TextMade, ReadoutAsked>,
        Answer<TextPlaced, ReadoutMade>,
    );
    type BeginSends = (Tells<Path<Window>, window::Subscribe>,);
    type EndSends = ();

    fn begin(config: OverlayConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.to(path::<Window>(config.window)).send(&window::Subscribe)?;

        Ok(Self {
            window: config.window,
            stop: Stop::Hidden,
            surface: Surface { width: 1, height: 1, scale: 1.0 },
            frames: FrameTimes::default(),
            chart: Chart::Unasked,
            readout: Readout::Unasked,
            figures_due_nanos: 0,
        })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}
