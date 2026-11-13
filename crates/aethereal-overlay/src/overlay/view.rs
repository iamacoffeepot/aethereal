//! The overlay's screen (ADR-0053: The three stops; ADR-0056): its view in markup, its styles, and what its view reads
//! of it. The band is a stack of a tint, which the pointer passes through to the program beneath, and two columns: on
//! the left, the frame chart, the GPU's graph, and the lines' rows, each the overlay's own primitive, each with its
//! gutter of text beside it and its figures beneath; a line between them; and on the right, the memory meters, a row
//! each of a graph and its gutter. The console, beneath the band, is a scroll of the scrollback's rows, which builds
//! only the rows it shows and keeps to its newest, and the prompt's strip: the prompt, the line's text input with the
//! completion of the word before its caret shown ahead of it, and what is running at its right end. Its panels fill
//! their boxes, so the console blocks the pointer from the program beneath, and the wheel over it scrolls it.
//!
//! ```text
//! Column (the window)
//! ├─ Stack, at the band and the console's stops: Tint (the band's panel), Row:
//! │  ├─ Column (60%): Row (FrameChart, gutter), figures, rule, Row (GpuGraph, gutter), figures, rule, Row (LineRows, gutter)
//! │  ├─ Tint (the line between the columns)
//! │  └─ Column: a Row (MeterGraph, gutter) for each meter
//! └─ Column, at the console's stop (a third of the window): rule, Scroll (the rows), rule, Row (> line…, running)
//! ```

use super::{Calling, Overlay};
use crate::chart::{ChartShown, FrameChart};
use crate::colour::{BONE, LICHEN, PANEL_OPACITY, SLATE, SLATE_DARKER, SLATE_LIGHTER, STONE, Swatch};
use crate::console::Row as PrintedRow;
use crate::gpu::{self, GpuGraph, GpuShown, GpuTiming, GraphTop};
use crate::layout::{
    CONSOLE_SHARE, DIVIDER_PIXELS, GAP_PIXELS, LEFT_SHARE, Layout, METER_INSET_PIXELS, PADDING_PIXELS, SECTION_PIXELS,
    STRIP_PADDING_PIXELS, TEXT_PIXELS, advance_pixels, gutter_pixels, meter_gutter_pixels,
};
use crate::lines::{LineRows, LinesShown};
use crate::meters::{self, MOST_METER_COLUMNS, Meter, MeterDrawn, MeterGraph};
use crate::overlay::stats::MOST_COLUMNS;
use crate::widgets::{Printed, Tint, runs_of};
use aethereal_actor::{Asks, Path, Tells};
use aethereal_render::Renderer;
use aethereal_render::stats::{SubscribeFrameStats, SubscribeGpuTimes, UnsubscribeFrameStats, UnsubscribeGpuTimes};
use aethereal_render::text::{Colour, Font, Run};
use aethereal_tools::{CallTool, Tools};
use aethereal_ui::prelude::*;

/// The overlay's root, the window, and the console's column within it, as the markup places them.
const CONSOLE: [Segment; 2] = [Segment::Place(0), Segment::Place(1)];

/// The path of the scroll the scrollback's rows are in: the console's second child.
pub fn scrollback_path() -> KeyPath {
    KeyPath::of(&[CONSOLE[0], CONSOLE[1], Segment::Place(1)])
}

/// The path of the line's text input: in the prompt's strip, the console's fourth child, in the stack second in it,
/// first.
pub fn prompt_path() -> KeyPath {
    KeyPath::of(&[CONSOLE[0], CONSOLE[1], Segment::Place(3), Segment::Place(1), Segment::Place(0)])
}

/// `left`, `right`, `top`, and `bottom` logical pixels of padding.
fn padded(left: f32, right: f32, top: f32, bottom: f32) -> Edges<Space> {
    Edges {
        left: Space::Pixels(left),
        right: Space::Pixels(right),
        top: Space::Pixels(top),
        bottom: Space::Pixels(bottom),
    }
}

/// `top` and `bottom` logical pixels of margin, and none either side.
fn spaced(top: f32, bottom: f32) -> Edges<Length> {
    Edges {
        left: Length::Pixels(0.0),
        right: Length::Pixels(0.0),
        top: Length::Pixels(top),
        bottom: Length::Pixels(bottom),
    }
}

/// How opaque the lines between the panels are drawn, in stone: faint, so they part the panels without drawing the eye.
const DIVIDER_OPACITY: f32 = 0.28;

/// A panel's colour, at the panels' opacity.
fn panel(swatch: Swatch) -> Colour {
    swatch.at(PANEL_OPACITY)
}

aethereal_ui::style! {
    /// The window, which the overlay's root fills, drawing nothing, so the pointer passes through it.
    pub whole { width: Length::Share(1.0), height: Length::Share(1.0) }
    /// The overlay's text: IBM Plex Mono at the band's size, in bone.
    pub mono { font: Font::Mono, font_size: TEXT_PIXELS, colour: BONE.opaque() }
    /// The band's two columns, its padding above and below them.
    pub band { padding: padded(0.0, 0.0, PADDING_PIXELS, PADDING_PIXELS) }
    /// The left column: the window's share, inside its padding.
    pub left { width: Length::Share(LEFT_SHARE), padding: padded(PADDING_PIXELS, PADDING_PIXELS, 0.0, 0.0) }
    /// The right column: the rest of the window, inside its padding.
    pub right { grow: 1.0, padding: padded(PADDING_PIXELS, PADDING_PIXELS, 0.0, 0.0) }
    /// A graph of the left column: as wide as its row leaves it beside its gutter.
    pub graph { grow: 1.0 }
    /// The gutter beside a graph of the left column: its characters, a section's space after the graph.
    pub gutter { width: Length::Pixels(gutter_pixels()), shrink: 0.0, padding: padded(SECTION_PIXELS, 0.0, 0.0, 0.0) }
    /// A line of figures beneath a graph, a gap beneath it.
    pub figures: mono { margin: spaced(GAP_PIXELS, 0.0) }
    /// The band's panel: slate, so the program shows behind it.
    pub band_panel { colour: panel(SLATE) }
    /// A line between the panels and the sections: faint stone, so it parts them without drawing the eye.
    pub rule { colour: STONE.at(DIVIDER_OPACITY) }
    /// The line between two sections of the left column, centred in the section's space between them.
    pub rule_across: rule {
        height: Length::Pixels(DIVIDER_PIXELS),
        margin: spaced(SECTION_PIXELS / 2.0, SECTION_PIXELS / 2.0 - DIVIDER_PIXELS),
    }
    /// The line between the band's two columns.
    pub rule_down: rule { width: Length::Pixels(DIVIDER_PIXELS), shrink: 0.0 }
    /// A meter's row: a share of the right column's height, its line of text centred down it.
    pub meter_row { grow: 1.0, align_items: Alignment::Center }
    /// A meter's graph or bar: across the right column to its gutter, inset from its row's top and foot.
    pub meter_graph {
        grow: 1.0,
        align_self: SelfAlign::Own(Alignment::Stretch),
        margin: spaced(METER_INSET_PIXELS, METER_INSET_PIXELS),
    }
    /// A meter's gutter: its characters, a character's space after the graph.
    pub meter_gutter: mono {
        width: Length::Pixels(meter_gutter_pixels()),
        shrink: 0.0,
        padding: padded(advance_pixels(), 0.0, 0.0, 0.0),
    }
    /// The console's panel: a third of the window, beneath the band.
    pub console { height: Length::Share(CONSOLE_SHARE) }
    /// The line between the band and the console, over the scrollback's tint.
    pub band_rule: rule { height: Length::Pixels(DIVIDER_PIXELS), background: panel(SLATE_LIGHTER) }
    /// The scrollback: the rest of the console above the prompt's strip, a step lighter than the band, its rows inside
    /// its padding and its bar, faint stone, at its right edge.
    pub scrollback {
        grow: 1.0,
        background: panel(SLATE_LIGHTER),
        padding: padded(PADDING_PIXELS, PADDING_PIXELS, PADDING_PIXELS, GAP_PIXELS),
        colour: STONE.opaque(),
    }
    /// The line between the scrollback and the prompt's strip, over the strip's tint.
    pub prompt_rule: rule { height: Length::Pixels(DIVIDER_PIXELS), background: panel(SLATE_DARKER) }
    /// The prompt's strip: a line of text tall inside its padding, a step darker than the band.
    pub prompt_strip {
        background: panel(SLATE_DARKER),
        padding: padded(PADDING_PIXELS, PADDING_PIXELS, STRIP_PADDING_PIXELS, STRIP_PADDING_PIXELS),
        align_items: Alignment::Center,
    }
    /// The line typed: lichen.
    pub typed: mono { colour: LICHEN.opaque() }
    /// The prompt before the line: lichen, a character's space after it, as a text measures no space at its end.
    pub sign: typed { padding: padded(0.0, advance_pixels(), 0.0, 0.0) }
    /// The line, as wide as the strip leaves it.
    pub line { grow: 1.0 }
}

aethereal_ui::view! { Overlay, ui,
    Column { style: whole,
        Stack { show: self.shows_band(),
            Tint { style: band_panel }
            Row { style: band,
                Column { style: left,
                    Row {
                        FrameChart { style: graph, shown: self.chart(), tall: self.chart_tall() }
                        Column { style: gutter,
                            Printed { style: mono, runs: self.median_runs() }
                            Printed { style: mono, runs: self.alerts_runs() }
                        }
                    }
                    Printed { style: figures, runs: self.figures_runs() }
                    Tint { style: rule_across }
                    Row { show: self.gpu_timed(),
                        GpuGraph { style: graph, shown: self.gpu_graph(), tall: self.gpu_tall() }
                        Column { style: gutter,
                            Printed { style: mono, runs: self.gpu_top_runs() }
                        }
                    }
                    Printed { style: figures, runs: self.gpu_figures_runs() }
                    Tint { style: rule_across }
                    Row {
                        LineRows { style: graph, shown: self.line_rows() }
                        Column { style: gutter,
                            Printed { style: mono, runs: self.lines_runs() }
                        }
                    }
                }
                Tint { style: rule_down }
                Column { style: right, items: self.meters() as meter key meter.number,
                    Row { style: meter_row,
                        MeterGraph { style: meter_graph, drawn: meter.drawn }
                        Printed { style: meter_gutter, runs: meter.runs }
                    }
                }
            }
        }
        Column { style: console, show: self.shows_console(),
            Tint { style: band_rule }
            Scroll { .., style: scrollback, keeps: Keeps::End, items: self.rows() as row key row.number(),
                Printed { style: mono, runs: row.runs() }
            }
            Tint { style: prompt_rule }
            Row { style: prompt_strip,
                Text { style: sign, ">" }
                Stack { style: line,
                    TextInput { style: typed, text: self.line(), change => self.edit(), submit => self.enter() }
                    Printed { style: mono, runs: self.hint_runs() }
                }
                Printed { style: mono, runs: self.running_runs() }
            }
        }
    }
}

/// What the overlay's methods send: the renderer's frame statistics and the GPU's times, subscribed to as it is shown
/// and let go as it hides, and the tools the console's line calls.
pub type OverlaySends = aethereal_ui::sends![
    Tells<Path<Renderer>, SubscribeFrameStats>,
    Tells<Path<Renderer>, UnsubscribeFrameStats>,
    Tells<Path<Renderer>, SubscribeGpuTimes>,
    Tells<Path<Renderer>, UnsubscribeGpuTimes>,
    Asks<Path<Tools>, CallTool, Calling>,
];

/// A row of the scrollback as its scroll shows it: one printed, or a blank one above the first while the rows printed
/// do not fill the view, so the newest stands just above the prompt.
pub enum Shown<'console> {
    Printed(&'console PrintedRow),
    Blank(u64),
}

impl Shown<'_> {
    /// What tells it from the other rows: a row's number, or, for a blank one, a number counted down from the most.
    pub const fn number(&self) -> u64 {
        match self {
            Self::Printed(row) => row.number,
            Self::Blank(number) => *number,
        }
    }

    /// Its runs.
    pub fn runs(&self) -> Vec<Run> {
        match self {
            Self::Printed(row) => runs_of(row.pieces.iter().cloned()),
            Self::Blank(_) => runs_of(Vec::new()),
        }
    }
}

/// A meter's row as the right column shows it: the meter, its graph or bar, and its line of text.
pub struct MeterRow {
    pub number: u8,
    pub drawn: MeterDrawn,
    pub runs: Vec<Run>,
}

impl Overlay {
    /// The overlay as the window is now.
    fn layout(&self) -> Layout {
        Layout::of(self.ui.viewport())
    }

    /// Whether the band shows: at the band's stop and the console's.
    pub(super) const fn shows_band(&self) -> bool {
        self.stop.draws()
    }

    /// Whether the console shows.
    pub(super) const fn shows_console(&self) -> bool {
        self.stop.console()
    }

    /// How tall the frame chart stands on the window now.
    pub(super) fn chart_tall(&self) -> f32 {
        self.layout().chart_pixels_tall()
    }

    /// How tall the GPU's graph stands on the window now.
    pub(super) fn gpu_tall(&self) -> f32 {
        self.layout().gpu_pixels_tall()
    }

    /// What the frame chart shows: its columns as last cut, where they stand, the budget, and the marks.
    pub(super) fn chart(&self) -> ChartShown {
        let count = self.layout().chart_columns(MOST_COLUMNS);

        ChartShown {
            columns: self.frames.columns().collect(),
            slide: self.frames.slide(count),
            budget: self.budget,
            marks: self.marks.times().collect(),
        }
    }

    /// The gutter's line beside the frame chart: its median.
    pub(super) fn median_runs(&self) -> Vec<Run> {
        self.figures.gutter_runs()
    }

    /// The gutter's line beneath the median: the warnings and errors since the console was last opened.
    pub(super) fn alerts_runs(&self) -> Vec<Run> {
        runs_of(self.alerts.pieces())
    }

    /// The figures beneath the frame chart.
    pub(super) fn figures_runs(&self) -> Vec<Run> {
        self.figures.runs()
    }

    /// Whether the device times the GPU's passes, so its graph and figures show.
    pub(super) const fn gpu_timed(&self) -> bool {
        matches!(self.gpu, GpuTiming::Timed(_))
    }

    /// What the GPU's graph shows: its columns as last cut, where they stand, and the budget.
    pub(super) fn gpu_graph(&self) -> GpuShown {
        let count = self.layout().chart_columns(MOST_COLUMNS);
        let columns = match &self.gpu {
            GpuTiming::Timed(times) => times.columns().collect(),
            GpuTiming::Unsupported => Vec::new(),
        };

        GpuShown { columns, slide: self.frames.slide(count), budget: self.budget }
    }

    /// The gutter's line beside the GPU's graph: the top it is drawn to.
    pub(super) fn gpu_top_runs(&self) -> Vec<Run> {
        GraphTop::of(self.gpu_graph().columns).runs()
    }

    /// The figures beneath the GPU's graph, or in its place that the device has no timestamps.
    pub(super) fn gpu_figures_runs(&self) -> Vec<Run> {
        match self.gpu {
            GpuTiming::Timed(_) => self.gpu_figures.runs(),
            GpuTiming::Unsupported => gpu::unsupported(),
        }
    }

    /// What the lines' rows show: their columns as last cut, and where they stand.
    pub(super) fn line_rows(&self) -> LinesShown {
        let count = self.layout().chart_columns(MOST_COLUMNS);

        LinesShown { columns: self.lines.columns().collect(), slide: self.frames.slide(count) }
    }

    /// The gutter's lines beside the lines' rows: each label and its newest value.
    pub(super) fn lines_runs(&self) -> Vec<Run> {
        self.lines.runs()
    }

    /// Each meter's row: its graph or bar as last cut, and its line of text.
    pub(super) fn meters(&self) -> Vec<MeterRow> {
        let count = self.layout().meter_columns(MOST_METER_COLUMNS);
        let slide = self.frames.slide(count);
        let gpu_bytes = self.histories.gpu_shown();

        Meter::ALL
            .into_iter()
            .map(|meter| MeterRow {
                number: meter.index(),
                drawn: meters::drawn(meter, &self.memory, &self.histories, slide, count),
                runs: self.memory.row_runs(meter, gpu_bytes),
            })
            .collect()
    }

    /// The scrollback's rows, oldest first, after as many blank ones as the view has rows the printed ones do not fill.
    pub(super) fn rows(&self) -> Vec<Shown<'_>> {
        let printed = self.console.rows();
        let blank = self.layout().scrollback_rows().saturating_sub(printed.len());
        let blanks = (0..blank).rev().map(|back| Shown::Blank(u64::MAX - u64::try_from(back).unwrap_or(u64::MAX)));

        blanks.chain(printed.map(Shown::Printed)).collect()
    }

    /// The line as typed.
    pub(super) fn line(&self) -> String {
        String::from(self.console.line())
    }

    /// Where the line's caret stands, a byte offset into it: at its end while its text input has not edited the line
    /// as it stands, as a line recalled or completed puts it there.
    pub(super) fn caret(&self) -> usize {
        let line = self.console.line();
        let editing = self.ui.table().state::<TextInput<OverlayEvent>>(&prompt_path()).unwrap_or_default();
        let own = editing.edited == line;
        if !own {
            return line.len();
        }

        usize::try_from(editing.caret).unwrap_or(usize::MAX)
    }

    /// The completion of the word before the caret, in stone, set after the line as typed, which is drawn clear, so the
    /// completion stands just ahead of the line in the mono face.
    pub(super) fn hint_runs(&self) -> Vec<Run> {
        let ahead = self.console.completion_ahead(self.caret());
        if ahead.is_empty() {
            return runs_of(Vec::new());
        }

        vec![Run::new(self.console.line(), Colour::CLEAR), Run::new(&ahead, STONE.opaque())]
    }

    /// What is running, at the prompt's right end.
    pub(super) fn running_runs(&self) -> Vec<Run> {
        runs_of(self.console.running())
    }
}
