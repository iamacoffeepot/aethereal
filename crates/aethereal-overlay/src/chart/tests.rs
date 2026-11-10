use super::{RECORD_BYTES, records};
use crate::colour::{BRICK, LICHEN, PANEL_OPACITY, RUST, SLATE, STONE};
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

/// A column within the budget is lichen, one over it rust, and one over twice it brick and drawn no taller than the
/// chart, so a threshold off, or a spike that spills over the band, is caught. The panels come first, beneath the
/// columns, and the console's only at its stop.
#[test]
fn columns_are_shaded_by_the_budget_and_held_to_the_chart() {
    let layout = Layout::of(Surface { width: 1920, height: 1080, scale: 1.0 });
    let chart = layout.chart();
    let columns = [
        Column { span: 2, longest_nanos: BUDGET },
        Column { span: 1, longest_nanos: BUDGET + 1 },
        Column { span: 0, longest_nanos: BUDGET * 40 },
    ];
    let slide = Slide { newest: 2, through: 0.5 };
    let mut band = Vec::new();
    let mut console = Vec::new();

    records(&mut band, layout, Stop::Band, columns, slide, LIMIT);
    records(&mut console, layout, Stop::Console, columns, slide, LIMIT);

    let (band, console) = (read(&band), read(&console));
    assert_eq!(console.len(), band.len() + 1);
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
    records(&mut bytes, layout, Stop::Band, columns.iter().copied(), slide, LIMIT);

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
    let columns = [newest, newest - 1, 0].map(|span| Column { span, longest_nanos: BUDGET });
    assert_eq!(newest, 226, "1080 does not hold 226 columns of 4 pixels and a gap");

    // At 1080 the chart is 1132 pixels wide, 226 columns of 5 and 2 over, so the column 226 back is cut at the left
    // edge once it has come half way through its span.
    let early = bars(layout, &columns, Slide { newest, through: 0.5 });
    let late = bars(layout, &columns, Slide { newest, through: 0.75 });

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
    let slide = Slide { newest: 0, through: 0.0 };
    let line = |budget| {
        let mut bytes = Vec::new();
        records(&mut bytes, layout, Stop::Band, [], slide, budget);
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
