use super::{LINE_EMS, Layout};
use aethereal_window::Surface;

/// At 1080 the band is a tenth of the window, the console's panel starts where the band ends, the figures' line sits at
/// the band's foot, and the chart fills most of the band's left column above it, so a panel that overlaps another, a
/// chart that spills out of the band, or a chart squeezed into a strip of the band is caught.
#[test]
fn the_band_holds_the_chart_and_the_console_sits_beneath_it() {
    let layout = Layout::of(Surface { width: 1920, height: 1080, scale: 1.0 });

    let (band, console, chart) = (layout.band(), layout.console(), layout.chart());
    let figures = layout.figures_at();

    assert_eq!((band.height(), console.min.y, console.height()), (108.0, 108.0, 360.0));
    let inside = band.min.y < chart.min.y && chart.max.x < 1920.0 * 0.6 && chart.max.y < figures.y;
    assert!(inside, "{chart:?} is not inside the band's left column above {figures:?}");
    let line_foot = layout.text_pixels().mul_add(LINE_EMS, figures.y);
    let at_foot = line_foot < band.max.y && band.max.y - line_foot <= 12.0;
    assert!(at_foot, "{figures:?} does not sit at the foot of {band:?}");
    assert!(chart.height() > band.height() / 2.0, "{chart:?} takes too little of {band:?}");
}
