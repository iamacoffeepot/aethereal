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
//! console's text. Every colour is a role of the overlay's palette, tinted by its theme (`colour`): each panel of the
//! console is a surface of the ladder, the sheet a step lighter than the band for the scrollback and the transcript,
//! raised again for the owner's lines, inset for the prompt, the grip, and the open tab, and deepest for the tabs'
//! strip, a closed tab shaded a step lighter while the pointer is over it; and every rule and divider is the one
//! divider ink. Each session's tab marks its status with a small shape before its title, which pulses while its
//! container comes up or a turn runs. Once the engine knows its own id, its name stands before the tabs as a label, and
//! the pointer resting on the `local` tag of its own lines shows its name and id.
//!
//! ```text
//! Column (the window)
//! ├─ Stack, at the band and the console's stops: Tint (the band's panel), Row:
//! │  ├─ Column (60%): Row (FrameChart, gutter), figures, rule, Row (GpuGraph, gutter), figures, rule, Row (LineRows, gutter)
//! │  ├─ Tint (the line between the columns)
//! │  └─ Column: a Row (MeterGraph, gutter) for each meter
//! └─ Column, at the console's stop (a third of the window): Grip (the seam), Scroll (the rows) or Scroll (a
//!    transcript), asks, rule, Row (> line…, running), rule, Row (own name, then the tabs: console ‖ ◦ session |
//!    • session)
//! ```

use super::{Calling, Overlay};
use crate::chart::{ChartShown, FrameChart};
use crate::chat::{Icon, Mine, Open, Parting, Row as TranscriptRow, Status, Title};
use crate::colour::{Ink, Palette, Surface};
use crate::console::{LOCAL, Piece, Row as PrintedRow, Shade};
use crate::gpu::{self, GpuGraph, GpuShown, GpuTiming, GraphTop};
use crate::layout::{
    DIVIDER_PIXELS, GAP_PIXELS, GRIP_PIXELS, LEFT_SHARE, Layout, METER_INSET_PIXELS, PADDING_PIXELS, SECTION_PIXELS,
    STRIP_PADDING_PIXELS, TAB_BREAK_PIXELS, TAB_DIVIDER_PIXELS, TAB_GAP_PIXELS, TAB_ICON_GAP_PIXELS, TAB_ICON_PIXELS,
    TAB_PADDING_PIXELS, TAB_SIDE_PIXELS, TEXT_PIXELS, advance_pixels, gutter_pixels, meter_gutter_pixels,
};
use crate::lines::{LineRows, LinesShown};
use crate::meters::{self, MOST_METER_COLUMNS, Meter, MeterDrawn, MeterGraph};
use crate::overlay::stats::MOST_COLUMNS;
use crate::widgets::{Grip, Printed, Shape, StatusIcon, TabButton, Tint, runs_of};
use aethereal_actor::{Asks, Fault, Level, Path, Tells, ToType, Turn};
use aethereal_link::Link;
use aethereal_render::Renderer;
use aethereal_render::stats::{SubscribeFrameStats, SubscribeGpuTimes, UnsubscribeFrameStats, UnsubscribeGpuTimes};
use aethereal_render::text::{Colour, Font, Run};
use aethereal_sessions::{Chat, engine_words};
use aethereal_tools::{CallTool, Tools};
use aethereal_ui::markdown::{Images, Inks, MarkdownBlock, Prose};
use aethereal_ui::prelude::*;
use aethereal_ui::widgets::DELAY;
use core::f64::consts::TAU;

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

/// How opaque the grip's handle is drawn, dim: a mark that says the seam is taken hold of, quieter than the text.
const HANDLE_OPACITY: f32 = 0.35;

/// How opaque the lines either side of a divider's label are drawn, in the mark's ink: thin beside the label, which
/// carries the reading.
const MARK_LINE_OPACITY: f32 = 0.35;

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

/// The padding inside a scrollback's row: either side of one of the owner's lines, as a transcript's turn is inset, so
/// its text stands clear of its shade's edge, and none around any other. A line the owner entered is printed as one
/// row for each line it wraps to, so none is padded above or below, which would part its rows.
fn sides(row: &Shown<'_>) -> Edges<Space> {
    match row.shade() {
        Shade::Raised => padded(GAP_PIXELS, GAP_PIXELS, 0.0, 0.0),
        Shade::Sheet => padded(0.0, 0.0, 0.0, 0.0),
    }
}

/// How long a status icon's pulse takes, from its brightest round to its brightest again, in nanoseconds.
const PULSE_NANOS: u64 = 1_400_000_000;

/// How opaque a pulsing status icon is at its faintest: still seen, so the tab never looks unmarked.
const PULSE_FAINTEST: f32 = 0.3;

/// How opaque a pulsing status icon is at `nanos` of the program's time: from opaque down to its faintest and back,
/// smoothly, once each pulse.
fn pulse(nanos: u64) -> f32 {
    // Nanoseconds within one pulse, under two seconds, which an f64 holds exactly enough to draw by.
    #[allow(clippy::cast_precision_loss)]
    let phase = (nanos % PULSE_NANOS) as f64 / PULSE_NANOS as f64;
    // A share from nothing to one, which an f32 holds.
    #[allow(clippy::cast_possible_truncation)]
    let lit = 0.5f64.mul_add((phase * TAU).cos(), 0.5) as f32;

    (1.0 - PULSE_FAINTEST).mul_add(lit, PULSE_FAINTEST)
}

impl Status {
    /// The shape it is drawn as, and in which ink: a ring coming up, idle, or compacting, half filled while it
    /// compacts; a dot while a turn runs, or an ask waits; a dash stopped; and a square lost.
    const fn drawn(self) -> (Shape, Ink) {
        match self {
            Self::Starting => (Shape::Ring, Ink::Lichen),
            Self::Idle => (Shape::Ring, Ink::Dim),
            Self::Working => (Shape::Dot, Ink::Lichen),
            Self::Compacting => (Shape::HalfRing, Ink::Lichen),
            Self::Waiting => (Shape::Dot, Ink::Mark),
            Self::Stopped => (Shape::Dash, Ink::Faint),
            Self::Lost => (Shape::Square, Ink::Error),
        }
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

    /// The space either side of the line beyond the gap between it and its tabs: room either side of the break, so it
    /// stands the padding from each, and none beside a light divider, whose tabs' own padding parts it from their
    /// titles.
    fn margin(self) -> Edges<Length> {
        let beside = PADDING_PIXELS - TAB_GAP_PIXELS;

        match self {
            Self::Break => margined(beside, beside, 0.0, 0.0),
            Self::First | Self::Light => margined(0.0, 0.0, 0.0, 0.0),
        }
    }
}

aethereal_ui::style! {
    /// The window, which the overlay's root fills, drawing nothing, so the pointer passes through it.
    pub whole { width: Length::Share(1.0), height: Length::Share(1.0) }
    /// The overlay's text: IBM Plex Mono at the band's size. Its colour is the palette's, set where it is used.
    pub mono { font: Font::Mono, font_size: TEXT_PIXELS }
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
    /// The band's panel, so the program shows behind it.
    pub band_panel {}
    /// The line between two sections of the left column, centred in the section's space between them.
    pub rule_across {
        height: Length::Pixels(DIVIDER_PIXELS),
        margin: spaced(SECTION_PIXELS / 2.0, SECTION_PIXELS / 2.0 - DIVIDER_PIXELS),
    }
    /// The line between the band's two columns.
    pub rule_down { width: Length::Pixels(DIVIDER_PIXELS), shrink: 0.0 }
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
    /// The seam between the band and the console, a grip a few pixels tall for dragging over its whole width: an inset
    /// well under the shadow the console's sheet casts on the band, with a short dim handle centred in it to say it is
    /// taken hold of.
    pub grip { height: Length::Pixels(GRIP_PIXELS) }
    /// The scrollback: the rest of the console above the prompt's strip, on the sheet, a step lighter than the band, its
    /// rows inside its padding and its bar, dim, at its right edge.
    pub scrollback { grow: 1.0, padding: padded(PADDING_PIXELS, PADDING_PIXELS, PADDING_PIXELS, GAP_PIXELS) }
    /// One row of the scrollback, across it: on the sheet, or the raised band of a line the owner entered.
    pub printed_row { width: Length::Share(1.0) }
    /// A session's transcript: as the scrollback is, its rows a small gap apart.
    pub transcript: scrollback { gap: Gap { rows: Space::Pixels(GAP_PIXELS), columns: Space::Pixels(0.0) } }
    /// One row of a transcript, across it, at least a line of text tall, which the scroll windows its rows by; its
    /// background and padding its own, the owner's lines set apart from Claude's on the raised band.
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
    /// A divider across a transcript: a line either side of its label, centred down the row.
    pub divided {
        width: Length::Share(1.0),
        align_items: Alignment::Center,
        gap: Gap { rows: Space::Pixels(0.0), columns: Space::Pixels(SECTION_PIXELS) },
    }
    /// The line either side of a divider's label, sharing the row's width.
    pub divider_line { grow: 1.0, height: Length::Pixels(DIVIDER_PIXELS) }
    /// A divider's label, unbroken.
    pub divider_label: mono { shrink: 0.0 }
    /// The fold of a subagent's events, pressed to open or close it.
    pub fold { align_self: SelfAlign::Own(Alignment::Start) }
    /// The fold's text.
    pub folded: mono {}
    /// Claude's text, as markdown.
    pub reply { width: Length::Share(1.0) }
    /// The asks of the open session that wait on the owner, above the prompt's strip, inset as the prompt is.
    pub asks { padding: padded(PADDING_PIXELS, PADDING_PIXELS, GAP_PIXELS, GAP_PIXELS) }
    /// One ask, its answers at its end.
    pub ask_row { align_items: Alignment::Center, gap: Gap { rows: Space::Pixels(0.0), columns: Space::Pixels(SECTION_PIXELS) } }
    /// What an ask says, as wide as the row leaves it.
    pub asked: mono { grow: 1.0 }
    /// An ask's answer, pressed.
    pub answer { padding: padded(SECTION_PIXELS, SECTION_PIXELS, 2.0, 2.0) }
    /// The session tabs beneath the prompt's strip, nearest the scene: a surface of their own, the deepest of the
    /// console's, so the prompt and the tabs never merge. It has no padding above or below, so the tabs fill its height,
    /// and a small gap between its tabs.
    pub tab_strip { padding: padded(PADDING_PIXELS, PADDING_PIXELS, 0.0, 0.0), align_items: Alignment::Stretch }
    /// This engine's own name at the strip's start, once it knows its id: a label, not a tab, in the label's ink,
    /// centred down the strip and set apart from the console's tab by the room the break between the console's tab
    /// and the sessions' takes.
    pub own_name: mono {
        shrink: 0.0,
        align_self: SelfAlign::Own(Alignment::Center),
        margin: margined(0.0, PADDING_PIXELS.mul_add(2.0, DIVIDER_PIXELS), 0.0, 0.0),
    }
    /// The tabs, a small gap between each.
    pub tab_row {
        align_items: Alignment::Stretch,
        gap: Gap { rows: Space::Pixels(0.0), columns: Space::Pixels(TAB_GAP_PIXELS) },
    }
    /// What the pointer resting on `local` shows: this engine's name and id, on the raised surface.
    pub own_tip { padding: padded(SECTION_PIXELS, SECTION_PIXELS, GAP_PIXELS, GAP_PIXELS) }
    /// The hairline along the tabs' strip's top, parting it from the prompt.
    pub tab_strip_rule { height: Length::Pixels(DIVIDER_PIXELS) }
    /// A tab and what parts it from the one before, the full height of the strip, the gap between them as between
    /// two tabs.
    pub tab_item {
        align_items: Alignment::Stretch,
        gap: Gap { rows: Space::Pixels(0.0), columns: Space::Pixels(TAB_GAP_PIXELS) },
    }
    /// What parts a tab from the one before: a line, its height and space as its parting says, centred against the
    /// tabs' titles, so it stands in from the strip's top and foot.
    pub tab_rule { width: Length::Pixels(DIVIDER_PIXELS), align_self: SelfAlign::Own(Alignment::Center), shrink: 0.0 }
    /// A tab, pressed to open it: its label, and, drawn by the tab, the shade the pointer over it casts and the open
    /// tab's line along its bottom edge.
    pub tab_button {}
    /// A tab's label, inside the tab's padding, which fills the strip's height: a session's status icon, then its title.
    pub tab_label {
        padding: padded(TAB_SIDE_PIXELS, TAB_SIDE_PIXELS, TAB_PADDING_PIXELS, TAB_PADDING_PIXELS),
        align_items: Alignment::Center,
        gap: Gap { rows: Space::Pixels(0.0), columns: Space::Pixels(TAB_ICON_GAP_PIXELS) },
    }
    /// A session's status icon: a fixed slot, so its title never moves as the icon changes.
    pub tab_icon { width: Length::Pixels(TAB_ICON_PIXELS), height: Length::Pixels(TAB_ICON_PIXELS), shrink: 0.0 }
    /// A tab's title.
    pub tab_text: mono {}
    /// The line between the scrollback and the prompt's strip.
    pub prompt_rule { height: Length::Pixels(DIVIDER_PIXELS) }
    /// The prompt's strip: a line of text tall inside its padding, an inset well a step darker than the band.
    pub prompt_strip {
        padding: padded(PADDING_PIXELS, PADDING_PIXELS, STRIP_PADDING_PIXELS, STRIP_PADDING_PIXELS),
        align_items: Alignment::Center,
    }
    /// The line typed.
    pub typed: mono {}
    /// The prompt before the line, a character's space after it, as a text measures no space at its end.
    pub sign: typed { padding: padded(0.0, advance_pixels(), 0.0, 0.0) }
    /// The line, as wide as the strip leaves it.
    pub line { grow: 1.0 }
}

aethereal_ui::view! { Overlay, ui,
    Column { style: whole,
        Stack { show: self.shows_band(),
            Tint { style: band_panel { colour: self.surface(Surface::Band) } }
            Row { style: band,
                Column { style: left,
                    Row {
                        FrameChart { style: graph, shown: self.chart(), tall: self.chart_tall(), palette: self.palette }
                        Column { style: gutter,
                            Printed { style: mono, runs: self.median_runs() }
                            Printed { style: mono, runs: self.alerts_runs() }
                        }
                    }
                    Printed { style: figures, runs: self.figures_runs() }
                    Tint { style: rule_across { colour: self.ink(Ink::Divider) } }
                    Row { show: self.gpu_timed(),
                        GpuGraph { style: graph, shown: self.gpu_graph(), tall: self.gpu_tall(), palette: self.palette }
                        Column { style: gutter,
                            Printed { style: mono, runs: self.gpu_top_runs() }
                        }
                    }
                    Printed { style: figures, runs: self.gpu_figures_runs() }
                    Tint { style: rule_across { colour: self.ink(Ink::Divider) } }
                    Row {
                        LineRows { style: graph, shown: self.line_rows(), palette: self.palette }
                        Column { style: gutter,
                            Printed { style: mono, runs: self.lines_runs() }
                        }
                    }
                }
                Tint { style: rule_down { colour: self.ink(Ink::Divider) } }
                Column { style: right, items: self.meters() as meter key meter.number,
                    Row { style: meter_row,
                        MeterGraph { style: meter_graph, drawn: meter.drawn, palette: self.palette }
                        Printed { style: meter_gutter, runs: meter.runs }
                    }
                }
            }
        }
        Column { style: console { height: Length::Share(self.console_share) }, show: self.shows_console(),
            Grip {
                style: grip { background: self.surface(Surface::Inset), colour: self.faded(Ink::Dim, HANDLE_OPACITY) },
                change => self.resize(),
            }
            Scroll { ..,
                style: scrollback { background: self.surface(Surface::Sheet), colour: self.ink(Ink::Dim) },
                show: self.shows_scrollback(),
                keeps: Keeps::End,
                items: self.rows() as row key row.number(),
                Column { style: printed_row { background: self.printed_shade(row), padding: sides(row) },
                    Printed { style: mono, runs: self.printed_runs(row) }
                }
            }
            Scroll { ..,
                style: transcript { background: self.surface(Surface::Sheet), colour: self.ink(Ink::Dim) },
                show: self.shows_transcript(),
                keeps: Keeps::End,
                items: self.chats.rows() as row key row.key,
                Column { style: entry { background: self.entry_shade(row), padding: inset(row) },
                    Selectable { show: row.is_line(), style: said { colour: self.entry_colour(row) }, text: row.text() }
                    Row { show: row.is_turn(), style: turn,
                        Printed { show: row.is_other_turn(), style: tag, runs: self.tag_runs(row) }
                        Tooltip { show: row.is_own_turn(), side: Side::Above, delay: DELAY,
                            Printed { style: tag, runs: self.tag_runs(row) }
                            Column { style: own_tip { background: self.surface(Surface::Raised) },
                                Text { style: mono { colour: self.ink(Ink::Text) }, "{self.local_tip()}" }
                            }
                        }
                        Selectable { style: turn_text { colour: self.entry_colour(row) }, text: row.text() }
                    }
                    Row { show: row.is_divider(), style: divided,
                        Tint { style: divider_line { colour: self.faded(Ink::Mark, MARK_LINE_OPACITY) } }
                        Text { style: divider_label { colour: self.ink(Ink::Mark) }, "{row.text()}" }
                        Tint { style: divider_line { colour: self.faded(Ink::Mark, MARK_LINE_OPACITY) } }
                    }
                    Button { show: row.is_fold(), style: fold, click => self.fold(row.key),
                        Text { style: folded { colour: self.ink(Ink::Dim) }, "{row.text()}" }
                    }
                    Column { style: reply, items: self.chats.reply(row) as block key block.index,
                        MarkdownBlock { memo, block: block, prose: self.prose(), images: self.images(), link => self.follow() }
                    }
                }
            }
            Column {
                style: asks { background: self.surface(Surface::Inset) },
                show: self.asking(),
                items: self.chats.asks() as ask key ask.ask,
                Row { style: ask_row,
                    Text { style: asked { colour: self.ink(Ink::Mark) }, "session {ask.session} asks: {ask.what}" }
                    Button { style: answer { background: self.surface(Surface::Sheet) }, click => self.allow(ask.ask),
                        Text { style: mono { colour: self.ink(Ink::Text) }, "allow" }
                    }
                    Button { style: answer { background: self.surface(Surface::Sheet) }, click => self.deny(ask.ask),
                        Text { style: mono { colour: self.ink(Ink::Text) }, "deny" }
                    }
                }
            }
            Tint { style: prompt_rule { colour: self.ink(Ink::Divider) } }
            Row { style: prompt_strip { background: self.surface(Surface::Inset) },
                Text { style: sign { colour: self.ink(Ink::Input) }, ">" }
                Stack { style: line,
                    TextInput {
                        style: typed { colour: self.ink(Ink::Input) },
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
            Tint { style: tab_strip_rule { colour: self.ink(Ink::Divider) }, show: self.chats.is_listed() }
            Row {
                style: tab_strip { background: self.surface(Surface::Deepest) },
                show: self.chats.is_listed(),
                Text { style: own_name { colour: self.ink(Ink::Label) }, show: self.knows_itself(), "{self.own_name()}" }
                Row { style: tab_row, items: self.chats.titles() as tab key tab.place,
                Row { style: tab_item,
                    Tint {
                        show: tab.parting.parts(),
                        style: tab_rule {
                            height: tab.parting.height(),
                            margin: tab.parting.margin(),
                            colour: self.ink(Ink::Divider),
                        },
                    }
                    TabButton {
                        style: tab_button { background: self.tab_background(tab) },
                        open: tab.open,
                        palette: self.palette,
                        click => self.choose(tab.place),
                        Row { style: tab_label,
                            StatusIcon {
                                style: tab_icon,
                                show: tab.icon.marks(),
                                shape: self.tab_shape(tab),
                                colour: self.tab_icon_colour(tab),
                            }
                            Text { style: tab_text { colour: self.tab_colour(tab) }, "{tab.name}" }
                        }
                    }
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

    /// Its runs, in `palette`.
    pub fn runs(&self, palette: &Palette) -> Vec<Run> {
        match self {
            Self::Printed(row) => runs_of(palette, row.pieces.iter().cloned()),
            Self::Blank(_) => runs_of(palette, Vec::new()),
        }
    }

    /// What it is drawn on: a blank one on the sheet.
    pub const fn shade(&self) -> Shade {
        match self {
            Self::Printed(row) => row.shade,
            Self::Blank(_) => Shade::Sheet,
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

    /// `surface` in the overlay's palette, at its opacity.
    pub(super) const fn surface(&self, surface: Surface) -> Colour {
        self.palette.surface(surface)
    }

    /// `ink` in the overlay's palette, opaque.
    pub(super) const fn ink(&self, ink: Ink) -> Colour {
        self.palette.ink(ink)
    }

    /// `ink` in the overlay's palette, at `alpha`.
    pub(super) const fn faded(&self, ink: Ink, alpha: f32) -> Colour {
        self.palette.faded(ink, alpha)
    }

    /// What a row drawn on `shade` is filled with: nothing on the sheet, which its panel draws, and the raised surface
    /// for the owner's lines.
    const fn shaded(&self, shade: Shade) -> Colour {
        match shade {
            Shade::Sheet => Colour::CLEAR,
            Shade::Raised => self.palette.surface(Surface::Raised),
        }
    }

    /// What the scrollback's row `row` is filled with.
    pub(super) const fn printed_shade(&self, row: &Shown<'_>) -> Colour {
        self.shaded(row.shade())
    }

    /// The scrollback's row `row`'s runs.
    pub(super) fn printed_runs(&self, row: &Shown<'_>) -> Vec<Run> {
        row.runs(&self.palette)
    }

    /// What the transcript's row `row` is filled with.
    pub(super) const fn entry_shade(&self, row: &TranscriptRow) -> Colour {
        self.shaded(row.shade())
    }

    /// The colour of the transcript's row `row`'s text.
    pub(super) const fn entry_colour(&self, row: &TranscriptRow) -> Colour {
        self.palette.ink(row.ink())
    }

    /// The runs of the tag of the transcript's row `row`.
    pub(super) fn tag_runs(&self, row: &TranscriptRow) -> Vec<Run> {
        row.tag_runs(&self.palette)
    }

    /// What the tab `tab` is drawn on.
    pub(super) const fn tab_background(&self, tab: &Title) -> Colour {
        tab.background(&self.palette)
    }

    /// The colour of the tab `tab`'s title.
    pub(super) const fn tab_colour(&self, tab: &Title) -> Colour {
        self.palette.ink(tab.ink())
    }

    /// Whether this engine has learned its own id, so its name shows at the tabs' start.
    pub(super) const fn knows_itself(&self) -> bool {
        matches!(self.chats.mine(), Mine::Engine(_))
    }

    /// This engine's own name, its words alone: none until it has learned its id, when it does not show.
    pub(super) fn own_name(&self) -> String {
        match self.chats.mine() {
            Mine::Engine(engine) => engine_words(engine),
            Mine::Unknown => String::new(),
        }
    }

    /// What the pointer resting on the `local` tag of this engine's own lines shows: `local`, its name, and its id,
    /// the one place in the chat its id shows. A line is tagged `local` only once the id is learned.
    pub(super) fn local_tip(&self) -> String {
        match self.chats.mine() {
            Mine::Engine(engine) => format!("{LOCAL} · {} · engine {engine}", engine_words(engine)),
            Mine::Unknown => String::from(LOCAL),
        }
    }

    /// The shape of the tab `tab`'s status icon: blank for the console's own tab.
    #[allow(clippy::unused_self, reason = "markup reads a computed value through a `&self` method")]
    pub(super) const fn tab_shape(&self, tab: &Title) -> Shape {
        match tab.icon {
            Icon::Marked(status) => status.drawn().0,
            Icon::Unmarked => Shape::Blank,
        }
    }

    /// The colour of the tab `tab`'s status icon now: its ink, at the opacity of the pulse at the program's time when
    /// its status pulses; clear for the console's own tab, which shows none.
    pub(super) fn tab_icon_colour(&self, tab: &Title) -> Colour {
        let Icon::Marked(status) = tab.icon else {
            return Colour::CLEAR;
        };
        let ink = status.drawn().1;
        if status.pulses() {
            return self.faded(ink, pulse(self.now_nanos));
        }

        self.ink(ink)
    }

    /// `pieces` as a text's runs, each in its ink of the overlay's palette.
    fn painted(&self, pieces: impl IntoIterator<Item = Piece>) -> Vec<Run> {
        runs_of(&self.palette, pieces)
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
    /// IBM Plex Mono, and its body at the console's size, so a reply reads as every other line of the console does;
    /// its text in the palette's text ink, its code in the median's, its links in the link's, and its rules and quote
    /// bars in the divider ink.
    pub(super) fn prose(&self) -> Prose {
        let inks = Inks::default();
        let text = self.ink(Ink::Text);
        let body = |style: Style| Style { font: Font::Mono, font_size: TEXT_PIXELS, colour: text, ..style };
        let heading = |style: Style| Style { font: Font::Mono, colour: text, ..style };
        let [first, second, third] = inks.headings;
        let code = self.ink(Ink::Median);
        let divider = self.ink(Ink::Divider);

        Prose::new(Inks {
            paragraph: body(inks.paragraph),
            headings: [heading(first), heading(second), heading(third)],
            code: Style { colour: code, ..body(inks.code) },
            marker: body(inks.marker),
            head_cell: body(inks.head_cell),
            cell: body(inks.cell),
            rule: Style { background: divider, ..inks.rule },
            quote_bar: Style { background: divider, ..inks.quote_bar },
            inline_code: code,
            link: self.ink(Ink::Link),
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
        self.painted(self.figures.gutter_pieces())
    }

    /// The gutter's line beneath the median: the warnings and errors since the console was last opened, and how many
    /// sessions wait on the owner, in the mark's ink, when any does (ADR-0057).
    pub(super) fn alerts_runs(&self) -> Vec<Run> {
        let mut pieces = self.alerts.pieces();
        let waiting = self.chats.waiting();
        if waiting > 0 {
            pieces.push(Piece::new(&format!(" {waiting}?"), Ink::Mark));
        }

        self.painted(pieces)
    }

    /// The figures beneath the frame chart.
    pub(super) fn figures_runs(&self) -> Vec<Run> {
        self.painted(self.figures.pieces())
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
        self.painted(GraphTop::of(self.gpu_graph().columns).pieces())
    }

    /// The figures beneath the GPU's graph, or in its place that the device has no timestamps.
    pub(super) fn gpu_figures_runs(&self) -> Vec<Run> {
        let pieces = match self.gpu {
            GpuTiming::Timed(_) => self.gpu_figures.pieces(),
            GpuTiming::Unsupported => gpu::unsupported(),
        };

        self.painted(pieces)
    }

    /// What the lines' rows show: their columns as last cut, and where they stand.
    pub(super) fn line_rows(&self) -> LinesShown {
        let count = self.layout().chart_columns(MOST_COLUMNS);

        LinesShown { columns: self.lines.columns().collect(), slide: self.frames.slide(count) }
    }

    /// The gutter's lines beside the lines' rows: each label and its newest value.
    pub(super) fn lines_runs(&self) -> Vec<Run> {
        self.painted(self.lines.pieces())
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
                runs: self.painted(self.memory.row_pieces(meter, gpu_bytes)),
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

    /// The completion of the word before the caret, faint, set after the line as typed, which is drawn clear, so the
    /// completion stands just ahead of the line in the mono face: none once the line runs onto a second line, where the
    /// line wraps as its text input lays it out and the hint would not.
    pub(super) fn hint_runs(&self) -> Vec<Run> {
        let ahead = self.console.completion_ahead(self.caret());
        let line = self.console.line();
        let broken = line.contains('\n');
        let wraps = line.chars().count() + ahead.chars().count() >= self.columns();
        let hidden = ahead.is_empty() || broken || wraps;
        if hidden {
            return self.painted(Vec::new());
        }

        vec![Run::new(self.console.line(), Colour::CLEAR), self.palette.run(&ahead, Ink::Faint)]
    }

    /// What is running, at the prompt's right end.
    pub(super) fn running_runs(&self) -> Vec<Run> {
        self.painted(self.console.running())
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
