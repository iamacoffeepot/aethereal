use super::bars::fill_swatch;
use super::history::MOST_METER_COLUMNS;
use super::{EngineMemory, Held, MemoryFigures, Meter, MeterHistories, ProcessMemory, rows};
use crate::chart::columns_across;
use crate::chart::tests::{Record, read};
use crate::colour::{BRICK, LICHEN, RUST, STONE};
use crate::frames::{Slide, span_nanos};
use crate::layout::Layout;
use aethereal_math::{Rect2, Vec2};
use aethereal_render::stats::{Alive, AtlasFill, FrameBudget, FrameDrawn, FrameStats, GpuMemory};
use aethereal_window::Surface;

/// A second of the program's time, in nanoseconds.
const SECOND: u64 = 1_000_000_000;

/// The engine's memory with the program at `process`, the blob store holding `blobs`, a backlog of `backlog`, and no
/// guests.
const fn read_with(process: ProcessMemory, blobs: Held, backlog: Held) -> EngineMemory {
    EngineMemory::Read(MemoryFigures { process, blobs, backlog, guest_bytes: 0 })
}

/// The statistics of a frame of the cycle at `elapsed_nanos` that holds a byte, two, three, four, and five of each
/// kind of GPU memory.
fn stats(elapsed_nanos: u64) -> FrameStats {
    FrameStats {
        elapsed_nanos,
        budget: FrameBudget::UNTOLD,
        drawn: FrameDrawn { draws: 0, triangles: 0, uploaded_bytes: 0 },
        memory: GpuMemory { meshes_bytes: 1, textures_bytes: 2, targets_bytes: 3, buffers_bytes: 4, atlas_bytes: 5 },
        alive: Alive::default(),
        atlas: AtlasFill::default(),
    }
}

/// The slide of a graph cut into `count` columns when the program's time is `elapsed_nanos`.
fn slide_at(elapsed_nanos: u64) -> impl Fn(u32) -> Slide {
    move |count| {
        let span_nanos = span_nanos(count);
        // Nanoseconds of a span, at most a minute, which an f32 holds closely enough to draw.
        #[allow(clippy::cast_precision_loss)]
        let through = (elapsed_nanos % span_nanos) as f32 / span_nanos as f32;
        Slide { newest: elapsed_nanos / span_nanos, through, span_nanos }
    }
}

/// `area` as a record reads it back.
const fn boxed(area: Rect2) -> [f32; 4] {
    [area.min.x, area.min.y, area.max.x - area.min.x, area.max.y - area.min.y]
}

/// A fill is lichen under 75% of its most, rust from 75%, and brick from 90%, so a threshold set a step off the
/// ADR's table, or a percentage worked out in the wrong order, is caught.
#[test]
fn a_fill_turns_rust_at_three_quarters_and_brick_at_nine_tenths() {
    let at = |bytes| fill_swatch(Held { bytes, most: 100 });

    assert_eq!([at(74), at(75), at(89), at(90), at(250)], [LICHEN, RUST, RUST, BRICK, BRICK]);
}

/// Each meter with a limit draws its trough in faint stone across its graph's room, left of its row's gutter, and its
/// fill over it from the trough's left, as wide a share as the bytes are of their
/// most and never past the trough; a meter with no most draws its trough alone; and the meters with no limit draw
/// their troughs alone while they have read nothing. So a fill that runs out of its trough when an actor holds more than its most,
/// one scaled against the wrong figure, a trough that keeps a fixed width rather than filling its row, or a fill drawn
/// for a backlog that is none, is caught.
#[test]
fn a_bar_s_fill_is_its_share_of_the_trough_and_the_trough_fills_its_row() {
    let layout = Layout::of(Surface { width: 1920, height: 1080, scale: 1.0 });
    let process = ProcessMemory::Unknown;
    let memory = read_with(process, Held { bytes: 25, most: 100 }, Held { bytes: 300, most: 100 });
    let empty = read_with(process, Held { bytes: 0, most: 100 }, Held { bytes: 0, most: 0 });
    let (mut records, mut empty_records) = (Vec::new(), Vec::new());

    rows(&mut records, layout, &memory, &mut MeterHistories::default(), slide_at(0));
    rows(&mut empty_records, layout, &empty, &mut MeterHistories::default(), slide_at(0));

    let drawn = read(&records);
    let (blobs, backlog) = (layout.meter_graph(Meter::Blobs.index()), layout.meter_graph(Meter::Backlog.index()));
    assert_eq!(drawn.len(), 7, "{drawn:?} are not three troughs of history and two bars");
    assert_eq!(drawn[1].area, boxed(blobs));
    assert_eq!(drawn[1].colour, STONE.at(0.08));
    assert!((drawn[2].area[2] - blobs.width() / 4.0).abs() < 0.01, "{:?} is not a quarter of {blobs:?}", drawn[2]);
    assert_eq!(drawn[2].colour, LICHEN.opaque());
    assert_eq!(drawn[3].area, boxed(backlog));
    assert_eq!(drawn[4].colour, BRICK.opaque());
    let fills_row = blobs.width() > layout.right_column().width() / 2.0;
    assert!(fills_row, "{blobs:?} leaves most of {:?} empty", layout.right_column());
    assert_eq!(read(&empty_records).len(), 5, "an empty meter drew a fill");
}

/// Each meter's line is the gutter's twenty characters, its label from the first and its value flush right at the
/// last, a value that is no figure in stone; a meter with a limit writes the bytes it holds alone, as its bar shows
/// the share, even at the widest bytes against the widest limit; and the process's label says `heap` where a browser
/// reports the script heap. So a value column that wanders from line to line, a line that runs past the band's padding
/// at a long value, or a heap shown as the program's resident memory, is caught.
#[test]
fn the_meters_lines_fill_the_gutter_and_a_limit_is_left_to_the_bar() {
    let widest = Held { bytes: 1023 << 20, most: 1023 << 30 };
    let memory = read_with(ProcessMemory::Unknown, widest, Held { bytes: 0, most: 0 });
    let heap = read_with(ProcessMemory::Heap { bytes: 84 << 20 }, widest, widest);

    let line = |memory: &EngineMemory, meter: Meter| -> String {
        memory.row_runs(meter, 2048).iter().map(|run| run.text.as_str()).collect()
    };

    for meter in Meter::ALL {
        for written in [line(&memory, meter), line(&heap, meter)] {
            assert_eq!(written.chars().count(), 20, "{written:?} is not the gutter's width");
        }
    }
    assert_eq!(line(&memory, Meter::Process), "process      unknown");
    assert_eq!(memory.row_runs(Meter::Process, 0)[1].colour, STONE.opaque());
    assert_eq!(line(&memory, Meter::Blobs), "blobs     1023.0 MiB");
    assert_eq!(line(&memory, Meter::Backlog), "backlog         none");
    assert_eq!(line(&memory, Meter::Guests), "guests     no guests");
    assert_eq!(line(&memory, Meter::Gpu), "gpu          2.0 KiB");
    assert_eq!(line(&heap, Meter::Process), "heap        84.0 MiB");
}

/// A meter's history carries a reading into a column no reading fell in, so at a width whose columns are a little
/// longer than the figures' reads are apart, a steady reading that skips a column draws a steady line with no hole in
/// it; and a pause in the readings longer than a second, as while the overlay is hidden, is left a gap. So a one-column
/// hole in a steady line, or a stale reading drawn across a pause, is caught.
#[test]
fn a_column_no_reading_fell_in_carries_the_reading_before_it() {
    let layout = Layout::of(Surface { width: 1920, height: 1080, scale: 1.0 });
    let pitch = layout.column_pitch();
    let area = Rect2 { min: Vec2::new(1000.0, 100.0), max: Vec2::new(60.0_f32.mul_add(pitch, 1000.0), 140.0) };
    let mut history = super::history::History::default();
    // Seconds 0 to 9 read, 3 skipped, and none from 10 to 19; the graph's spans are a second each.
    for second in (0..10).filter(|second| *second != 3) {
        history.push(second * SECOND + SECOND / 2, 1024);
    }
    history.push(20 * SECOND + SECOND / 2, 1024);
    let mut records = Vec::new();

    history.graph(&mut records, layout, area, |_| Slide { newest: 20, through: 0.5, span_nanos: SECOND });

    let lefts: Vec<f32> = read(&records)[1..].iter().map(|record| record.area[0]).collect();
    let at = |span: u64| {
        // Spans of a minute's columns, which an f32 holds exactly.
        #[allow(clippy::cast_precision_loss)]
        let back = (20 - span) as f32;
        (back + 0.5).mul_add(-pitch, area.max.x)
    };
    let drawn = |span: u64| lefts.iter().any(|left| (left - at(span)).abs() < 0.01);
    assert!(drawn(3), "the skipped column at 3 is a hole in the steady line {lefts:?}");
    let carried_through_the_pause = (12..20).any(drawn);
    assert!(!carried_through_the_pause, "the reading was carried across the pause: {lefts:?}");
    assert!(drawn(20) && drawn(9) && drawn(0), "{lefts:?} lost a column that was read");
}

/// A meter's history is its trough of faint stone the graph's size, and over it a stone bar a column, each the most of the readings in its span, against the most any column
/// shown came to, from the graph's foot, the newest column at its right edge; and the GPU's is every kind of its memory
/// summed, which its text shows as the engine's figures are read. So a column that averages its readings and hides a
/// spike, a graph scaled against something else than what it shows, a newest column drawn away from the right edge, or
/// a GPU meter of one kind alone, is caught.
#[test]
fn a_meter_s_history_is_the_most_of_each_column_against_the_most_shown() {
    let layout = Layout::of(Surface { width: 1920, height: 1080, scale: 1.0 });
    let pitch = layout.column_pitch();
    let area = Rect2 { min: Vec2::new(1000.0, 100.0), max: Vec2::new(60.0_f32.mul_add(pitch, 1000.0), 140.0) };
    let resident =
        |bytes| read_with(ProcessMemory::Resident { bytes }, Held { bytes: 0, most: 0 }, Held { bytes: 0, most: 0 });
    let mut histories = MeterHistories::default();
    for (elapsed_nanos, bytes) in [(SECOND / 10, 100), (SECOND / 2, 300), (SECOND * 6 / 5, 150), (SECOND * 13 / 10, 50)]
    {
        histories.framed(&stats(elapsed_nanos));
        histories.read(elapsed_nanos, &resident(bytes));
    }
    let mut records = Vec::new();

    assert_eq!(columns_across(layout, area.width(), MOST_METER_COLUMNS), 60);
    histories.process.graph(&mut records, layout, area, |_| Slide { newest: 1, through: 0.5, span_nanos: SECOND });

    let read_back = read(&records);
    let (trough, drawn) = (&read_back[0], &read_back[1..]);
    assert_eq!((trough.area, trough.colour), (boxed(area), STONE.at(0.08)));
    let heights: Vec<f32> = drawn.iter().map(|record| record.area[3]).collect();
    let expected = [area.height() / 2.0, area.height()];
    let near = heights.len() == expected.len() && heights.iter().zip(expected).all(|(a, b)| (a - b).abs() < 0.01);
    assert!(near, "{heights:?} are not {expected:?}");
    assert!(drawn.iter().all(|record| record.colour == STONE.at(0.6)));
    let at_foot = drawn.iter().all(|record| (record.area[1] + record.area[3] - area.max.y).abs() < 0.01);
    assert!(at_foot, "{drawn:?} are off the graph's foot");
    assert!((drawn[0].area[0] - (area.max.x - pitch / 2.0)).abs() < 0.01, "{:?} is not at the right edge", drawn[0]);
    assert_eq!(histories.gpu_shown(), 15);
}

/// A meter's history keeps its readings, not only its columns, so a graph cut into another count as the window is
/// resized is binned again from the same minute: at 1080, then at 720, then at 1080 again, the meters draw the same
/// records at 1080 both times, and at 720 still draw their minute, each graph's tallest bar its full height. So a
/// history that loses its minute at a resize, or one that keeps the columns of the old count, is caught.
#[test]
fn a_meter_s_history_survives_its_graph_being_cut_again() {
    let (wide, narrow) = (
        Layout::of(Surface { width: 1920, height: 1080, scale: 1.0 }),
        Layout::of(Surface { width: 1280, height: 720, scale: 1.0 }),
    );
    let mut histories = MeterHistories::default();
    let readings = (1_u64..=200).map(|step| (step * SECOND / 4, step % 37 * 1024 * 1024));
    for (elapsed_nanos, bytes) in readings {
        let memory =
            read_with(ProcessMemory::Resident { bytes }, Held { bytes: 0, most: 0 }, Held { bytes: 0, most: 0 });
        histories.read(elapsed_nanos, &memory);
    }
    let memory = EngineMemory::Unread;
    let now = 50 * SECOND;
    let draw = |layout: Layout, histories: &mut MeterHistories| {
        let mut records = Vec::new();
        rows(&mut records, layout, &memory, histories, slide_at(now));
        read(&records)
    };

    let first = draw(wide, &mut histories);
    let resized = draw(narrow, &mut histories);
    let again = draw(wide, &mut histories);

    assert!(!first.is_empty());
    assert_eq!(first, again, "the history drew otherwise after a resize and back");
    let graph = narrow.meter_graph(Meter::Process.index());
    let bar_in_graph = |record: &&Record| {
        let bar = record.colour == STONE.at(0.6);
        let within = record.area[1] >= graph.min.y && record.area[1] + record.area[3] <= graph.max.y + 0.01;
        bar && within
    };
    let tallest = resized.iter().filter(bar_in_graph).map(|record| record.area[3]).fold(0.0_f32, f32::max);
    assert!((tallest - graph.height()).abs() < 0.01, "the tallest bar at 720 is {tallest}, not {graph:?}'s height");
}
