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

/// The console's scroll bar lies inside its panel, right of the scrollback's text by a gap, and as tall as the rows in view, at
/// 1080 and on a screen of twice the pixels, so a bar drawn over the text, outside the panel, or past the prompt is
/// caught.
#[test]
fn the_scroll_bar_lies_in_the_panels_padding_beside_the_rows() {
    for scale in [1.0, 2.0] {
        let layout = Layout::of(Surface { width: 1920, height: 1080, scale });

        let (bar, inside, console) = (layout.scrollbar(), layout.console_inside(), layout.console());
        let rows = u16::try_from(layout.scrollback_rows()).map_or(f32::MAX, f32::from);

        // A gap of a few logical pixels at least, so the text does not run up against the bar.
        let clear_of_text = bar.min.x - inside.max.x >= layout.pixels(8.0);
        let in_panel = bar.max.x <= console.max.x;
        assert!(clear_of_text && in_panel, "{bar:?} is not between {inside:?} and the right edge of {console:?}");
        assert!(bar.width() >= 1.0, "{bar:?} is too thin to see");
        let tall = layout.line_pixels_tall() * rows;
        assert_eq!(bar.min.y, layout.scrollback_at().y);
        assert!((bar.height() - tall).abs() < 0.01, "{bar:?} is not {rows} rows tall");
        assert!(bar.max.y <= layout.prompt_at().y, "{bar:?} runs past the prompt");
    }
}

/// The band, the scrollback's strip, and the prompt's strip tile the overlay from the window's top to the console's
/// foot, each the window's width, none overlapping the next and none left empty, at 1080 and on a screen of twice the
/// pixels, so a strip drawn twice over at the panels' opacity, or a seam of the program showing through, is caught.
#[test]
fn the_strips_tile_the_panel() {
    for scale in [1.0, 2.0] {
        let layout = Layout::of(Surface { width: 1920, height: 1080, scale });

        let strips = [layout.band(), layout.console_body(), layout.prompt_strip()];

        assert_eq!(strips[0].min.y, 0.0);
        assert_eq!(strips[2].max.y, layout.console().max.y);
        for pair in strips.windows(2) {
            assert_eq!(pair[0].max.y, pair[1].min.y, "{:?} and {:?} do not meet", pair[0], pair[1]);
        }
        for strip in strips {
            let across = strip.min.x == 0.0 && strip.max.x == 1920.0;
            assert!(across && strip.height() > 0.0, "{strip:?} does not cross the window");
        }
    }
}

/// The prompt's line sits inside its strip, and the scrollback's rows end above it; each divider is a line along the
/// top of the strip it heads; and what is running ends at the right of the console's text on the prompt's line, so a
/// prompt drawn over the scrollback's tint, a divider off its seam, or a running line past the scroll bar is caught.
#[test]
fn the_prompt_and_its_divider_sit_in_their_strip() {
    for scale in [1.0, 2.0] {
        let layout = Layout::of(Surface { width: 1920, height: 1080, scale });

        let (strip, body, prompt) = (layout.prompt_strip(), layout.console_body(), layout.prompt_at());
        let rows = u16::try_from(layout.scrollback_rows()).map_or(f32::MAX, f32::from);
        let running = layout.running_at(10);

        let line_foot = prompt.y + layout.line_pixels_tall();
        let in_strip = prompt.y >= strip.min.y && line_foot <= strip.max.y;
        assert!(in_strip, "the prompt at {prompt:?} is not inside {strip:?}");
        let rows_foot = layout.line_pixels_tall().mul_add(rows, layout.scrollback_at().y);
        assert!(rows_foot <= strip.min.y, "the rows end at {rows_foot}, past {strip:?}");
        for (divider, heads) in [(layout.band_divider(), body), (layout.prompt_divider(), strip)] {
            let on_seam = divider.min == heads.min && divider.max.x == heads.max.x;
            assert!(on_seam && divider.height() == layout.line_pixels(1.0), "{divider:?} does not head {heads:?}");
        }
        let ends = layout.advance().mul_add(10.0, running.x);
        assert_eq!(running.y, prompt.y);
        assert!((ends - layout.console_inside().max.x).abs() < 0.01, "what is running ends at {ends}");
    }
}
