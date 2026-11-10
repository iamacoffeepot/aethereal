//! How the overlay draws once it has made its pieces (ADR-0031, ADR-0052): what its window hears steps it and lays it
//! out again, and each `Tick` while it is shown keeps the cycle's time, writes the placement's records again in one
//! write, and writes the figures' text when what it shows changes.

use super::making::{ChartAsked, ReadoutAsked};
use super::{BUDGET_NANOS, Chart, FIGURES_EVERY_NANOS, Overlay, Readout};
use crate::chart::{self, RECORD_BYTES};
use crate::layout::Layout;
use aethereal_actor::{Allows, Asks, Fault, Handles, Level, On, Path, Tells, Tick, Turn, path};
use aethereal_render::Renderer;
use aethereal_render::draw_list::DrawList;
use aethereal_render::frame::{Frame, ShapeOf};
use aethereal_render::instances::WriteInstances;
use aethereal_render::pass::Main;
use aethereal_render::text::{Clip, CreateText, Layer, MoveText, WriteText};
use aethereal_render::uniforms::WriteUniforms;
use aethereal_window::{Event, Surface};

/// The key that steps the overlay, by its place on the keyboard: the backtick, left of the 1 (ADR-0053).
const STEP_KEY: &str = "Backquote";

/// What the overlay sends as it hears its window: its list as it steps, its records as they change, the surface's
/// size and the text's place as the window is resized, and the first asks for what it draws.
type Heard = (
    Tells<Path<Renderer>, DrawList>,
    Tells<Path<Renderer>, WriteInstances>,
    Tells<Path<Renderer>, WriteUniforms>,
    Tells<Path<Renderer>, MoveText>,
    Asks<Path<Renderer>, ShapeOf, ChartAsked>,
    Asks<Path<Renderer>, CreateText, ReadoutAsked>,
);

/// A backtick steps the overlay, and a resize lays it out again; the overlay hears nothing else of its window.
impl Handles<Event> for Overlay {
    type Sends = Heard;

    fn handle(&mut self, event: Event, turn: &mut Turn<'_, Self, Heard>) -> Result<(), Fault> {
        match event {
            Event::Resized(surface) => self.resized(surface, turn),
            Event::KeyDown { code, repeat: false, .. } if code == STEP_KEY => self.step(turn),
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
        self.move_readout(turn)?;
        if self.stop.draws() {
            self.write_chart(turn)?;
        }

        Ok(())
    }

    /// Steps to the next stop: shown, it hears each `Tick` and draws its list, written for the stop at once; hidden,
    /// it hears no `Tick` and draws an empty list.
    fn step(&mut self, turn: &mut Turn<'_, Self, Heard>) -> Result<(), Fault> {
        let was = self.stop;
        self.stop = was.next();
        match (was.draws(), self.stop.draws()) {
            (false, true) => {
                turn.subscribe::<Tick>();
                self.figures_due_nanos = 0;
            }
            (true, false) => turn.unsubscribe::<Tick>(),
            (false, false) | (true, true) => {}
        }
        if self.stop.draws() {
            self.write_chart(turn)?;
        }

        self.send_list(turn)
    }

    /// Sends the overlay's list for the window's main pass: what it has placed, the panels and chart first and the
    /// figures over them, while it is shown; nothing while it is hidden. A list is drawn after the pass's loose
    /// placements, so over what the program drew loose there (ADR-0031).
    pub(super) fn send_list<S, I>(&self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<Tells<Path<Renderer>, DrawList>, I>,
    {
        let mut placements = Vec::new();
        if self.stop.draws() {
            if let Chart::Made { placement, .. } = self.chart {
                placements.push(placement);
            }
            if let Readout::Made { placement, .. } = self.readout {
                placements.push(placement);
            }
        }
        turn.to(path::<Renderer>(())).send(&DrawList::of::<Main>(Frame::Main(self.window), placements))?;

        Ok(())
    }

    /// Writes every record of the placement for the stop, the layout, and the cycles kept, in one write into a buffer
    /// the overlay keeps, and cuts the records to as many when there are fewer than were last written. Nothing while
    /// the placement is not made.
    pub(super) fn write_chart<S, I>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<Tells<Path<Renderer>, WriteInstances>, I>,
    {
        let Chart::Made { instances, written, .. } = &mut self.chart else {
            return Ok(());
        };
        let layout = Layout::of(self.surface);
        let columns = self.frames.columns(chart::column_count(layout));
        let mut records = turn.buffer((columns.len() + 2) * RECORD_BYTES as usize);
        chart::records(&mut records, layout, self.stop, &columns, BUDGET_NANOS);
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

    /// Moves the figures' text to where the layout puts it. Nothing while it is not placed.
    pub(super) fn move_readout<S, I>(&self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<Tells<Path<Renderer>, MoveText>, I>,
    {
        let Readout::Made { placement, .. } = self.readout else {
            return Ok(());
        };
        let at = Layout::of(self.surface).figures_at();
        turn.to(path::<Renderer>(())).send(&MoveText {
            placement,
            at,
            layer: Layer::Over,
            opacity: 1.0,
            clip: Clip::None,
        })?;

        Ok(())
    }
}

/// Each cycle's time is kept, the chart written again, and the figures worked out again when they are due, their text
/// written only when what it shows changes. Heard only while the overlay is shown.
impl Handles<On<Tick>> for Overlay {
    type Sends = (Tells<Path<Renderer>, WriteInstances>, Tells<Path<Renderer>, WriteText>);

    fn handle(&mut self, tick: Tick, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.frames.push(&tick);
        self.write_chart(turn)?;

        let due = tick.elapsed_nanos >= self.figures_due_nanos;
        if !due {
            return Ok(());
        }
        self.figures_due_nanos = tick.elapsed_nanos.saturating_add(FIGURES_EVERY_NANOS);
        let figures = self.frames.figures();
        let Readout::Made { text, shown, .. } = &mut self.readout else {
            return Ok(());
        };
        if figures == *shown {
            return Ok(());
        }
        turn.to(path::<Renderer>(())).send(&WriteText { text: *text, runs: figures.runs() })?;
        *shown = figures;

        Ok(())
    }
}
