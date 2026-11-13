use super::scale::height_fraction;
use super::{Fill, marks, records};
use crate::colour::{BONE, LICHEN, OCHRE, SAND, SLATE_TEAL, STONE};
use crate::frames::{Column, Slide};
use crate::layout::Scale;
use aethereal_math::{Rect2, Vec2};
use aethereal_render::stats::FrameBudget;
use aethereal_render::text::Colour;

/// A budget of 16.7 milliseconds, in nanoseconds.
const BUDGET: u64 = 16_700_000;

/// That budget, as a limit the frames are held to.
const LIMIT: FrameBudget = FrameBudget::Limit { nanos: BUDGET };

/// How many boxes come before the columns: the line at twice the budget.
const BEFORE_COLUMNS: usize = 1;

/// The scale of a window of one pixel to the logical one.
const SCALE: Scale = Scale(1.0);

/// The chart's box on a window 1080 pixels tall at that scale: 956 pixels wide, as the left column leaves it beside
/// its gutter, and 65 tall.
const CHART: Rect2 = Rect2 { min: Vec2::new(10.0, 10.0), max: Vec2::new(966.0, 75.0) };

/// One box read back: its left, top, width, and height, and its colour.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub area: [f32; 4],
    pub colour: Colour,
}

/// The boxes `fills` holds, in order.
pub fn read(fills: &[Fill]) -> Vec<Record> {
    fills
        .iter()
        .map(|fill| Record {
            area: [fill.area.min.x, fill.area.min.y, fill.area.width(), fill.area.height()],
            colour: fill.colour,
        })
        .collect()
}

/// A column of one cycle `nanos` long, of `span`.
const fn flat(span: u64, nanos: u64) -> Column {
    Column { span, fastest_nanos: nanos, median_nanos: nanos, slowest_nanos: nanos }
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

/// Each of a column's three bars is the colour of its layer whatever the budget, so a column over the budget still
/// draws its median in lichen, and a spike of forty budgets is drawn no taller than the chart, so a bar shaded by the
/// budget, or a spike that spills over the band, is caught.
#[test]
fn layers_keep_their_colours_whatever_the_budget_and_are_held_to_the_chart() {
    let columns = [flat(2, BUDGET / 2), flat(1, BUDGET * 3 / 2), flat(0, BUDGET * 40)];
    let slide = Slide { newest: 2, through: 0.5, span_nanos: 1_000_000_000 };
    let mut fills = Vec::new();

    records(&mut fills, CHART, SCALE, columns, slide, LIMIT);

    let band = read(&fills);
    let shaded: Vec<Colour> = band[BEFORE_COLUMNS..][..9].iter().map(|record| record.colour).collect();
    let layers = [SAND.opaque(), LICHEN.opaque(), SLATE_TEAL.opaque()];
    assert_eq!(shaded, [layers, layers, layers].concat());
    let spike = band[BEFORE_COLUMNS + 6].area;
    assert_eq!((spike[1], spike[3]), (CHART.min.y, CHART.height()));
}

/// The box of each column drawn from `columns` at `slide`, in order: the first of each column's three boxes, after the
/// line at twice the budget, before the budget line's dashes.
fn bars(columns: &[Column], slide: Slide) -> Vec<[f32; 4]> {
    let mut fills = Vec::new();
    records(&mut fills, CHART, SCALE, columns.iter().copied(), slide, LIMIT);

    read(&fills)[BEFORE_COLUMNS..][..columns.len() * 3].iter().step_by(3).map(|record| record.area).collect()
}

/// Every column moves left by the share of a pitch the newest cycle came through its span, the newest growing out of
/// the chart's right edge and the oldest cut at its left edge, so a chart that slides the wrong way, hops by whole
/// columns, or spills past its box as it slides is caught.
#[test]
fn columns_slide_left_through_their_span() {
    let (chart, pitch) = (CHART, SCALE.column_pitch());
    let newest = u64::from(SCALE.columns_across(chart.width(), 3_000));
    let columns = [newest, newest - 1, 0].map(|span| flat(span, BUDGET));
    assert_eq!(newest, 191, "1080 does not hold 191 columns of 4 pixels and a gap");

    // At 1080 the chart is 956 pixels wide, left of the rows' gutter, 191 columns of 5 and 1 over, so the column 191
    // back is cut at the left edge once it has come half way through its span.
    let early = bars(&columns, Slide { newest, through: 0.5, span_nanos: 1_000_000_000 });
    let late = bars(&columns, Slide { newest, through: 0.75, span_nanos: 1_000_000_000 });

    let [newest_early, before_early, oldest_early] = [early[0], early[1], early[2]];
    let [newest_late, before_late, oldest_late] = [late[0], late[1], late[2]];
    assert_eq!((newest_early[0], newest_early[2]), (chart.max.x - pitch / 2.0, pitch / 2.0));
    assert_eq!((newest_late[0], newest_late[2]), (pitch.mul_add(-0.75, chart.max.x), pitch * 0.75));
    assert_eq!(before_early[0] - before_late[0], pitch / 4.0);
    assert_eq!((oldest_early[0], oldest_late[0]), (chart.min.x, chart.min.x));
    assert!(oldest_late[2] < oldest_early[2], "{oldest_late:?} does not slide off the left edge past {oldest_early:?}");
}

/// A budget that is a reference is drawn in a lighter dash than a limit, bone at half a limit's opacity and sparser, at
/// the same height, so a budget the frames are not held to does not read as one they are.
#[test]
fn a_reference_budget_is_drawn_lighter_than_a_limit_at_the_same_height() {
    let slide = Slide { newest: 0, through: 0.0, span_nanos: 1_000_000_000 };
    let line = |budget| {
        let mut fills = Vec::new();
        records(&mut fills, CHART, SCALE, [], slide, budget);
        read(&fills)[BEFORE_COLUMNS..].to_vec()
    };

    let limit = line(LIMIT);
    let reference = line(FrameBudget::Reference { nanos: BUDGET });

    assert!(limit.iter().all(|dash| dash.colour == BONE.at(0.7)));
    assert!(reference.iter().all(|dash| dash.colour == BONE.at(0.35)));
    assert_eq!(limit[0].area[1], reference[0].area[1]);
    assert!(reference[0].area[2] < limit[0].area[2], "a reference's dashes are shorter");
}

/// A mark's tick stands where its moment falls among the columns: a mark two and a half spans before the newest
/// moment is two and a half pitches left of where the newest moment is drawn, the chart's right edge; and a mark older
/// than the chart reaches is not drawn, so a tick that slides the wrong way, or at the wrong rate, is caught.
#[test]
fn a_mark_s_tick_stands_at_its_time() {
    let (chart, pitch) = (CHART, SCALE.column_pitch());
    let span_nanos = 1_000_000_000;
    // The newest moment is 300.25 spans in; the mark is at 297.75.
    let slide = Slide { newest: 300, through: 0.25, span_nanos };
    let mark = 297 * span_nanos + span_nanos * 3 / 4;
    let mut fills = Vec::new();

    marks(&mut fills, CHART, SCALE, slide, [mark, 0]);

    let ticks = read(&fills);
    assert_eq!(ticks.len(), 1, "a mark older than the chart is drawn");
    assert_eq!(ticks[0].colour, OCHRE.opaque());
    assert_eq!(ticks[0].area[0], pitch.mul_add(-2.5, chart.max.x));
    assert_eq!((ticks[0].area[1], ticks[0].area[3]), (chart.min.y, chart.height()));
}

/// Everything drawn with one column, a column's pitch left of the chart's right edge.
fn drawn_with(column: Column) -> Vec<Record> {
    let slide = Slide { newest: 6, through: 0.0, span_nanos: 1_000_000_000 };
    let mut fills = Vec::new();
    records(&mut fills, CHART, SCALE, [Column { span: 5, ..column }], slide, LIMIT);

    read(&fills)
}

/// On the log scale a column's three bars each stand the column's width from the chart's foot, back to front: its
/// slowest in sand, its median in lichen, and its fastest in slate-teal; the faint line at twice the budget stands
/// three sevenths up and the budget's dashes two, so a bar drawn on a linear scale, from the wrong cycle, narrower than
/// the column, or out of order, which hides a shorter bar behind a taller one, is caught.
#[test]
fn a_column_stands_its_three_bars_back_to_front_on_the_log_scale() {
    let (chart, width) = (CHART, SCALE.column_pixels());
    let at = |sevenths: f32| (sevenths / 7.0).mul_add(-chart.height(), chart.max.y);
    let near = |a: f32, b: f32| (a - b).abs() <= 1.0;
    // The fastest is half the budget, one seventh up; the median the budget, two sevenths; the slowest four budgets,
    // four sevenths.
    let column = Column { span: 0, fastest_nanos: BUDGET / 2, median_nanos: BUDGET, slowest_nanos: BUDGET * 4 };

    let drawn = drawn_with(column);

    let ours = &drawn[BEFORE_COLUMNS..][..3];
    let colours: Vec<Colour> = ours.iter().map(|record| record.colour).collect();
    assert_eq!(colours, vec![SAND.opaque(), LICHEN.opaque(), SLATE_TEAL.opaque()]);
    let left = chart.max.x - SCALE.column_pitch();
    for (bar, sevenths) in ours.iter().map(|record| record.area).zip([4.0, 2.0, 1.0]) {
        assert_eq!((bar[0], bar[2]), (left, width));
        assert!(near(bar[1], at(sevenths)) && near(bar[1] + bar[3], chart.max.y), "{bar:?} is not {sevenths}/7 up");
    }
    let twice = &drawn[BEFORE_COLUMNS - 1];
    assert_eq!((twice.colour, twice.area[0], twice.area[2]), (STONE.at(0.15), chart.min.x, chart.width()));
    assert!(near(twice.area[1], at(3.0)), "{twice:?} is not three sevenths up");
    let dash = &drawn[BEFORE_COLUMNS + 3];
    assert_eq!(dash.colour, BONE.at(0.7));
    assert!(near(dash.area[1], at(2.0)), "{dash:?} is not two sevenths up");
}
