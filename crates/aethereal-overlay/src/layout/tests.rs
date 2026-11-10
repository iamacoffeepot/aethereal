use super::{LINE_EMS, Layout};
use aethereal_window::Surface;

/// Down the band's left column, at 1080 and at an ultrawide 1373, the frame chart, its figures, the GPU's graph, its
/// figures, and the rows stand in that order without overlapping, inside the band, the GPU's graph two thirds of the
/// frame chart's height; the graph and the rows span the frame chart's pixels across, so their columns stand under
/// its; and the console's panel starts where the band ends. So a section drawn over another, one that spills out of
/// the band, or a graph whose columns do not line up with the frame chart's, is caught.
#[test]
fn the_band_stacks_its_sections_on_one_axis_and_the_console_sits_beneath_it() {
    for (width, height) in [(1920, 1080), (3440, 1373)] {
        let layout = Layout::of(Surface { width, height, scale: 1.0 });

        let (band, console, chart) = (layout.band(), layout.console(), layout.chart());
        let (gpu, rows) = (layout.gpu_graph(), layout.rows());
        let line = layout.text_pixels() * LINE_EMS;

        assert_eq!(console.min.y, band.max.y);
        let (figures, gpu_figures) = (layout.figures_at().y, layout.gpu_figures_at().y);
        // Each edge down the column, top to bottom: each pair a section's foot and the next one's top.
        let edges = [
            (band.min.y, chart.min.y),
            (chart.max.y, figures),
            (figures + line, gpu.min.y),
            (gpu.max.y, gpu_figures),
            (gpu_figures + line, rows.min.y),
            (rows.max.y, band.max.y),
        ];
        let in_order = edges.iter().all(|(above, below)| above < below);
        assert!(in_order, "the sections of {band:?} overlap or spill out of it");
        let across = |area: aethereal_math::Rect2| area.min.x == chart.min.x && area.max.x == chart.max.x;
        assert!(across(gpu) && across(rows), "{gpu:?} or {rows:?} is not across {chart:?}");
        assert!((gpu.height() - (chart.height() * 2.0 / 3.0).round()).abs() < 0.01, "{gpu:?} is not 2/3 of {chart:?}");
    }
}

/// Each row's bars stand inside its line of the rows, the first's at the top of the gutter's text and each line of
/// that text beside its row, the GPU graph's top is written in the same gutter level with the graph's top, and the
/// gutter's text ends inside the band's left column, so a row's label written beside the wrong row, a graph's top
/// written away from its graph, or a gutter running into the right column, is caught.
#[test]
fn the_gutter_s_lines_stand_beside_their_rows() {
    for scale in [1.0, 2.0] {
        let layout = Layout::of(Surface { width: 1920, height: 1080, scale });

        let (rows, at, tall) = (layout.rows(), layout.lines_at(), layout.line_pixels_tall());

        assert_eq!(at.y, rows.min.y);
        assert_eq!(layout.gpu_top_at(), aethereal_math::Vec2::new(at.x, layout.gpu_graph().min.y));
        for index in 0..4_u8 {
            let row = layout.row(index);
            let line_top = f32::from(index).mul_add(tall, at.y);
            let beside = row.min.y >= line_top && row.max.y <= line_top + tall && row.height() >= 1.0;
            assert!(beside, "row {index} at {row:?} is not beside its line at {line_top}");
        }
        // A gutter of some tens of characters, which an f32 holds exactly.
        #[allow(clippy::cast_precision_loss)]
        let gutter_end = (super::GUTTER_CHARACTERS as f32).mul_add(layout.advance(), at.x);
        assert!(at.x > rows.max.x && gutter_end <= 1920.0 * 0.6, "the gutter from {at:?} is out of the left column");
    }
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
