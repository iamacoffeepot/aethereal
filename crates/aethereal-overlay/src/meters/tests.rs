use super::adapter::written;
use super::bars::fill_swatch;
use super::figures::{LINE_CHARACTERS, cut_back, cut_front};
use super::{Backlog, EngineMemory, Held, MemoryFigures, Meter, ProcessMemory, bars};
use crate::chart::tests::read;
use crate::colour::{BRICK, LICHEN, RUST, STONE};
use crate::layout::Layout;
use aethereal_render::stats::GpuDescribed;
use aethereal_window::Surface;

/// The engine's memory with the blob store holding `blobs` and a backlog of `backlog` held by `actor`.
fn read_with(blobs: Held, backlog: Held, actor: &str) -> EngineMemory {
    EngineMemory::Read(MemoryFigures {
        process: ProcessMemory::Unknown,
        blobs,
        backlog: Backlog::Held { unsent: backlog, actor: String::from(actor) },
        guest_bytes: 0,
    })
}

/// A fill is lichen under 75% of its most, rust from 75%, and brick from 90%, so a threshold set a step off the
/// ADR's table, or a percentage worked out in the wrong order, is caught.
#[test]
fn a_fill_turns_rust_at_three_quarters_and_brick_at_nine_tenths() {
    let at = |bytes| fill_swatch(Held { bytes, most: 100 });

    assert_eq!([at(74), at(75), at(89), at(90), at(250)], [LICHEN, RUST, RUST, BRICK, BRICK]);
}

/// Each meter with a limit draws its trough in faint stone and its fill over it from the trough's left, as wide a share
/// as the bytes are of their most and never past the trough; a meter with no most draws its trough alone. So a fill
/// that runs out of its trough when an actor holds more than its most, one scaled against the wrong figure, or a
/// fill drawn for a backlog that is none, is caught.
#[test]
fn a_bar_s_fill_is_its_share_of_the_trough_and_stays_inside_it() {
    let layout = Layout::of(Surface { width: 1920, height: 1080, scale: 1.0 });
    let memory = read_with(Held { bytes: 25, most: 100 }, Held { bytes: 300, most: 100 }, "aether.tcp");
    let empty = read_with(Held { bytes: 0, most: 100 }, Held { bytes: 0, most: 0 }, "aether.tcp");
    let (mut records, mut empty_records) = (Vec::new(), Vec::new());

    bars(&mut records, layout, &memory);
    bars(&mut empty_records, layout, &empty);

    let drawn = read(&records);
    let (blobs, backlog) = (layout.meter_bar(Meter::Blobs.index()), layout.meter_bar(Meter::Backlog.index()));
    assert_eq!(drawn.len(), 4);
    assert_eq!(drawn[0].area, [blobs.min.x, blobs.min.y, blobs.width(), blobs.height()]);
    assert_eq!(drawn[0].colour, STONE.at(0.2));
    assert!((drawn[1].area[2] - blobs.width() / 4.0).abs() < 0.01, "{:?} is not a quarter of {blobs:?}", drawn[1]);
    assert_eq!(drawn[1].colour, LICHEN.opaque());
    assert_eq!(drawn[3].area, [backlog.min.x, backlog.min.y, backlog.width(), backlog.height()]);
    assert_eq!(drawn[3].colour, BRICK.opaque());
    assert_eq!(read(&empty_records).len(), 2, "an empty meter drew a fill");
}

/// Each meter's line is its label, then its value flush right at the same column, a value that is no figure in stone;
/// the backlog's actor follows it, cut at its front to the room left, and is left out where the column has too little;
/// so a value column that wanders from line to line, an actor's path running past the right column, or one cut so
/// short it names nothing, is caught.
#[test]
fn the_meters_values_line_up_and_the_backlog_s_actor_is_cut_to_the_column() {
    let actor = "aether.tcp/connection:1234567890";
    let memory = read_with(Held { bytes: 1024, most: 8 * 1024 * 1024 }, Held { bytes: 0, most: 1024 }, actor);

    let wide = memory.runs(2048, LINE_CHARACTERS + 2 + 12);
    let narrow = memory.runs(2048, LINE_CHARACTERS + 2 + 4);

    let text: String = wide.iter().map(|run| run.text.as_str()).collect();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 5);
    assert!(lines.iter().all(|line| line.chars().count() >= LINE_CHARACTERS));
    assert_eq!(lines[0].trim_end(), format!("process{:>w$}", "unknown", w = LINE_CHARACTERS - 7));
    assert!(lines[1].ends_with("1.0 KiB / 8.0 MiB"), "{}", lines[1]);
    assert!(lines[2].ends_with("0 B / 1.0 KiB  …:1234567890"), "{}", lines[2]);
    assert_eq!(lines[2].chars().count(), LINE_CHARACTERS + 2 + 12);
    assert!(lines[3].ends_with("no guests") && lines[4].ends_with("2.0 KiB"), "{lines:?}");
    let narrow_text: String = narrow.iter().map(|run| run.text.as_str()).collect();
    assert!(!narrow_text.contains("aether") && !narrow_text.contains('…'), "{narrow_text}");
}

/// The adapter's line parts its name, backend, and driver by dots, leaving out a part the platform leaves empty, and
/// is cut at its end to the column, so a line that runs past the right column or prints a stray dot for an empty
/// driver is caught.
#[test]
fn the_adapter_s_line_leaves_out_empty_parts_and_is_cut_to_the_column() {
    let described = GpuDescribed {
        name: String::from("AMD Radeon RX 6900 XT (RADV NAVI21)"),
        device_type: String::from("DiscreteGpu"),
        backend: String::from("vulkan"),
        driver: String::from("radv"),
        driver_info: String::from("Mesa 25.2.8"),
    };
    let bare = GpuDescribed { driver: String::new(), driver_info: String::new(), ..described.clone() };

    assert_eq!(written(&described), "AMD Radeon RX 6900 XT (RADV NAVI21) · vulkan · radv Mesa 25.2.8");
    assert_eq!(written(&bare), "AMD Radeon RX 6900 XT (RADV NAVI21) · vulkan");
    assert_eq!([cut_back("abcdef", 4), cut_front("abcdef", 4), cut_back("abc", 4)], ["abc…", "…def", "abc"]);
}
