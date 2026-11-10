use super::{ChartStyle, RECORD_BYTES, marks, records};
use crate::colour::{BRICK, LICHEN, OCHRE, PANEL_OPACITY, RUST, SLATE, STONE};
use crate::frames::{Candle, Column, Slide};
use crate::layout::Layout;
use crate::stop::Stop;
use aethereal_render::stats::FrameBudget;
use aethereal_render::text::Colour;
use aethereal_window::Surface;

/// A budget of 16.7 milliseconds, in nanoseconds.
const BUDGET: u64 = 16_700_000;

/// That budget, as a limit the frames are held to.
const LIMIT: FrameBudget = FrameBudget::Limit { nanos: BUDGET };

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
    Column { span, candle: Candle::flat(nanos) }
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

    records(&mut band, layout, Stop::Band, ChartStyle::Bars, columns, slide, LIMIT);
    records(&mut console, layout, Stop::Console, ChartStyle::Bars, columns, slide, LIMIT);

    let (band, console) = (read(&band), read(&console));
    assert_eq!(console.len(), band.len() + 4);
    assert_eq!(band[0].colour, SLATE.at(PANEL_OPACITY));
    let shaded: Vec<Colour> = band[1..4].iter().map(|record| record.colour).collect();
    assert_eq!(shaded, vec![LICHEN.opaque(), RUST.opaque(), BRICK.opaque()]);
    let spike = band[3].area;
    assert_eq!((spike[1], spike[3]), (chart.min.y, chart.height()));
}

/// The box of each column drawn on `layout` from `columns` at `slide`, in order: the records after the band's panel,
/// before the budget line's dashes.
fn bars(layout: Layout, columns: &[Column], slide: Slide) -> Vec<[f32; 4]> {
    let mut bytes = Vec::new();
    records(&mut bytes, layout, Stop::Band, ChartStyle::Bars, columns.iter().copied(), slide, LIMIT);

    read(&bytes)[1..=columns.len()].iter().map(|record| record.area).collect()
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
        records(&mut bytes, layout, Stop::Band, ChartStyle::Candles, [], slide, budget);
        assert!(bytes.len() <= super::records_bytes(layout), "{budget:?} overflows its room");
        read(&bytes)[1..].to_vec()
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

/// The records of one candle, drawn a column's pitch left of the chart's right edge on `layout`: its wick, its body,
/// and its median, when it has one; the records between the band's panel and the budget line's stone dashes.
fn candle_records(layout: Layout, candle: Candle) -> Vec<Record> {
    let slide = Slide { newest: 6, through: 0.0, span_nanos: 1_000_000_000 };
    let mut bytes = Vec::new();
    records(&mut bytes, layout, Stop::Band, ChartStyle::Candles, [Column { span: 5, candle }], slide, LIMIT);
    let drawn = read(&bytes);
    let count = drawn.iter().skip(1).take_while(|record| record.colour != STONE.opaque()).count();

    drawn[1..=count].to_vec()
}

/// A candle's wick is a line a pixel wide in its column, from its slowest cycle down to its fastest; its body the
/// column's width from its 75th percentile down to its 25th; and its median a slate line across the body, so a wick
/// off the column's middle, a body of the wrong quartiles, or a median drawn outside the body is caught.
#[test]
fn a_candle_stands_from_its_fastest_to_its_slowest() {
    let layout = Layout::of(Surface { width: 1920, height: 1080, scale: 1.0 });
    let (chart, width) = (layout.chart(), layout.column_pixels());
    // The chart's top is three budgets, so a budget's share of it is a third.
    let at = |budgets: f32| (budgets / 3.0).mul_add(-chart.height(), chart.max.y);
    let budget = |share: f32| {
        // A share of the budget, which a u64 of nanoseconds holds closely enough.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
        let nanos = (BUDGET as f32 * share) as u64;
        nanos
    };
    let candle = Candle {
        fastest: budget(0.3),
        lower: budget(0.6),
        median: budget(0.7),
        upper: budget(0.9),
        slowest: budget(2.4),
    };

    let drawn = candle_records(layout, candle);

    assert_eq!(drawn.len(), 3, "{drawn:?} are not a wick, a body, and a median");
    let [wick, body, median] = [drawn[0].area, drawn[1].area, drawn[2].area];
    let left = chart.max.x - layout.column_pitch();
    let near = |a: f32, b: f32| (a - b).abs() <= 1.0;
    assert_eq!((wick[0], wick[2]), (left + ((width - 1.0) / 2.0).floor(), 1.0));
    assert!(near(wick[1], at(2.4)) && near(wick[1] + wick[3], at(0.3)), "{wick:?} is not fastest to slowest");
    assert_eq!((body[0], body[2]), (left, width));
    assert!(near(body[1], at(0.9)) && near(body[1] + body[3], at(0.6)), "{body:?} is not the quartiles");
    let (median_foot, body_foot) = (median[1] + median[3], body[1] + body[3]);
    let inside = median[1] >= body[1] && median_foot <= body_foot;
    assert!(inside && near(median[1], at(0.7)), "{median:?} is not across {body:?} at the median");
    assert_eq!((median[3], drawn[2].colour), (1.0, SLATE.opaque()));
}

/// A candle's body is shaded by its 75th percentile and its wick by its slowest cycle: one spike among healthy cycles
/// is a rust wick over a lichen body, and a stretch of slow cycles a rust body, so a body shaded by the spike, which
/// paints every column with one slow frame as slow, is caught.
#[test]
fn a_spike_shades_the_wick_and_a_slow_stretch_the_body() {
    let layout = Layout::of(Surface { width: 1920, height: 1080, scale: 1.0 });
    let healthy = BUDGET * 9 / 10;
    let spike = Candle { slowest: BUDGET * 3 / 2, ..Candle::flat(healthy) };
    let slow = Candle { upper: BUDGET * 3 / 2, slowest: BUDGET * 3 / 2, ..Candle::flat(healthy) };

    let (spiked, slowed) = (candle_records(layout, spike), candle_records(layout, slow));

    assert_eq!((spiked[0].colour, spiked[1].colour), (RUST.opaque(), LICHEN.opaque()));
    assert_eq!((slowed[0].colour, slowed[1].colour), (RUST.opaque(), RUST.opaque()));
}
