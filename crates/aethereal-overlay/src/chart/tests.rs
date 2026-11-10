use super::{RECORD_BYTES, records};
use crate::colour::{BRICK, LICHEN, PANEL_OPACITY, RUST, SLATE};
use crate::frames::Column;
use crate::layout::Layout;
use crate::stop::Stop;
use aethereal_render::text::Colour;
use aethereal_window::Surface;

/// A budget of 16.7 milliseconds, in nanoseconds.
const BUDGET: u64 = 16_700_000;

/// One record read back: its box, left, top, width, and height, and its colour.
#[derive(Debug, PartialEq)]
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
        Column { at: 0, longest_nanos: BUDGET },
        Column { at: 1, longest_nanos: BUDGET + 1 },
        Column { at: 2, longest_nanos: BUDGET * 40 },
    ];
    let mut band = Vec::new();
    let mut console = Vec::new();

    records(&mut band, layout, Stop::Band, &columns, BUDGET);
    records(&mut console, layout, Stop::Console, &columns, BUDGET);

    let (band, console) = (read(&band), read(&console));
    assert_eq!(console.len(), band.len() + 1);
    assert_eq!(band[0].colour, SLATE.at(PANEL_OPACITY));
    let shaded: Vec<Colour> = band[1..4].iter().map(|record| record.colour).collect();
    assert_eq!(shaded, vec![LICHEN.opaque(), RUST.opaque(), BRICK.opaque()]);
    let spike = band[3].area;
    assert_eq!((spike[1], spike[3]), (chart.min.y, chart.height()));
}
