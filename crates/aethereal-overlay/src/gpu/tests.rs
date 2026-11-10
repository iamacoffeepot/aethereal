use super::{GpuFigures, PassTimes, graph};
use crate::chart::tests::read;
use crate::colour::{BONE, MAUVE, MUTED_VIOLET, SLATE_BLUE, Swatch};
use crate::frames::Slide;
use crate::layout::Layout;
use aethereal_render::stats::FrameBudget;
use aethereal_window::Surface;

/// A millisecond, in nanoseconds.
const MILLI: u64 = 1_000_000;

/// A budget of 16.7 milliseconds, held to: the graph's top is twice it.
const LIMIT: FrameBudget = FrameBudget::Limit { nanos: 16_700_000 };

/// The renderer's main pass and the overlay's, as the renderer labels them.
const MAIN: &str = "aether.render.main";
const OVERLAY: &str = "aether.render.overlay";

/// A column's stack is its passes in the order their labels were first heard, whatever order a frame ran them in,
/// each at its median over the column's frames, from the graph's foot on a linear scale to twice the budget, in its
/// pass's colour, with the budget's dashes half way up; a stack over the top is cut there. So a stack in a frame's own
/// order, a pass at its mean, which one spike drags up, a log scale, or a stack spilling into the rows beneath, is
/// caught.
#[test]
fn a_column_stacks_each_pass_s_median_in_the_order_first_heard() {
    let layout = Layout::of(Surface { width: 1920, height: 1080, scale: 1.0 });
    let area = layout.gpu_graph();
    let mut times = PassTimes::default();
    times.take(100 * MILLI, [(MAIN, 4 * MILLI), (OVERLAY, MILLI)]);
    times.take(200 * MILLI, [(OVERLAY, MILLI), (MAIN, 4 * MILLI)]);
    times.take(300 * MILLI, [(MAIN, 40 * MILLI), (OVERLAY, MILLI)]);
    times.take(1_100 * MILLI, [(MAIN, 100 * MILLI)]);
    let slide = Slide { newest: 1, through: 0.5, span_nanos: 1_000 * MILLI };
    let mut bytes = Vec::new();

    graph(&mut bytes, layout, slide, LIMIT, times.columns(60));

    let drawn = read(&bytes);
    let tall = |nanos: u64| {
        // Milliseconds of a test, which an f32 holds exactly enough.
        #[allow(clippy::cast_precision_loss)]
        let share = nanos as f32 / (2.0 * 16_700_000.0);
        share * area.height()
    };
    let near = |a: f32, b: f32| (a - b).abs() < 0.01;
    let [spike, main, overlay, dash] = [&drawn[0], &drawn[1], &drawn[2], &drawn[3]];
    assert_eq!((spike.colour, spike.area[1], spike.area[3]), (SLATE_BLUE.opaque(), area.min.y, area.height()));
    assert_eq!(main.colour, SLATE_BLUE.opaque());
    assert!(near(main.area[1] + main.area[3], area.max.y) && near(main.area[3], tall(4 * MILLI)), "{main:?}");
    assert_eq!(overlay.colour, MUTED_VIOLET.opaque());
    assert!(near(overlay.area[1] + overlay.area[3], main.area[1]), "{overlay:?} does not stand on {main:?}");
    assert!(near(overlay.area[3], tall(MILLI)), "{overlay:?} is not a millisecond tall");
    assert_eq!(dash.colour, BONE.at(0.7));
    assert!((dash.area[1] - (area.max.y - area.height() / 2.0)).abs() <= 1.0, "{dash:?} is not half way up");
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
