use super::{GpuFigures, GraphTop, PassTimes, graph};
use crate::chart::tests::{Record, read};
use crate::colour::{BONE, MAUVE, MUTED_VIOLET, SLATE_BLUE, STONE, Swatch};
use crate::frames::Slide;
use crate::layout::{GUTTER_CHARACTERS, Scale};
use aethereal_math::{Rect2, Vec2};
use aethereal_render::stats::FrameBudget;

/// A millisecond, in nanoseconds.
const MILLI: u64 = 1_000_000;

/// A budget of 16.7 milliseconds, held to.
const LIMIT: FrameBudget = FrameBudget::Limit { nanos: 16_700_000 };

/// A budget of 4.17 milliseconds, as a 240 Hz display holds to.
const FAST: FrameBudget = FrameBudget::Limit { nanos: 4_170_000 };

/// The renderer's main pass and the overlay's, as the renderer labels them.
const MAIN: &str = "aether.render.main";
const OVERLAY: &str = "aether.render.overlay";

/// The graph's box on a window 1080 pixels tall at a scale of one: as wide as the frame chart above it, and two thirds
/// as tall.
const AREA: Rect2 = Rect2 { min: Vec2::new(10.0, 110.0), max: Vec2::new(966.0, 153.0) };

/// The columns of `times` cut for a chart of `count`.
fn cut(times: &mut PassTimes, count: u32) -> Vec<super::Spanned<super::Stack>> {
    times.cut(count);

    times.columns().collect()
}

/// How tall `nanos` stands in `area` against a top of `top_nanos`.
fn tall(area: Rect2, nanos: u64, top_nanos: u64) -> f32 {
    // Milliseconds of a test, which an f32 holds exactly enough.
    #[allow(clippy::cast_precision_loss)]
    let share = nanos as f32 / top_nanos as f32;

    share * area.height()
}

/// Two columns a second apart: the first of three frames, `main` and `overlay` in each, the second of one frame of
/// `spike` in the main pass alone; drawn against `budget`, its boxes read back, and the top it was drawn to.
fn drawn(main: u64, overlay: u64, spike: u64, budget: FrameBudget) -> (Vec<Record>, GraphTop) {
    let mut times = PassTimes::default();
    times.take(100 * MILLI, [(MAIN, main), (OVERLAY, overlay)]);
    times.take(200 * MILLI, [(OVERLAY, overlay), (MAIN, main)]);
    times.take(300 * MILLI, [(MAIN, 10 * main), (OVERLAY, overlay)]);
    times.take(1_100 * MILLI, [(MAIN, spike)]);
    let slide = Slide { newest: 1, through: 0.5, span_nanos: 1_000 * MILLI };
    let mut fills = Vec::new();

    let top = graph(&mut fills, AREA, Scale(1.0), slide, budget, cut(&mut times, 60));

    (read(&fills), top)
}

/// A column's stack is its passes in the order their labels were first heard, whatever order a frame ran them in,
/// each at its median over the column's frames, from the graph's foot on a linear scale to a top at the tallest stack,
/// in its pass's colour; the tallest reaches the top and is not cut. So a stack in a frame's own order, a pass at its
/// mean, which one spike drags up, a log scale, or a top under the tallest stack, which clips it, is caught.
#[test]
fn a_column_stacks_each_pass_s_median_in_the_order_first_heard() {
    let area = AREA;

    let (drawn, top) = drawn(4 * MILLI, MILLI, 10 * MILLI, LIMIT);

    let near = |a: f32, b: f32| (a - b).abs() < 0.01;
    assert_eq!(top.nanos(), 10 * MILLI);
    let [spike, main, overlay] = [&drawn[0], &drawn[1], &drawn[2]];
    assert_eq!((spike.colour, spike.area[1], spike.area[3]), (SLATE_BLUE.opaque(), area.min.y, area.height()));
    assert_eq!(main.colour, SLATE_BLUE.opaque());
    let main_tall = tall(area, 4 * MILLI, top.nanos());
    assert!(near(main.area[1] + main.area[3], area.max.y) && near(main.area[3], main_tall), "{main:?}");
    assert_eq!(overlay.colour, MUTED_VIOLET.opaque());
    assert!(near(overlay.area[1] + overlay.area[3], main.area[1]), "{overlay:?} does not stand on {main:?}");
    assert!(near(overlay.area[3], tall(area, MILLI, top.nanos())), "{overlay:?} is not a millisecond tall");
}

/// The top is the tallest stack rounded up to one, two, or five of a power of ten nanoseconds, and no less than ten
/// microseconds. So a top under the tallest stack, which clips it; a top far over it, which leaves the graph flat, as
/// twice a 240 Hz budget left a GPU's 0.08 ms under a pixel; or no floor, which draws an idle GPU's noise full
/// height, is caught.
#[test]
fn the_top_rounds_the_tallest_stack_up_to_a_step() {
    let cases = [
        (0, 10_000),
        (3_000, 10_000),
        (10_000, 10_000),
        (10_001, 20_000),
        (83_000, 100_000),
        (100_000, 100_000),
        (120_000, 200_000),
        (3_100_000, 5_000_000),
        (5_000_001, 10_000_000),
        (40 * MILLI, 50 * MILLI),
    ];
    for (tallest_nanos, top_nanos) in cases {
        let mut times = PassTimes::default();
        times.take(100 * MILLI, [(MAIN, tallest_nanos / 2), (OVERLAY, tallest_nanos - tallest_nanos / 2)]);

        let top = GraphTop::of(cut(&mut times, 60));

        assert_eq!(top.nanos(), top_nanos, "the tallest stack of {tallest_nanos} ns");
    }
}

/// The budget's dashes stand where the budget falls on the graph's scale when it is at or under the top, and are
/// not drawn when it is over the top. So a budget line pinned at the top, which reads as every frame at the
/// budget, or one drawn where it falls off the scale, is caught.
#[test]
fn the_budget_is_dashed_only_under_the_top() {
    let area = AREA;

    let (under, top) = drawn(4 * MILLI, MILLI, 10 * MILLI, FAST);
    let (over, _) = drawn(4 * MILLI, MILLI, 10 * MILLI, LIMIT);

    let dash = &under[3];
    let budget_y = area.max.y - tall(area, 4_170_000, top.nanos());
    assert_eq!(dash.colour, BONE.at(0.7));
    assert!((dash.area[1] - budget_y).abs() <= 1.0, "{dash:?} is not where 4.17 ms falls");
    let dashed = over.iter().any(|record| record.colour == BONE.at(0.7));
    assert!(!dashed, "a budget over the top was dashed");
}

/// A pass that took any time stands at least a pixel tall on the one beneath it, however short against the top, and
/// a pass that took none is not drawn. So a short pass that vanishes from the stack, or a zero-height record, is
/// caught.
#[test]
fn a_pass_too_short_for_a_pixel_still_stands_one() {
    let (drawn, _) = drawn(6 * MILLI, 1_000, 6 * MILLI, LIMIT);

    let [main, overlay] = [&drawn[1], &drawn[2]];
    assert_eq!(overlay.colour, MUTED_VIOLET.opaque());
    assert!((overlay.area[3] - 1.0).abs() < 0.01, "{overlay:?} is not a pixel tall");
    assert!((overlay.area[1] + overlay.area[3] - main.area[1]).abs() < 0.01, "{overlay:?} does not stand on {main:?}");
    assert_eq!(drawn.len(), 3, "the spike's column drew a pass it did not run");
}

/// The gutter's line beside the graph is `gpu` in stone and its top in milliseconds in bone, flush right in the
/// gutter as the rows' values are, with as many places as the top needs. So a top written in the wrong unit, with
/// trailing zeros, or out of line with the rows' values, is caught.
#[test]
fn the_gutter_says_the_graph_s_top() {
    for (top_nanos, line) in [
        (100_000, "gpu       top 0.1 ms"),
        (20_000, "gpu      top 0.02 ms"),
        (5 * MILLI, "gpu         top 5 ms"),
        (50 * MILLI, "gpu        top 50 ms"),
    ] {
        let mut times = PassTimes::default();
        times.take(100 * MILLI, [(MAIN, top_nanos)]);

        let runs = GraphTop::of(cut(&mut times, 60)).runs();

        let written: String = runs.iter().map(|run| run.text.as_str()).collect();
        assert_eq!(written, line);
        assert_eq!(written.len(), GUTTER_CHARACTERS);
        assert_eq!([runs[0].colour, runs[1].colour], [STONE.opaque(), BONE.opaque()]);
    }
}

/// The figures are each pass's median, its label without the renderer's prefix, in its pass's colour, so the line is
/// the graph's legend; two passes of one label in a frame, as two windows run, count together; and a pass past the
/// stack's room is left out of the figures but not of the GPU's busy share. So a legend whose colours do not match
/// the stack, a second window's pass dropped, or a busy share that forgets the passes the graph cannot stack, is
/// caught.
#[test]
fn the_figures_are_the_legend_and_the_busy_share_counts_every_pass() {
    let mut times = PassTimes::default();
    let passes = [
        (MAIN, 2 * MILLI),
        (MAIN, 2 * MILLI),
        (OVERLAY, MILLI),
        ("shadow", MILLI),
        ("bloom", MILLI),
        ("tonemap", MILLI),
        ("compute", MILLI),
        ("past", MILLI),
    ];
    for frame in 1..=3 {
        times.take(frame * 100 * MILLI, passes);
    }

    let figures: GpuFigures = times.figures(20 * MILLI);

    let runs = figures.runs();
    let texts: Vec<&str> = runs.iter().map(|run| run.text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "main 4.00",
            "  overlay 1.00",
            "  shadow 1.00",
            "  bloom 1.00",
            "  tonemap 1.00",
            "  compute 1.00",
            " ms  gpu busy ",
            "50%"
        ]
    );
    let legend: Vec<_> = runs[..6].iter().map(|run| run.colour).collect();
    let swatches = [SLATE_BLUE, MUTED_VIOLET, MAUVE].map(Swatch::opaque);
    assert_eq!(legend, [swatches, swatches].concat());
}
