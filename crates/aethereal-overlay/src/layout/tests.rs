use super::Layout;
use aethereal_window::Surface;

/// At 1080 the band is a tenth of the window, the console's panel starts where the band ends, and the chart and its
/// figures sit inside the band's left column, so a panel that overlaps another or a chart that spills out of the band
/// is caught.
#[test]
fn the_band_holds_the_chart_and_the_console_sits_beneath_it() {
    let layout = Layout::of(Surface { width: 1920, height: 1080, scale: 1.0 });

    let (band, console, chart) = (layout.band(), layout.console(), layout.chart());
    let figures = layout.figures_at();

    assert_eq!((band.height(), console.min.y, console.height()), (108.0, 108.0, 360.0));
    let inside = band.min.y < chart.min.y && chart.max.x < 1920.0 * 0.6 && chart.max.y < figures.y;
    assert!(inside, "{chart:?} is not inside the band's left column above {figures:?}");
    assert!(figures.y + layout.text_pixels() < band.max.y, "{figures:?} runs past the band");
}
