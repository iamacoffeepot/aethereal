use super::scale::height_fraction;
use super::{RECORD_BYTES, marks, records};
use crate::colour::{BRICK, LICHEN, OCHRE, PANEL_OPACITY, RUST, SLATE, STONE};
use crate::frames::{Column, Slide};
use crate::layout::Layout;
use crate::stop::Stop;
use aethereal_render::stats::FrameBudget;
use aethereal_render::text::Colour;
use aethereal_window::Surface;

/// A budget of 16.7 milliseconds, in nanoseconds.
const BUDGET: u64 = 16_700_000;

/// That budget, as a limit the frames are held to.
const LIMIT: FrameBudget = FrameBudget::Limit { nanos: BUDGET };

/// How many records come before the columns at the band's stop: the band's panel and the line at twice the budget.
const BEFORE_COLUMNS: usize = 2;

/// One record read back: its box, left, top, width, and height, and its colour.
#[derive(Debug, Clone, PartialEq)]
struct Record {
    area: [f32; 4],
    colour: Colour,
}

/// The records `bytes` holds, in order.
fn read(bytes: &[u8]) -> Vec<Record> {
    let float = |chunk: &[u8], at: usize| f32::from_ne_bytes([chunk[at], chunk[at + 1], chunk[at + 2], chunk[at + 3]]);
    bytes
        .chunks_exact(RECORD_BYTES as usize)
        .map(|chunk| {
            let [left, top, width, height, red, green, blue, alpha] =
                [0, 4, 8, 12, 16, 20, 24, 28].map(|at| float(chunk, at));
            Record { area: [left, top, width, height], colour: Colour { red, green, blue, alpha } }
        })
        .collect()
}

/// A column of one cycle `nanos` long, of `span`.
const fn flat(span: u64, nanos: u64) -> Column {
    Column { span, median_nanos: nanos, slowest_nanos: nanos }
}

/// A quarter of the budget or less stands at the chart's foot and thirty-two budgets or more at its top, the budget
/// two sevenths of the way up, and each doubling of a cycle's time a seventh higher, whatever the budget, so a scale
/// that is linear, spills past the chart, or is pinned to one budget is caught.
#[test]
fn the_scale_doubles_by_sevenths_from_a_quarter_budget_to_thirty_two() {
    let near = |a: f32, b: f32| (a - b).abs() < 1e-6;
    for budget in [BUDGET, 4_166_667] {
        assert_eq!(height_fraction(0, budget), 0.0);
        assert_eq!(height_fraction(budget / 8, budget), 0.0);
        assert!(near(height_fraction(budget / 4, budget), 0.0));
        assert!(near(height_fraction(budget * 32, budget), 1.0));
        assert_eq!(height_fraction(budget * 1_000, budget), 1.0);
        assert!(near(height_fraction(budget, budget), 2.0 / 7.0));
        for nanos in [budget / 3, budget, budget * 3 / 2, budget * 5] {
            let step = height_fraction(nanos * 2, budget) - height_fraction(nanos, budget);
            assert!(near(step, 1.0 / 7.0), "doubling {nanos} moves it {step}");
        }
    }
}

/// A bar within the budget is lichen, one over it rust, and one over twice it brick and drawn no taller than the
/// chart, so a threshold off, or a spike that spills over the band, is caught. The panels come first, beneath the
/// columns, and the console's strips and the lines between them only at its stop.
#[test]
fn columns_are_shaded_by_the_budget_and_held_to_the_chart() {
    let layout = Layout::of(Surface { width: 1920, height: 1080, scale: 1.0 });
    let chart = layout.chart();
    let columns = [flat(2, BUDGET), flat(1, BUDGET + 1), flat(0, BUDGET * 40)];
    let slide = Slide { newest: 2, through: 0.5, span_nanos: 1_000_000_000 };
    let mut band = Vec::new();
    let mut console = Vec::new();

    records(&mut band, layout, Stop::Band, columns, slide, LIMIT);
    records(&mut console, layout, Stop::Console, columns, slide, LIMIT);

    let (band, console) = (read(&band), read(&console));
    assert_eq!(console.len(), band.len() + 4);
    assert_eq!(band[0].colour, SLATE.at(PANEL_OPACITY));
    let shaded: Vec<Colour> = band[BEFORE_COLUMNS..BEFORE_COLUMNS + 3].iter().map(|record| record.colour).collect();
    assert_eq!(shaded, vec![LICHEN.opaque(), RUST.opaque(), BRICK.opaque()]);
    let spike = band[BEFORE_COLUMNS + 2].area;
    assert_eq!((spike[1], spike[3]), (chart.min.y, chart.height()));
}

/// The box of each column drawn on `layout` from `columns` at `slide`, in order: the records after the band's panel
/// and the line at twice the budget, before the budget line's dashes.
fn bars(layout: Layout, columns: &[Column], slide: Slide) -> Vec<[f32; 4]> {
    let mut bytes = Vec::new();
    records(&mut bytes, layout, Stop::Band, columns.iter().copied(), slide, LIMIT);

    read(&bytes)[BEFORE_COLUMNS..BEFORE_COLUMNS + columns.len()].iter().map(|record| record.area).collect()
}

/// Every column moves left by the share of a pitch the newest cycle came through its span, the newest growing out of
/// the chart's right edge and the oldest cut at its left edge, so a chart that slides the wrong way, hops by whole
/// columns, or spills past its box as it slides is caught.
#[test]
fn columns_slide_left_through_their_span() {
    let layout = Layout::of(Surface { width: 1920, height: 1080, scale: 1.0 });
    let (chart, pitch) = (layout.chart(), layout.column_pitch());
    let newest = u64::from(super::column_count(layout));
    let columns = [newest, newest - 1, 0].map(|span| flat(span, BUDGET));
    assert_eq!(newest, 226, "1080 does not hold 226 columns of 4 pixels and a gap");

    // At 1080 the chart is 1132 pixels wide, 226 columns of 5 and 2 over, so the column 226 back is cut at the left
    // edge once it has come half way through its span.
    let early = bars(layout, &columns, Slide { newest, through: 0.5, span_nanos: 1_000_000_000 });
    let late = bars(layout, &columns, Slide { newest, through: 0.75, span_nanos: 1_000_000_000 });

    let [newest_early, before_early, oldest_early] = [early[0], early[1], early[2]];
    let [newest_late, before_late, oldest_late] = [late[0], late[1], late[2]];
    assert_eq!((newest_early[0], newest_early[2]), (chart.max.x - pitch / 2.0, pitch / 2.0));
    assert_eq!((newest_late[0], newest_late[2]), (pitch.mul_add(-0.75, chart.max.x), pitch * 0.75));
    assert_eq!(before_early[0] - before_late[0], pitch / 4.0);
    assert_eq!((oldest_early[0], oldest_late[0]), (chart.min.x, chart.min.x));
    assert!(oldest_late[2] < oldest_early[2], "{oldest_late:?} does not slide off the left edge past {oldest_early:?}");
}

/// A budget that is a reference is drawn in a lighter dash than a limit, stone at half its opacity and sparser, at the
/// same height, so a budget the frames are not held to does not read as one they are; and its closer dashes fit in the
/// room the records are given.
#[test]
fn a_reference_budget_is_drawn_lighter_than_a_limit_at_the_same_height() {
    let layout = Layout::of(Surface { width: 1920, height: 1080, scale: 1.0 });
    let slide = Slide { newest: 0, through: 0.0, span_nanos: 1_000_000_000 };
    let line = |budget| {
        let mut bytes = Vec::new();
        records(&mut bytes, layout, Stop::Band, [], slide, budget);
        assert!(bytes.len() <= super::records_bytes(layout), "{budget:?} overflows its room");
        read(&bytes)[BEFORE_COLUMNS..].to_vec()
    };

    let limit = line(LIMIT);
    let reference = line(FrameBudget::Reference { nanos: BUDGET });

    assert!(limit.iter().all(|dash| dash.colour == STONE.opaque()));
    assert!(reference.iter().all(|dash| dash.colour == STONE.at(0.5)));
    assert_eq!(limit[0].area[1], reference[0].area[1]);
    assert!(reference[0].area[2] < limit[0].area[2], "a reference's dashes are shorter");
}

/// A mark's tick stands where its moment falls among the columns: a mark two and a half spans before the newest
/// moment is two and a half pitches left of where the newest moment is drawn, the chart's right edge; and a mark older
/// than the chart reaches is not drawn, so a tick that slides the wrong way, or at the wrong rate, is caught.
#[test]
fn a_mark_s_tick_stands_at_its_time() {
    let layout = Layout::of(Surface { width: 1920, height: 1080, scale: 1.0 });
    let (chart, pitch) = (layout.chart(), layout.column_pitch());
    let span_nanos = 1_000_000_000;
    // The newest moment is 300.25 spans in; the mark is at 297.75.
    let slide = Slide { newest: 300, through: 0.25, span_nanos };
    let mark = 297 * span_nanos + span_nanos * 3 / 4;
    let mut bytes = Vec::new();

    marks(&mut bytes, layout, slide, [mark, 0]);

    let ticks = read(&bytes);
    assert_eq!(ticks.len(), 1, "a mark older than the chart is drawn");
    assert_eq!(ticks[0].colour, OCHRE.opaque());
    assert_eq!(ticks[0].area[0], pitch.mul_add(-2.5, chart.max.x));
    assert_eq!((ticks[0].area[1], ticks[0].area[3]), (chart.min.y, chart.height()));
}

/// Everything drawn at the band's stop on `layout` with one column, a column's pitch left of the chart's right edge.
fn drawn_with(layout: Layout, column: Column) -> Vec<Record> {
    let slide = Slide { newest: 6, through: 0.0, span_nanos: 1_000_000_000 };
    let mut bytes = Vec::new();
    records(&mut bytes, layout, Stop::Band, [Column { span: 5, ..column }], slide, LIMIT);

    read(&bytes)
}

/// The records of one column: its bar, and its line when it has one; the records after the band's panel and the line
/// at twice the budget, before the budget line's stone dashes.
fn column_records(layout: Layout, column: Column) -> Vec<Record> {
    let drawn = drawn_with(layout, column);
    let count = drawn.iter().skip(BEFORE_COLUMNS).take_while(|record| record.colour != STONE.opaque()).count();

    drawn[BEFORE_COLUMNS..BEFORE_COLUMNS + count].to_vec()
}

/// On the log scale a column's bar stands the column's width from the chart's foot to its median, and its line a
/// pixel wide in the column's middle from the bar's top to its slowest; the faint line at twice the budget stands
/// three sevenths up and the budget's dashes two, so a bar or a line drawn on a linear scale, off the column's middle,
/// or from the wrong cycle, is caught.
#[test]
fn a_column_stands_its_median_and_reaches_its_slowest_on_the_log_scale() {
    let layout = Layout::of(Surface { width: 1920, height: 1080, scale: 1.0 });
    let (chart, width) = (layout.chart(), layout.column_pixels());
    let at = |sevenths: f32| (sevenths / 7.0).mul_add(-chart.height(), chart.max.y);
    let near = |a: f32, b: f32| (a - b).abs() <= 1.0;
    // The median is the budget, two sevenths up; the slowest four budgets, four sevenths up.
    let column = Column { span: 0, median_nanos: BUDGET, slowest_nanos: BUDGET * 4 };

    let drawn = drawn_with(layout, column);
    let ours = column_records(layout, column);

    assert_eq!(ours.len(), 2, "{ours:?} are not a bar and a line");
    let [bar, line] = [ours[0].area, ours[1].area];
    let left = chart.max.x - layout.column_pitch();
    assert_eq!((bar[0], bar[2]), (left, width));
    assert!(near(bar[1], at(2.0)) && near(bar[1] + bar[3], chart.max.y), "{bar:?} is not the foot to the median");
    assert_eq!((line[0], line[2]), (left + ((width - 1.0) / 2.0).floor(), 1.0));
    assert!(near(line[1], at(4.0)) && near(line[1] + line[3], bar[1]), "{line:?} is not the bar's top to the slowest");
    assert_eq!((ours[0].colour, ours[1].colour), (LICHEN.opaque(), BRICK.opaque()));
    let twice = &drawn[1];
    assert_eq!((twice.colour, twice.area[0], twice.area[2]), (STONE.at(0.15), chart.min.x, chart.width()));
    assert!(near(twice.area[1], at(3.0)), "{twice:?} is not three sevenths up");
    let dash = &drawn[BEFORE_COLUMNS + 2];
    assert!(near(dash.area[1], at(2.0)), "{dash:?} is not two sevenths up");
}

/// The line to the slowest cycle is drawn only when the slowest stands more than 15% above the median, so a column of
/// even cycles stays a plain bar and one with a real spike does not, and a threshold off, or a line always drawn, is
/// caught.
#[test]
fn the_line_is_drawn_only_past_fifteen_percent_over_the_median() {
    let layout = Layout::of(Surface { width: 1920, height: 1080, scale: 1.0 });
    let median_nanos = BUDGET / 2;
    let column = |slowest_nanos| Column { span: 0, median_nanos, slowest_nanos };

    let even = column_records(layout, column(median_nanos));
    let at_threshold = column_records(layout, column(median_nanos * 115 / 100));
    let past = column_records(layout, column(median_nanos * 115 / 100 + 1));

    assert_eq!((even.len(), at_threshold.len(), past.len()), (1, 1, 2));
}

/// A bar is shaded by its median and its line by its slowest cycle: one spike among healthy cycles is a rust line over
/// a lichen bar, and a stretch of slow cycles a rust bar, so a bar shaded by the spike, which paints every column with
/// one slow frame as slow, is caught.
#[test]
fn a_spike_shades_the_line_and_a_slow_stretch_the_bar() {
    let layout = Layout::of(Surface { width: 1920, height: 1080, scale: 1.0 });
    let spike = Column { span: 0, median_nanos: BUDGET * 9 / 10, slowest_nanos: BUDGET * 3 / 2 };
    let slow = Column { span: 0, median_nanos: BUDGET * 3 / 2, slowest_nanos: BUDGET * 8 / 5 };

    let (spiked, slowed) = (column_records(layout, spike), column_records(layout, slow));

    assert_eq!((spiked[0].colour, spiked[1].colour), (LICHEN.opaque(), RUST.opaque()));
    assert_eq!((slowed.len(), slowed[0].colour), (1, RUST.opaque()));
}
