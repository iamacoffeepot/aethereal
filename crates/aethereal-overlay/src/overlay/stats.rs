//! What the overlay keeps as the program runs (ADR-0053): its window's size, which its screen lays out to; each
//! `Tick` while it is shown, the cycle's time, the marks told since the last, the figures and the engine's memory when
//! they are due, the engine's recent log about once a second, and the charts cut to as many columns as the window holds;
//! and each frame's statistics and the GPU's time over its passes. Nothing here draws: the band and the console show
//! what is kept at the screen's next `Prepare`.

use super::view::prompt_path;
use super::{FIGURES_EVERY_NANOS, LOGS_EVERY_NANOS, Overlay};
use crate::console::Logged;
use crate::gpu::{GpuFigures, GpuTiming};
use crate::layout::Layout;
use crate::meters::{EngineMemory, MOST_METER_COLUMNS};
use aethereal_actor::{Fault, Handles, Level, On, Tick, Turn};
use aethereal_render::stats::{FrameDrawing, FrameStats, GpuTimes};
use aethereal_ui::Viewport;
use aethereal_window::Event;

/// The most columns the charts of the band's left column are cut into, whatever the window's width.
pub const MOST_COLUMNS: u32 = 3_000;

/// A resize lays the overlay's screen out again at the window's new size; the overlay hears nothing else of its window
/// raw.
impl Handles<Event> for Overlay {
    type Sends = aethereal_ui::Sends;

    fn handle(&mut self, event: Event, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Event::Resized(surface) = event else {
            return Ok(());
        };
        // A screen's scale is a small number that an f32 holds closely enough to lay out by.
        #[allow(clippy::cast_possible_truncation)]
        let viewport = Viewport::window(surface.width, surface.height, surface.scale as f32);
        self.ui.resize(viewport).send(turn)
    }
}

impl Overlay {
    /// Cuts the charts and the meters' graphs into as many columns as the window holds at its size now.
    fn cut(&mut self) {
        let layout = Layout::of(self.ui.viewport());
        let count = layout.chart_columns(MOST_COLUMNS);
        self.frames.cut(count);
        if let GpuTiming::Timed(times) = &mut self.gpu {
            times.cut(count);
        }
        self.lines.cut(count);
        self.histories.cut(layout.meter_columns(MOST_METER_COLUMNS));
    }

    /// Reads what the engine logged since it was last read: each entry but a debug line printed into the
    /// scrollback, and while the console is closed each warning and error counted for the band.
    fn read_logs<S>(&mut self, turn: &Turn<'_, Self, S>) {
        let recent = turn.recent(self.logs_since);
        let (next, left) = (recent.next, recent.left);
        let logged: Vec<Logged> = recent
            .entries
            .iter()
            .map(|named| Logged::of(named.recorded.step(), &named.actor, named.recorded.entry()))
            .collect();
        self.logs_since = next;

        let columns = self.columns();
        let open = self.stop.console();
        if left > 0 {
            self.console.missed(left, columns);
        }
        for logged in &logged {
            if !open {
                self.alerts.count(logged.level);
            }
            let debug = logged.level == Level::Debug;
            if !debug {
                self.console.logged(logged, columns);
            }
        }
    }
}

/// Each cycle's time is kept, the marks told since the last stamped with it and printed, the charts cut, the figures,
/// the GPU's figures, and the engine's memory for the meters worked out again when they are due and the memory kept in
/// the meters' histories, and the engine's log read when it is due. While the console is open its line is given its
/// screen's focus back, should a press have taken it elsewhere, so the console goes on taking every key. Heard only
/// while the overlay is shown.
impl Handles<On<Tick>> for Overlay {
    type Sends = ();

    fn handle(&mut self, tick: Tick, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.frames.tick(&tick);
        self.marks.keep_since(tick.elapsed_nanos);
        let stamped = self.marks.stamp(tick.elapsed_nanos);
        if !stamped.is_empty() {
            // Nanoseconds of the program's time, which an f64 holds closely enough to show in tenths of a second.
            #[allow(clippy::cast_precision_loss)]
            let at_seconds = tick.elapsed_nanos as f64 / 1e9;
            let columns = self.columns();
            for text in &stamped {
                self.console.marked(text, at_seconds, columns);
            }
        }
        self.cut();

        let figures_due = tick.elapsed_nanos >= self.figures_due_nanos;
        if figures_due {
            self.figures_due_nanos = tick.elapsed_nanos.saturating_add(FIGURES_EVERY_NANOS);
            self.figures = self.frames.figures();
            self.gpu_figures = match &self.gpu {
                GpuTiming::Timed(times) => times.figures(self.figures.median_nanos()),
                GpuTiming::Unsupported => GpuFigures::default(),
            };
            self.memory = EngineMemory::of(&turn.engine_stats());
            self.histories.read(tick.elapsed_nanos, &self.memory);
        }

        let logs_due = tick.elapsed_nanos >= self.logs_due_nanos;
        if logs_due {
            self.logs_due_nanos = tick.elapsed_nanos.saturating_add(LOGS_EVERY_NANOS);
            self.read_logs(turn);
        }

        if self.stop.console() {
            self.ui.focus(&prompt_path());
        }

        Ok(())
    }
}

/// The budget each frame's statistics bring, which the chart is drawn against; whether its cycle drew, which keeps it
/// in the chart or leaves it out; the values of a frame drawn for the lines' rows; and its GPU memory for the GPU
/// meter. Heard only while the overlay is shown.
impl Handles<FrameStats> for Overlay {
    type Sends = ();

    fn handle(&mut self, stats: FrameStats, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.budget = stats.budget;
        self.frames.framed(&stats);
        if let FrameDrawing::Drawn(drawn) = stats.drawn {
            self.lines.push(stats.elapsed_nanos, drawn);
        }
        self.histories.framed(&stats);

        Ok(())
    }
}

/// The GPU's time over a frame's passes, kept for its graph; or that the device has none, which the graph's place
/// says. Heard only while the overlay is shown.
impl Handles<GpuTimes> for Overlay {
    type Sends = ();

    fn handle(&mut self, told: GpuTimes, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.gpu.heard(told);

        Ok(())
    }
}
