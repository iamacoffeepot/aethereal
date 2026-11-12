//! How the overlay draws once it has made its pieces (ADR-0031, ADR-0052): the keys its list declares step it and
//! edit the console, its window's size lays it out again, the wheel over the console's panel scrolls it (ADR-0055), and each `Tick` while it is shown keeps the cycle's time, writes the placement's records again in one
//! write, writes the figures' texts when what they show changes, and reads the engine's recent log about once a second.
//! While it is shown it also hears each frame's statistics, for the budget the chart is drawn against, the lines'
//! rows, and the GPU meter, and the GPU's time over each frame's passes, for its graph (ADR-0053).

use super::making::{ChartAsked, TextAsked};
use super::texts::TextRole;
use super::{Calling, Chart, FIGURES_EVERY_NANOS, LOGS_EVERY_NANOS, Overlay};
use crate::chart::{self, RECORD_BYTES};
use crate::console::Logged;
use crate::gpu::{self, GpuFigures, GpuTiming};
use crate::layout::Layout;
use crate::lines;
use crate::meters::{self, EngineMemory, Meter};
use crate::scrollbar;
use crate::stop::Stop;
use aethereal_actor::{Allows, Asks, Fault, Handles, Level, On, Path, Tells, Tick, Turn, path};
use aethereal_render::Renderer;
use aethereal_render::draw_list::{DrawList, Entry, Pointer};
use aethereal_render::frame::{Frame, ShapeOf};
use aethereal_render::instances::WriteInstances;
use aethereal_render::pass::Interface;
use aethereal_render::placement::PlacementId;
use aethereal_render::stats::{
    FrameDrawing, FrameStats, GpuTimes, SubscribeFrameStats, SubscribeGpuTimes, UnsubscribeFrameStats,
    UnsubscribeGpuTimes,
};
use aethereal_render::text::{CreateText, MoveText, WriteText};
use aethereal_render::uniforms::WriteUniforms;
use aethereal_tools::{CallTool, Tools};
use aethereal_window::{Cursor, Event, KeyEvent, KeySet, Keyed, Pointed, PointerEvent, Surface, WindowId};

/// The key that steps the overlay, by its place on the keyboard: the backtick, left of the 1 (ADR-0053).
const STEP_KEY: &str = "Backquote";

/// What the overlay sends as it hears its window and its keys: its list as it steps, its records as they change, the
/// surface's size and its texts' places as the window is resized, the first asks for what it draws, the frame
/// statistics and the GPU's times it hears while shown, the console's texts as keys change them, and the tools the
/// console's line calls.
pub(super) type Heard = (
    Tells<Path<Renderer>, DrawList>,
    Tells<Path<Renderer>, WriteInstances>,
    Tells<Path<Renderer>, WriteUniforms>,
    Tells<Path<Renderer>, MoveText>,
    Asks<Path<Renderer>, ShapeOf, ChartAsked>,
    Asks<Path<Renderer>, CreateText, TextAsked>,
    Tells<Path<Renderer>, SubscribeFrameStats>,
    Tells<Path<Renderer>, UnsubscribeFrameStats>,
    Tells<Path<Renderer>, SubscribeGpuTimes>,
    Tells<Path<Renderer>, UnsubscribeGpuTimes>,
    Tells<Path<Renderer>, WriteText>,
    Asks<Path<Tools>, CallTool, Calling>,
);

/// A resize lays the overlay out again; the overlay hears nothing else of its window raw.
impl Handles<Event> for Overlay {
    type Sends = Heard;

    fn handle(&mut self, event: Event, turn: &mut Turn<'_, Self, Heard>) -> Result<(), Fault> {
        match event {
            Event::Resized(surface) => self.resized(surface, turn),
            _ => Ok(()),
        }
    }
}

/// A backtick steps the overlay, and at the console's stop the keys and typed text edit and enter its line: the keys
/// its list declares, the backtick alone while the console is closed, and every key while it is open.
impl Handles<Keyed> for Overlay {
    type Sends = Heard;

    fn handle(&mut self, keyed: Keyed, turn: &mut Turn<'_, Self, Heard>) -> Result<(), Fault> {
        let console = self.stop.console();
        match keyed.event {
            KeyEvent::KeyDown { code, repeat: false, .. } if code == STEP_KEY => self.step(turn),
            KeyEvent::KeyDown { key, .. } if console => self.pressed(&key, turn),
            KeyEvent::Text { text } if console => {
                self.console.typed(&text);
                self.write_console(turn)
            }
            _ => Ok(()),
        }
    }
}

/// The wheel over the console's panel, which blocks the pointer from what is beneath it, scrolls its scrollback; the
/// overlay hears nothing else of the pointer.
impl Handles<Pointed> for Overlay {
    type Sends = Heard;

    fn handle(&mut self, pointed: Pointed, turn: &mut Turn<'_, Self, Heard>) -> Result<(), Fault> {
        let console = self.stop.console();
        match pointed.event {
            PointerEvent::Wheel { delta_y, .. } if console => {
                let (notch_pixels, view) = (Layout::of(self.surface).notch_pixels(), self.view());
                self.console.wheeled(delta_y, notch_pixels, view);
                self.write_console(turn)
            }
            _ => Ok(()),
        }
    }
}

impl Overlay {
    /// Takes in the window's new surface: the first makes what the overlay draws, sized by it, and each lays out
    /// again what is made.
    fn resized(&mut self, surface: Surface, turn: &mut Turn<'_, Self, Heard>) -> Result<(), Fault> {
        self.surface = surface;
        self.make(turn)?;
        self.write_surface(turn)?;
        for role in TextRole::ALL {
            self.move_text(role, turn)?;
        }
        self.write_console(turn)?;
        if self.stop.draws() {
            self.write_chart(turn)?;
        }
        if self.stop.console() {
            self.send_list(turn)?;
        }

        Ok(())
    }

    /// Steps to the next stop: shown, it hears each `Tick`, each frame's statistics, and the GPU's times, and draws its
    /// list, written for the stop at once; hidden, it hears neither and draws nothing. Opening the console takes every
    /// key, as its list declares, and clears the count of warnings and errors, which the console shows; closing it
    /// leaves the backtick alone. The text the backtick typed, which comes next, is dropped.
    fn step(&mut self, turn: &mut Turn<'_, Self, Heard>) -> Result<(), Fault> {
        let was = self.stop;
        self.stop = was.next();
        self.console.stepped();
        match (was.draws(), self.stop.draws()) {
            (false, true) => {
                turn.subscribe::<Tick>();
                turn.to(path::<Renderer>(())).send(&SubscribeFrameStats)?;
                turn.to(path::<Renderer>(())).send(&SubscribeGpuTimes)?;
                self.figures_due_nanos = 0;
                self.logs_due_nanos = 0;
            }
            (true, false) => {
                turn.unsubscribe::<Tick>();
                turn.to(path::<Renderer>(())).send(&UnsubscribeFrameStats)?;
                turn.to(path::<Renderer>(())).send(&UnsubscribeGpuTimes)?;
            }
            (false, false) | (true, true) => {}
        }
        let opened = !was.console() && self.stop.console();
        if opened {
            self.alerts.clear();
            self.write_text(TextRole::Alerts, turn)?;
            self.write_console(turn)?;
        }
        if self.stop.draws() {
            self.write_chart(turn)?;
        }

        self.send_list(turn)
    }

    /// Closes the console back to the band, its list leaving the backtick alone.
    pub(super) fn close(&mut self, turn: &mut Turn<'_, Self, Heard>) -> Result<(), Fault> {
        self.stop = Stop::Band;
        self.write_chart(turn)?;

        self.send_list(turn)
    }

    /// Sends the overlay's list, as [`listing`] lays it out for the stop, with what it has placed, each joined listed
    /// to the window's interface pass, the panels and chart first and the texts over them, the console's only at its
    /// stop, while it is shown. The overlay is the last root the program's root list names, so its list is drawn over
    /// the program's, and its keys are declared above the program's (ADR-0031, ADR-0055).
    pub(super) fn send_list<S, I>(&self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<Tells<Path<Renderer>, DrawList>, I>,
    {
        let mut placements = Vec::new();
        if self.stop.draws() {
            if let Chart::Made { placement, .. } = self.chart {
                placements.push(placement);
            }
            let console = self.stop.console();
            let roles = if console {
                &TextRole::ALL[..]
            } else {
                &TextRole::ALL[..TextRole::BAND]
            };
            placements.extend(roles.iter().filter_map(|role| self.texts.placement(*role)));
        }
        let entries = listing(self.window, self.stop, self.surface, placements);
        turn.to(path::<Renderer>(())).send(&DrawList { entries })?;

        Ok(())
    }

    /// Writes every record of the placement for the stop, the layout, the cycles kept, the GPU's passes and the
    /// lines' frames kept, the marks, and the cursor and scroll bar at the console's stop, in one write into a buffer the overlay keeps, and cuts the
    /// records to as many when there are fewer than were last written. Nothing while the placement is not made.
    pub(super) fn write_chart<S, I>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<Tells<Path<Renderer>, WriteInstances>, I>,
    {
        let Chart::Made { instances, written, .. } = &mut self.chart else {
            return Ok(());
        };
        let layout = Layout::of(self.surface);
        let count = chart::column_count(layout);
        let mut records = turn.buffer(chart::records_bytes(layout));
        let slide = self.frames.slide(count);
        chart::records(&mut records, layout, self.stop, self.frames.columns(count), slide, self.budget);
        if let GpuTiming::Timed(times) = &mut self.gpu {
            self.gpu_top = gpu::graph(&mut records, layout, slide, self.budget, times.columns(count));
        }
        lines::rows(&mut records, layout, slide, &self.lines.columns(count));
        let frames = &self.frames;
        meters::rows(&mut records, layout, &self.memory, &mut self.histories, |count| frames.slide(count));
        chart::marks(&mut records, layout, slide, self.marks.times());
        if self.stop.console() {
            chart::cursor(&mut records, layout, self.console.cursor_column());
            let (row_pixels, shown) = (layout.line_pixels_tall(), layout.scrollback_rows());
            let bar = scrollbar::bar(layout.scrollbar(), row_pixels, shown, self.console.scrolled());
            chart::scrollbar(&mut records, bar);
        }
        let count = u32::try_from(records.len() / RECORD_BYTES as usize).unwrap_or(u32::MAX);
        let Ok(records) = turn.put(records) else {
            turn.log(Level::Warn, "the overlay's chart was not written: its records were not put");
            return Ok(());
        };
        turn.to(path::<Renderer>(())).send(&WriteInstances { instances: *instances, first: 0, records })?;

        if count < *written {
            let Ok(none) = turn.put(Vec::new()) else {
                turn.log(Level::Warn, "the overlay's chart was not cut: its empty write was not put");
                return Ok(());
            };
            turn.to(path::<Renderer>(())).send(&WriteInstances {
                instances: *instances,
                first: count,
                records: none,
            })?;
        }
        *written = count;

        Ok(())
    }

    /// Writes the surface's size into the placement's uniform, which turns its pixels into clip space. Nothing while
    /// the placement is not made.
    pub(super) fn write_surface<S, I>(&self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<Tells<Path<Renderer>, WriteUniforms>, I>,
    {
        let Chart::Made { uniforms, .. } = self.chart else {
            return Ok(());
        };
        let Ok(bytes) = turn.put(chart::surface(Layout::of(self.surface))) else {
            turn.log(Level::Warn, "the overlay's surface size was not written: its bytes were not put");
            return Ok(());
        };
        turn.to(path::<Renderer>(())).send(&WriteUniforms { uniforms, first: 0, bytes })?;

        Ok(())
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

        let view = self.view();
        let open = self.stop.console();
        if left > 0 {
            self.console.missed(left, view);
        }
        for logged in &logged {
            if !open {
                self.alerts.count(logged.level);
            }
            let debug = logged.level == Level::Debug;
            if !debug {
                self.console.logged(logged, view);
            }
        }
    }
}

/// The overlay's list on `window` at `stop`, its surface `surface`, drawing `placements`: first the keys it hears in
/// the window, which block every key beneath them, the backtick alone, or every key while the console is open; then,
/// at the console's stop, a hit over the console's panel that blocks the pointer, so the wheel over it scrolls the
/// console and reaches nothing beneath; then each placement, which the pointer passes through.
pub(super) fn listing(window: WindowId, stop: Stop, surface: Surface, placements: Vec<PlacementId>) -> Vec<Entry> {
    let frame = Frame::Main(window);
    let console = stop.console();
    let keys = if console {
        KeySet::All
    } else {
        KeySet::of([STEP_KEY])
    };
    let mut entries = vec![Entry::Keys { frame, keys, blocks: true }];
    if console {
        entries.push(Entry::hit::<Interface>(frame, Layout::of(surface).console(), Pointer::Blocks, Cursor::Default));
    }
    entries.extend(placements.into_iter().map(|placement| Entry::Draw {
        placement,
        pointer: Pointer::Ignores,
        cursor: Cursor::Default,
    }));

    entries
}

/// Each cycle's time is kept, the marks told since the last stamped with it, the chart written again, the figures,
/// the GPU's figures, and the engine's memory for the meters worked out again when they are due and the memory kept in
/// the meters' histories, and their texts and the rows' values written only when what they show changes, and the
/// engine's log read when it is due. Heard only while the overlay is shown.
impl Handles<On<Tick>> for Overlay {
    type Sends =
        (Tells<Path<Renderer>, WriteInstances>, Tells<Path<Renderer>, WriteText>, Tells<Path<Renderer>, MoveText>);

    fn handle(&mut self, tick: Tick, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.frames.tick(&tick);
        self.marks.keep_since(tick.elapsed_nanos);
        let stamped = self.marks.stamp(tick.elapsed_nanos);
        self.write_chart(turn)?;
        self.write_text(TextRole::GpuTop, turn)?;
        if !stamped.is_empty() {
            // Nanoseconds of the program's time, which an f64 holds closely enough to show in tenths of a second.
            #[allow(clippy::cast_precision_loss)]
            let at_seconds = tick.elapsed_nanos as f64 / 1e9;
            let view = self.view();
            for text in &stamped {
                self.console.marked(text, at_seconds, view);
            }
            self.write_console(turn)?;
        }

        let figures_due = tick.elapsed_nanos >= self.figures_due_nanos;
        if figures_due {
            self.figures_due_nanos = tick.elapsed_nanos.saturating_add(FIGURES_EVERY_NANOS);
            self.figures = self.frames.figures();
            self.gpu_figures = match &self.gpu {
                GpuTiming::Timed(times) => times.figures(self.figures.median_nanos()),
                GpuTiming::Unsupported => GpuFigures::default(),
            };
            self.write_text(TextRole::Figures, turn)?;
            self.write_text(TextRole::FrameMedian, turn)?;
            self.write_text(TextRole::GpuFigures, turn)?;
            self.write_text(TextRole::Lines, turn)?;
            self.memory = EngineMemory::of(&turn.engine_stats());
            self.histories.read(tick.elapsed_nanos, &self.memory);
            for meter in Meter::ALL {
                self.write_text(TextRole::Meter(meter), turn)?;
            }
        }

        let logs_due = tick.elapsed_nanos >= self.logs_due_nanos;
        if logs_due {
            self.logs_due_nanos = tick.elapsed_nanos.saturating_add(LOGS_EVERY_NANOS);
            self.read_logs(turn);
            self.write_text(TextRole::Alerts, turn)?;
            self.write_console(turn)?;
        }

        Ok(())
    }
}

/// The budget each frame's statistics bring, which the chart is drawn against from the next `Tick`; whether its cycle
/// drew, which keeps it in the chart or leaves it out; the values of a frame drawn for the lines' rows; and its GPU
/// memory for the GPU meter. Heard only while the overlay is shown.
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

/// The GPU's time over a frame's passes, kept for its graph from the next `Tick`; or that the device has none, which
/// the graph's place says at once. Heard only while the overlay is shown.
impl Handles<GpuTimes> for Overlay {
    type Sends = (Tells<Path<Renderer>, WriteText>, Tells<Path<Renderer>, MoveText>);

    fn handle(&mut self, told: GpuTimes, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let unsupported = matches!(told, GpuTimes::Unsupported);
        self.gpu.heard(told);
        if unsupported {
            self.write_and_move(TextRole::GpuFigures, turn)?;
        }

        Ok(())
    }
}
