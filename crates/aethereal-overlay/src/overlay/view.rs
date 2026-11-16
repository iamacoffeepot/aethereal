//! The overlay's screen (ADR-0053: The three stops; ADR-0056): its view in markup, its styles, and what its view reads
//! of it. The band is a stack of a tint, which the pointer passes through to the program beneath, and two columns: on
//! the left, the frame chart, the GPU's graph, and the lines' rows, each the overlay's own primitive, each with its
//! gutter of text beside it and its figures beneath; a line between them; and on the right, the memory meters, a row
//! each of a graph and its gutter. The console, beneath the band, is a scroll of the scrollback's rows, which builds
//! only the rows it shows and keeps to its newest, and the prompt's strip: the prompt, the line's text input with the
//! completion of the word before its caret shown ahead of it, and what is running at its right end. Its panels fill
//! their boxes, so the console blocks the pointer from the program beneath, and the wheel over it scrolls it. The band
//! is a see-through HUD over the scene and the console a sheet laid over it: the console's panels are nearly opaque, and
//! the grip, a seam whose shadow the sheet casts on the band, parts the two, so the band's graphs never read as the
//! console's text. Each panel of the console has its own slate: a step lighter than the band for the transcript, lighter
//! again for the owner's turns, a step darker for the prompt, and darkest for the tabs beneath it; and every divider in
//! it is one faint ink of stone.
//!
//! ```text
//! Column (the window)
//! ├─ Stack, at the band and the console's stops: Tint (the band's panel), Row:
//! │  ├─ Column (60%): Row (FrameChart, gutter), figures, rule, Row (GpuGraph, gutter), figures, rule, Row (LineRows, gutter)
//! │  ├─ Tint (the line between the columns)
//! │  └─ Column: a Row (MeterGraph, gutter) for each meter
//! └─ Column, at the console's stop (a third of the window): Grip (the seam), Scroll (the rows) or Scroll (a
//!    transcript), asks, rule, Row (> line…, running), rule, Row (the tabs: console ‖ session | session)
//! ```

use super::{Calling, Overlay};
use crate::chart::{ChartShown, FrameChart};
use crate::chat::{Open, Parting, Row as TranscriptRow};
use crate::colour::{
    BONE, CONSOLE_OPACITY, LICHEN, OCHRE, PANEL_OPACITY, SLATE, SLATE_DARKER, SLATE_DEEPEST, SLATE_LIGHTER, STONE,
    Swatch,
};
use crate::console::{Piece, Row as PrintedRow};
use crate::gpu::{self, GpuGraph, GpuShown, GpuTiming, GraphTop};
use crate::layout::{
    DIVIDER_PIXELS, GAP_PIXELS, GRIP_PIXELS, LEFT_SHARE, Layout, METER_INSET_PIXELS, PADDING_PIXELS, SECTION_PIXELS,
    STRIP_PADDING_PIXELS, TAB_ACCENT_PIXELS, TAB_BREAK_PIXELS, TAB_DIVIDER_PIXELS, TAB_PADDING_PIXELS,
    TAB_STRIP_PADDING_PIXELS, TEXT_PIXELS, advance_pixels, gutter_pixels, meter_gutter_pixels,
};
use crate::lines::{LineRows, LinesShown};
use crate::meters::{self, MOST_METER_COLUMNS, Meter, MeterDrawn, MeterGraph};
use crate::overlay::stats::MOST_COLUMNS;
use crate::widgets::{Grip, Printed, Tint, runs_of};
use aethereal_actor::{Asks, Fault, Level, Path, Tells, ToType, Turn};
use aethereal_link::Link;
use aethereal_render::Renderer;
use aethereal_render::stats::{SubscribeFrameStats, SubscribeGpuTimes, UnsubscribeFrameStats, UnsubscribeGpuTimes};
use aethereal_render::text::{Colour, Font, Run};
use aethereal_sessions::Chat;
use aethereal_tools::{CallTool, Tools};
use aethereal_ui::markdown::{Images, Inks, MarkdownBlock, Prose};
use aethereal_ui::prelude::*;

/// The overlay's root, the window, and the console's column within it, as the markup places them.
const CONSOLE: [Segment; 2] = [Segment::Place(0), Segment::Place(1)];

/// The path of the scroll the scrollback's rows are in: the console's second child.
pub fn scrollback_path() -> KeyPath {
    KeyPath::of(&[CONSOLE[0], CONSOLE[1], Segment::Place(1)])
}

/// The path of the scroll a session's transcript is in: the console's third child.
pub fn transcript_path() -> KeyPath {
    KeyPath::of(&[CONSOLE[0], CONSOLE[1], Segment::Place(2)])
}

/// The path of the line's text input: in the prompt's strip, the console's sixth child, in the stack second in it,
/// first.
pub fn prompt_path() -> KeyPath {
    KeyPath::of(&[CONSOLE[0], CONSOLE[1], Segment::Place(5), Segment::Place(1), Segment::Place(0)])
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

/// The most lines the console's line grows to as it is typed, past which it slides through them: Enter runs it, and
/// Shift-Enter breaks it.
const PROMPT_LINES: u32 = 6;

/// How opaque the lines between the band's sections are drawn, in stone: faint, so they part them without drawing the
/// eye.
const DIVIDER_OPACITY: f32 = 0.28;

/// How opaque the console's one divider ink is, in stone: every line in the console, above the prompt, above the tabs,
/// and between the tabs, so they part its panels and its tabs alike without drawing the eye, and a break differs from
/// a light divider by its height and its space alone.
const INK_OPACITY: f32 = 0.22;

/// How opaque the grip's handle is drawn, in stone: a mark that says the seam is taken hold of, quieter than the text.
const HANDLE_OPACITY: f32 = 0.35;

/// The band's panel's colour, at its opacity.
fn panel(swatch: Swatch) -> Colour {
    swatch.at(PANEL_OPACITY)
}

/// A console's panel's colour, at its opacity.
fn sheet(swatch: Swatch) -> Colour {
    swatch.at(CONSOLE_OPACITY)
}

/// `left` and `right` logical pixels of margin, and `top` and `bottom`.
fn margined(left: f32, right: f32, top: f32, bottom: f32) -> Edges<Length> {
    Edges {
        left: Length::Pixels(left),
        right: Length::Pixels(right),
        top: Length::Pixels(top),
        bottom: Length::Pixels(bottom),
    }
}

/// The padding inside a transcript's row: around one of the owner's lines, so its shade stands clear of its text, and
/// none around any other.
fn inset(row: &TranscriptRow) -> Edges<Space> {
    if row.is_turn() {
        padded(GAP_PIXELS, GAP_PIXELS, GAP_PIXELS / 2.0, GAP_PIXELS / 2.0)
    } else {
        padded(0.0, 0.0, 0.0, 0.0)
    }
}

impl Parting {
    /// How tall the line before the tab is: taller for the break than for a light divider.
    fn height(self) -> Length {
        match self {
            Self::Break => Length::Pixels(TAB_BREAK_PIXELS),
            Self::First | Self::Light => Length::Pixels(TAB_DIVIDER_PIXELS),
        }
    }

    /// The space either side of the line: room either side of the break, and none beside a light divider, whose
    /// tabs' own padding parts it from their titles.
    fn margin(self) -> Edges<Length> {
        match self {
            Self::Break => margined(PADDING_PIXELS, PADDING_PIXELS, 0.0, 0.0),
            Self::First | Self::Light => margined(0.0, 0.0, 0.0, 0.0),
        }
    }
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
    /// A line between the band's sections: faint stone, so it parts them without drawing the eye.
    pub rule { colour: STONE.at(DIVIDER_OPACITY) }
    /// A line in the console: its one divider ink, so its panels and its tabs are parted alike.
    pub ink { colour: STONE.at(INK_OPACITY) }
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
    /// The console's panel, beneath the band: its height is the overlay's, set by dragging its grip.
    pub console {}
    /// The seam between the band and the console, a grip a few pixels tall for dragging over its whole width: a well
    /// of the prompt's slate under the shadow the console's sheet casts on the band, with a short stone handle centred
    /// in it to say it is taken hold of.
    pub grip { height: Length::Pixels(GRIP_PIXELS), background: sheet(SLATE_DARKER), colour: STONE.at(HANDLE_OPACITY) }
    /// The scrollback: the rest of the console above the prompt's strip, a step lighter than the band, its rows inside
    /// its padding and its bar, faint stone, at its right edge.
    pub scrollback {
        grow: 1.0,
        background: sheet(SLATE_LIGHTER),
        padding: padded(PADDING_PIXELS, PADDING_PIXELS, PADDING_PIXELS, GAP_PIXELS),
        colour: STONE.opaque(),
    }
    /// A session's transcript: as the scrollback is, its rows a small gap apart.
    pub transcript: scrollback { gap: Gap { rows: Space::Pixels(GAP_PIXELS), columns: Space::Pixels(0.0) } }
    /// One row of a transcript, across it, at least a line of text tall, which the scroll windows its rows by; its
    /// background and padding its own, the owner's lines set apart from Claude's on a shade.
    pub entry { width: Length::Share(1.0), min_height: Length::Pixels(TEXT_PIXELS) }
    /// A line of a transcript, which its reader can select and copy: the mono face, its colour its own.
    pub said: mono {}
    /// One of the owner's lines: its tag, then its text.
    pub turn { width: Length::Share(1.0) }
    /// An owner's line's tag, who typed it and the prompt mark: a label in its own colour, unbroken, a character's space
    /// after it, as a text measures no space at its end.
    pub tag: mono { shrink: 0.0, padding: padded(0.0, advance_pixels(), 0.0, 0.0) }
    /// An owner's line's text, as wide as the row leaves it.
    pub turn_text: said { grow: 1.0 }
    /// The fold of a subagent's events, pressed to open or close it.
    pub fold { align_self: SelfAlign::Own(Alignment::Start) }
    /// The fold's text: stone.
    pub folded: mono { colour: STONE.opaque() }
    /// Claude's text, as markdown.
    pub reply { width: Length::Share(1.0) }
    /// The asks of the open session that wait on the owner, above the prompt's strip.
    pub asks { background: sheet(SLATE_DARKER), padding: padded(PADDING_PIXELS, PADDING_PIXELS, GAP_PIXELS, GAP_PIXELS) }
    /// One ask, its answers at its end.
    pub ask_row { align_items: Alignment::Center, gap: Gap { rows: Space::Pixels(0.0), columns: Space::Pixels(SECTION_PIXELS) } }
    /// What an ask says: ochre, as wide as the row leaves it.
    pub asked: mono { colour: OCHRE.opaque(), grow: 1.0 }
    /// An ask's answer, pressed.
    pub answer { padding: padded(SECTION_PIXELS, SECTION_PIXELS, 2.0, 2.0), background: sheet(SLATE_LIGHTER) }
    /// The session tabs beneath the prompt's strip, nearest the scene: a surface of their own, darkest of the console's,
    /// so the prompt and the tabs never merge, the tabs centred down it with even space above and below.
    pub tab_strip {
        background: sheet(SLATE_DEEPEST),
        padding: padded(PADDING_PIXELS, PADDING_PIXELS, TAB_STRIP_PADDING_PIXELS, TAB_STRIP_PADDING_PIXELS),
        align_items: Alignment::Center,
    }
    /// The line along the tabs' strip's top, in the divider ink over the strip's slate, parting it from the prompt.
    pub tab_strip_rule: ink { height: Length::Pixels(DIVIDER_PIXELS), background: sheet(SLATE_DEEPEST) }
    /// A tab and what parts it from the one before, centred down the strip.
    pub tab_item { align_items: Alignment::Center }
    /// What parts a tab from the one before: a line in the divider ink, its height and space as its parting says,
    /// centred against the tabs' titles.
    pub tab_rule: ink { width: Length::Pixels(DIVIDER_PIXELS), align_self: SelfAlign::Own(Alignment::Center), shrink: 0.0 }
    /// A tab, pressed to open it: its title and, while it is open, the line along its top laid over each other.
    pub tab_button {}
    /// The line along the open tab's top: lichen, as the typed line is, laid over the space above the tab's title.
    pub tab_accent {
        width: Length::Share(1.0),
        height: Length::Pixels(TAB_ACCENT_PIXELS),
        align_self: SelfAlign::Own(Alignment::Start),
        colour: LICHEN.opaque(),
    }
    /// A tab's title, inside the tab's padding: stone, or bone while open.
    pub tab_text: mono { padding: padded(PADDING_PIXELS, PADDING_PIXELS, TAB_PADDING_PIXELS, TAB_PADDING_PIXELS) }
    /// The line between the scrollback and the prompt's strip, in the divider ink over the strip's slate.
    pub prompt_rule: ink { height: Length::Pixels(DIVIDER_PIXELS), background: sheet(SLATE_DARKER) }
    /// The prompt's strip: a line of text tall inside its padding, a step darker than the band.
    pub prompt_strip {
        background: sheet(SLATE_DARKER),
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
        Column { style: console { height: Length::Share(self.console_share) }, show: self.shows_console(),
            Grip { style: grip, change => self.resize() }
            Scroll { .., style: scrollback, show: self.shows_scrollback(), keeps: Keeps::End,
                items: self.rows() as row key row.number(),
                Printed { style: mono, runs: row.runs() }
            }
            Scroll { .., style: transcript, show: self.shows_transcript(), keeps: Keeps::End,
                items: self.chats.rows() as row key row.key,
                Column { style: entry { background: row.background(), padding: inset(row) },
                    Selectable { show: row.is_line(), style: said { colour: row.colour() }, text: row.text() }
                    Row { show: row.is_turn(), style: turn,
                        Printed { style: tag, runs: row.tag_runs() }
                        Selectable { style: turn_text { colour: row.colour() }, text: row.text() }
                    }
                    Button { show: row.is_fold(), style: fold, click => self.fold(row.key),
                        Text { style: folded, "{row.text()}" }
                    }
                    Column { style: reply, items: self.chats.reply(row) as block key block.index,
                        MarkdownBlock { memo, block: block, prose: self.prose(), images: self.images(), link => self.follow() }
                    }
                }
            }
            Column { style: asks, show: self.asking(), items: self.chats.asks() as ask key ask.ask,
                Row { style: ask_row,
                    Text { style: asked, "session {ask.session} asks: {ask.what}" }
                    Button { style: answer, click => self.allow(ask.ask), Text { style: mono, "allow" } }
                    Button { style: answer, click => self.deny(ask.ask), Text { style: mono, "deny" } }
                }
            }
            Tint { style: prompt_rule }
            Row { style: prompt_strip,
                Text { style: sign, ">" }
                Stack { style: line,
                    TextInput {
                        style: typed,
                        text: self.line(),
                        most_lines: PROMPT_LINES,
                        enter: Enter::Submits,
                        change => self.edit(),
                        submit => self.enter(),
                    }
                    Printed { style: mono, runs: self.hint_runs() }
                }
                Printed { style: mono, runs: self.running_runs() }
            }
            Tint { style: tab_strip_rule, show: self.chats.is_listed() }
            Row { style: tab_strip, show: self.chats.is_listed(), items: self.chats.titles() as tab key tab.place,
                Row { style: tab_item,
                    Tint {
                        show: tab.parting.parts(),
                        style: tab_rule { height: tab.parting.height(), margin: tab.parting.margin() },
                    }
                    Button { style: tab_button { background: tab.background() }, click => self.choose(tab.place),
                        Text { style: tab_text { colour: tab.colour() }, "{tab.name}" }
                        Tint { style: tab_accent, show: tab.open }
                    }
                }
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
    ToType<Link, Chat>,
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

    /// Whether the console shows the engine's own scrollback: with its own tab open, as it always is before the hub
    /// lists the sessions.
    pub(super) const fn shows_scrollback(&self) -> bool {
        matches!(self.chats.open(), Open::Console)
    }

    /// Whether the console shows a session's transcript.
    pub(super) const fn shows_transcript(&self) -> bool {
        matches!(self.chats.open(), Open::Session(_))
    }

    /// Whether the open session has asks that wait on the owner.
    pub(super) fn asking(&self) -> bool {
        !self.chats.asks().is_empty()
    }

    /// The style set Claude's markdown is built with: the library's, with every text in the console's own face,
    /// IBM Plex Mono, and its body at the console's size, so a reply reads as every other line of the console does.
    #[allow(clippy::unused_self, reason = "markup reads a computed value through a `&self` method")]
    pub(super) fn prose(&self) -> Prose {
        let inks = Inks::default();
        let body = |style: Style| Style { font: Font::Mono, font_size: TEXT_PIXELS, ..style };
        let heading = |style: Style| Style { font: Font::Mono, ..style };
        let [first, second, third] = inks.headings;

        Prose::new(Inks {
            paragraph: body(inks.paragraph),
            headings: [heading(first), heading(second), heading(third)],
            code: body(inks.code),
            marker: body(inks.marker),
            head_cell: body(inks.head_cell),
            cell: body(inks.cell),
            ..inks
        })
    }

    /// The pictures of Claude's markdown's images: none, as the overlay fetches no image a reply names.
    #[allow(clippy::unused_self, reason = "markup reads a computed value through a `&self` method")]
    pub(super) fn images(&self) -> Images {
        Images::default()
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

    /// The gutter's line beneath the median: the warnings and errors since the console was last opened, and how many
    /// sessions wait on the owner, in ochre, when any does (ADR-0057).
    pub(super) fn alerts_runs(&self) -> Vec<Run> {
        let mut pieces = self.alerts.pieces();
        let waiting = self.chats.waiting();
        if waiting > 0 {
            pieces.push(Piece::new(&format!(" {waiting}?"), OCHRE));
        }

        runs_of(pieces)
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
        let blank = self.layout().scrollback_rows(self.console_share).saturating_sub(printed.len());
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
    /// completion stands just ahead of the line in the mono face: none once the line runs onto a second line, where the
    /// line wraps as its text input lays it out and the hint would not.
    pub(super) fn hint_runs(&self) -> Vec<Run> {
        let ahead = self.console.completion_ahead(self.caret());
        let line = self.console.line();
        let broken = line.contains('\n');
        let wraps = line.chars().count() + ahead.chars().count() >= self.columns();
        let hidden = ahead.is_empty() || broken || wraps;
        if hidden {
            return runs_of(Vec::new());
        }

        vec![Run::new(self.console.line(), Colour::CLEAR), Run::new(&ahead, STONE.opaque())]
    }

    /// What is running, at the prompt's right end.
    pub(super) fn running_runs(&self) -> Vec<Run> {
        runs_of(self.console.running())
    }
}

/// What the chat's widgets raise (ADR-0057).
#[aethereal_ui::events]
impl Overlay {
    /// Opens the tab at `place`, the console's own first.
    pub(super) fn choose(&mut self, turn: &mut Turn<'_, Self, OverlaySends>, place: u32) -> Result<(), Fault> {
        let chat = self.chats.choose(place);

        self.tell(turn, chat)
    }

    /// Opens or closes the subagent's fold the open transcript's row keyed `key` is.
    #[allow(clippy::unnecessary_wraps, reason = "an event's method answers a fault, as the library calls it")]
    pub(super) fn fold(&mut self, _: &mut Turn<'_, Self, OverlaySends>, key: u64) -> Result<(), Fault> {
        self.chats.fold(key);

        Ok(())
    }

    /// Lets the ask numbered `ask` go ahead.
    pub(super) fn allow(&mut self, turn: &mut Turn<'_, Self, OverlaySends>, ask: u64) -> Result<(), Fault> {
        let answer = self.chats.answer(ask, true);

        self.tell(turn, vec![answer])
    }

    /// Refuses the ask numbered `ask`.
    pub(super) fn deny(&mut self, turn: &mut Turn<'_, Self, OverlaySends>, ask: u64) -> Result<(), Fault> {
        let answer = self.chats.answer(ask, false);

        self.tell(turn, vec![answer])
    }

    /// A link in Claude's markdown pressed: logged, as the overlay opens nothing.
    #[allow(clippy::unnecessary_wraps, reason = "an event's method answers a fault, as the library calls it")]
    #[allow(clippy::needless_pass_by_value, reason = "the text an event hands is taken as a `String`, its type")]
    #[allow(clippy::unused_self, reason = "an event's method is called on its screen")]
    pub(super) fn follow(&mut self, turn: &mut Turn<'_, Self, OverlaySends>, target: String) -> Result<(), Fault> {
        turn.log(Level::Info, format_args!("a link was pressed: {target}"));

        Ok(())
    }
}
