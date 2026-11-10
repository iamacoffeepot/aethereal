//! The overlay's texts (ADR-0052, ADR-0053): the frame chart's figures, its latest median and the count of warnings
//! and errors in the gutter beside it, the GPU's figures, the GPU graph's top and the rows' labels and values in their
//! gutter, the memory meters'
//! labels and values and the adapter's line in the band's right column, and the console's scrollback, its prompt,
//! and what is running at the prompt's right end. Each is one text, made once and written again only when what it
//! shows changes, so a console that nothing changes costs nothing a frame. The scrollback is one text of the rows in
//! view, at a fixed place, clipped to the console's panel, and written again as the view scrolls or a row is printed.

use super::Overlay;
use crate::colour::BONE;
use crate::console::{Piece, View};
use crate::gpu::{self, GpuTiming};
use crate::layout::Layout;
use crate::lines::Line;
use aethereal_actor::{Allows, Fault, Path, Tells, Turn, path};
use aethereal_data::{Bound, Plain};
use aethereal_math::Vec2;
use aethereal_render::Renderer;
use aethereal_render::placement::PlacementId;
use aethereal_render::text::{Clip, Layer, MoveText, Run, TextId, WriteText};

/// Which of the overlay's texts.
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextRole {
    /// The figures beneath the chart.
    Figures,
    /// The chart's latest median, in the gutter beside it.
    FrameMedian,
    /// The count of warnings and errors, in the gutter beside the chart beneath its median.
    Alerts,
    /// The figures beneath the GPU's graph, or in its place that the device has no timestamps.
    GpuFigures,
    /// The GPU graph's top, in the gutter beside it: nothing on a device without timestamps.
    GpuTop,
    /// The rows' labels and their newest values, in the gutter beside them.
    Lines,
    /// The memory meters' labels and values, in the band's right column.
    Meters,
    /// The GPU adapter's line, beneath the meters.
    Adapter,
    /// The rows of the scrollback in view.
    Scrollback,
    /// The prompt's line: the prompt, the line typed, and a completion ahead of it.
    Prompt,
    /// The tools called from the line and not yet answered, at the prompt's right end.
    Running,
}

impl TextRole {
    /// Every text, in the order the overlay lists them: the band's first, then the console's.
    pub const ALL: [Self; 11] = [
        Self::Figures,
        Self::FrameMedian,
        Self::Alerts,
        Self::GpuFigures,
        Self::GpuTop,
        Self::Lines,
        Self::Meters,
        Self::Adapter,
        Self::Scrollback,
        Self::Prompt,
        Self::Running,
    ];

    /// How many of [`Self::ALL`], from the first, are the band's.
    pub const BAND: usize = 8;
}

/// What the overlay has made of one text.
#[derive(Bound)]
pub enum Text {
    /// Not asked for: the window has not said its size, which the text is sized by.
    Unasked,
    /// Its asks are under way.
    Making,
    /// Placed: the text, its placement, and the runs it was last written with.
    Made { text: TextId, placement: PlacementId, shown: Vec<Run> },
}

/// Each of the overlay's texts.
#[derive(Bound)]
pub struct Texts {
    figures: Text,
    frame_median: Text,
    alerts: Text,
    gpu_figures: Text,
    gpu_top: Text,
    lines: Text,
    meters: Text,
    adapter: Text,
    scrollback: Text,
    prompt: Text,
    running: Text,
}

impl Default for Texts {
    fn default() -> Self {
        Self {
            figures: Text::Unasked,
            frame_median: Text::Unasked,
            alerts: Text::Unasked,
            gpu_figures: Text::Unasked,
            gpu_top: Text::Unasked,
            lines: Text::Unasked,
            meters: Text::Unasked,
            adapter: Text::Unasked,
            scrollback: Text::Unasked,
            prompt: Text::Unasked,
            running: Text::Unasked,
        }
    }
}

impl Texts {
    /// The text of `role`.
    pub const fn of(&self, role: TextRole) -> &Text {
        match role {
            TextRole::Figures => &self.figures,
            TextRole::FrameMedian => &self.frame_median,
            TextRole::Alerts => &self.alerts,
            TextRole::GpuFigures => &self.gpu_figures,
            TextRole::GpuTop => &self.gpu_top,
            TextRole::Lines => &self.lines,
            TextRole::Meters => &self.meters,
            TextRole::Adapter => &self.adapter,
            TextRole::Scrollback => &self.scrollback,
            TextRole::Prompt => &self.prompt,
            TextRole::Running => &self.running,
        }
    }

    /// The text of `role`, to change.
    pub const fn of_mut(&mut self, role: TextRole) -> &mut Text {
        match role {
            TextRole::Figures => &mut self.figures,
            TextRole::FrameMedian => &mut self.frame_median,
            TextRole::Alerts => &mut self.alerts,
            TextRole::GpuFigures => &mut self.gpu_figures,
            TextRole::GpuTop => &mut self.gpu_top,
            TextRole::Lines => &mut self.lines,
            TextRole::Meters => &mut self.meters,
            TextRole::Adapter => &mut self.adapter,
            TextRole::Scrollback => &mut self.scrollback,
            TextRole::Prompt => &mut self.prompt,
            TextRole::Running => &mut self.running,
        }
    }

    /// The placement of the text of `role`, once it is placed.
    pub const fn placement(&self, role: TextRole) -> Option<PlacementId> {
        match self.of(role) {
            Text::Made { placement, .. } => Some(*placement),
            Text::Unasked | Text::Making => None,
        }
    }
}

/// `pieces` as a text's runs, each piece's colour opaque: a piece of no text left out, and a space for none at all, as
/// a text holds something.
pub fn runs_of(pieces: impl IntoIterator<Item = Piece>) -> Vec<Run> {
    let mut runs: Vec<Run> = pieces
        .into_iter()
        .filter(|piece| !piece.text.is_empty())
        .map(|piece| Run::new(&piece.text, piece.swatch.opaque()))
        .collect();
    if runs.is_empty() {
        runs.push(Run::new(" ", BONE.opaque()));
    }

    runs
}

impl Overlay {
    /// How much of the console shows on the window as it is now.
    pub(super) fn view(&self) -> View {
        let layout = Layout::of(self.surface);

        View { columns: layout.console_columns(), rows: layout.scrollback_rows() }
    }

    /// The runs the text of `role` shows now. The scrollback's rows in view are set at the bottom of its lines, after
    /// as many empty lines as there are rows in view it does not fill, so the newest sits just above the prompt.
    pub(super) fn runs(&self, role: TextRole) -> Vec<Run> {
        match role {
            TextRole::Figures => self.figures.runs(),
            TextRole::FrameMedian => self.figures.gutter_runs(),
            TextRole::Alerts => runs_of(self.alerts.pieces()),
            TextRole::GpuFigures => match self.gpu {
                GpuTiming::Timed(_) => self.gpu_figures.runs(),
                GpuTiming::Unsupported => gpu::unsupported(),
            },
            TextRole::GpuTop => match self.gpu {
                GpuTiming::Timed(_) => self.gpu_top.runs(),
                GpuTiming::Unsupported => runs_of(Vec::new()),
            },
            TextRole::Lines => self.lines.runs(),
            TextRole::Meters => {
                let gpu_bytes = self.lines.latest().of_line(Line::Memory);
                self.memory.runs(gpu_bytes, Layout::of(self.surface).right_columns())
            }
            TextRole::Adapter => self.adapter.runs(Layout::of(self.surface).right_columns()),
            TextRole::Prompt => runs_of(self.console.prompt()),
            TextRole::Running => runs_of(self.console.running()),
            TextRole::Scrollback => {
                let view = self.view();
                let rows: Vec<_> = self.console.rows(view).collect();
                let blank = view.rows.saturating_sub(rows.len());
                let mut pieces = vec![Piece::new(&"\n".repeat(blank), BONE)];
                for (index, row) in rows.into_iter().enumerate() {
                    if index > 0 {
                        pieces.push(Piece::new("\n", BONE));
                    }
                    pieces.extend(row.pieces.iter().cloned());
                }
                runs_of(pieces)
            }
        }
    }

    /// Where the text of `role` is placed, and what of it is drawn: the console's texts clipped to its panel, and the
    /// right column's to the column, so neither runs into the left column on a narrow window. A text
    /// that ends at a right edge is placed by how many characters it shows now, and the GPU's figures stand in the
    /// graph's place when the device has no timestamps, as there is no graph.
    pub(super) fn place_of(&self, role: TextRole) -> (Vec2, Clip) {
        let layout = Layout::of(self.surface);
        let characters = |pieces: Vec<Piece>| pieces.iter().map(|piece| piece.text.chars().count()).sum();
        match role {
            TextRole::Figures => (layout.figures_at(), Clip::None),
            TextRole::FrameMedian => (layout.frame_median_at(), Clip::None),
            TextRole::Alerts => (layout.alerts_at(), Clip::None),
            TextRole::GpuFigures => match self.gpu {
                GpuTiming::Timed(_) => (layout.gpu_figures_at(), Clip::None),
                GpuTiming::Unsupported => (Vec2::new(layout.gpu_figures_at().x, layout.gpu_graph().min.y), Clip::None),
            },
            TextRole::GpuTop => (layout.gpu_top_at(), Clip::None),
            TextRole::Lines => (layout.lines_at(), Clip::None),
            TextRole::Meters => (layout.meters_at(), Clip::To(layout.right_column())),
            TextRole::Adapter => (layout.adapter_at(), Clip::To(layout.right_column())),
            TextRole::Scrollback => (layout.scrollback_at(), Clip::To(layout.console_inside())),
            TextRole::Prompt => (layout.prompt_at(), Clip::To(layout.console_inside())),
            TextRole::Running => {
                let at = layout.running_at(characters(self.console.running()));
                (at, Clip::To(layout.console_inside()))
            }
        }
    }

    /// Writes the text of `role` again when what it shows has changed since it was last written. Nothing while it is
    /// not placed.
    pub(super) fn write_text<S, I>(&mut self, role: TextRole, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<Tells<Path<Renderer>, WriteText>, I>,
    {
        let runs = self.runs(role);
        let Text::Made { text, shown, .. } = self.texts.of_mut(role) else {
            return Ok(());
        };
        if runs == *shown {
            return Ok(());
        }
        turn.to(path::<Renderer>(())).send(&WriteText { text: *text, runs: runs.clone() })?;
        *shown = runs;

        Ok(())
    }

    /// Moves the text of `role` to where the layout puts it now. Nothing while it is not placed.
    pub(super) fn move_text<S, I>(&self, role: TextRole, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<Tells<Path<Renderer>, MoveText>, I>,
    {
        let Some(placement) = self.texts.placement(role) else {
            return Ok(());
        };
        let (at, clip) = self.place_of(role);
        turn.to(path::<Renderer>(())).send(&MoveText { placement, at, layer: Layer::Over, opacity: 1.0, clip })?;

        Ok(())
    }

    /// Writes the console's scrollback and prompt again where they have changed: after a key, an answer, or a log.
    pub(super) fn write_console<S, I>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<Tells<Path<Renderer>, WriteText>, I>,
    {
        let shown = self.stop.console();
        if !shown {
            return Ok(());
        }
        self.write_text(TextRole::Scrollback, turn)?;

        self.write_text(TextRole::Prompt, turn)
    }

    /// Writes a text that ends at a right edge again where it has changed, and moves it to end there: what is running
    /// at the prompt's right end; or the GPU's figures, which move to the graph's place when the device has no
    /// timestamps.
    pub(super) fn write_and_move<S, I, J>(&mut self, role: TextRole, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<Tells<Path<Renderer>, WriteText>, I> + Allows<Tells<Path<Renderer>, MoveText>, J>,
    {
        let before = self.texts.of(role);
        let unchanged = matches!(before, Text::Made { shown, .. } if *shown == self.runs(role));
        if unchanged {
            return Ok(());
        }
        self.write_text(role, turn)?;

        self.move_text(role, turn)
    }
}
