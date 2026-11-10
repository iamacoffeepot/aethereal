use super::{GUTTER_CHARACTERS, LINE_EMS, Layout, METER_LABEL_CHARACTERS};
use crate::meters::{Backlog, EngineMemory, Held, MemoryFigures, Meter, ProcessMemory, ROW_CHARACTERS};
use aethereal_math::{Rect2, Vec2};
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
        let across = |area: Rect2| area.min.x == chart.min.x && area.max.x == chart.max.x;
        assert!(across(gpu) && across(rows), "{gpu:?} or {rows:?} is not across {chart:?}");
        assert!((gpu.height() - (chart.height() * 2.0 / 3.0).round()).abs() < 0.01, "{gpu:?} is not 2/3 of {chart:?}");
    }
}

/// Each row's bars stand inside its line of the rows, the first's at the top of the gutter's text and each line of
/// that text beside its row; the frame chart's median is written in the same gutter level with the chart's top, and
/// the count of warnings and errors the line beneath it, both beside the chart; the GPU graph's top is written level
/// with the graph's top; and the gutter's text ends inside the band's left column. So a row's label written beside
/// the wrong row, a graph's line written away from its graph, a count of warnings that runs down over the frame
/// figures' line, or a gutter running into the right column, is caught.
#[test]
fn the_gutter_s_lines_stand_beside_their_rows() {
    for scale in [1.0, 2.0] {
        let layout = Layout::of(Surface { width: 1920, height: 1080, scale });

        let (rows, at, tall) = (layout.rows(), layout.lines_at(), layout.line_pixels_tall());

        assert_eq!(at.y, rows.min.y);
        let chart = layout.chart();
        assert_eq!(layout.frame_median_at(), Vec2::new(at.x, chart.min.y));
        assert_eq!(layout.alerts_at(), Vec2::new(at.x, chart.min.y + tall));
        let alerts_foot = layout.alerts_at().y + tall;
        assert!(alerts_foot <= chart.max.y, "the count of warnings ending at {alerts_foot} is not beside {chart:?}");
        assert_eq!(layout.gpu_top_at(), Vec2::new(at.x, layout.gpu_graph().min.y));
        for index in 0..4_u8 {
            let row = layout.row(index);
            let line_top = f32::from(index).mul_add(tall, at.y);
            let beside = row.min.y >= line_top && row.max.y <= line_top + tall && row.height() >= 1.0;
            assert!(beside, "row {index} at {row:?} is not beside its line at {line_top}");
        }
        // A gutter of some tens of characters, which an f32 holds exactly.
        #[allow(clippy::cast_precision_loss)]
        let gutter_end = (GUTTER_CHARACTERS as f32).mul_add(layout.advance(), at.x);
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

/// At 1080, an ultrawide 1373, a narrow 720, and 1080 on a screen of twice the pixels, the band's right column stands
/// right of the left column's gutter and inside the band, from the frame chart's top to the left column's foot, ending
/// at the band's right padding; its five meters' rows share it at one pitch, the first's top its top and the last's
/// foot its foot; each row's line of text is centred down the row and ends at the band's right padding; and each
/// meter's graph stands inside its row, from the end of its label's eight characters, at least a character's space
/// after its label, to a character's space before what its row writes at its right, so it fills the room between them. So rows that leave an empty block at the column's
/// foot, rows of uneven heights, text that sits at a row's top, a graph that runs under its label or its value, or one
/// that stops short of either and leaves an empty stretch, is caught.
#[test]
fn the_meters_rows_fill_the_right_column_and_their_graphs_fill_between_label_and_value() {
    let memory = memory();
    for (width, height, scale) in SIZES {
        let layout = Layout::of(Surface { width, height, scale });

        let (right, band, tall, advance) =
            (layout.right_column(), layout.band(), layout.line_pixels_tall(), layout.advance());
        let columns = layout.right_columns();
        // A gutter of some tens of characters, which an f32 holds exactly.
        #[allow(clippy::cast_precision_loss)]
        let gutter_end = (GUTTER_CHARACTERS as f32).mul_add(advance, layout.lines_at().x);
        let size = format!("{width}x{height} at {scale}");
        assert!(right.min.x > gutter_end, "{right:?} at {size} is over the gutter ending at {gutter_end}");
        let spans = right.min.y == layout.chart().min.y && right.max.y == layout.rows().max.y;
        assert!(spans, "{right:?} at {size} does not run from the frame chart's top to the rows' foot");
        let in_band = right.max.x <= band.max.x && right.max.y <= band.max.y;
        assert!(in_band, "{right:?} spills out of {band:?}");
        assert_eq!(right.max.x, band.max.x - layout.pixels(10.0), "{right:?} does not end at the band's padding");
        assert!(columns >= ROW_CHARACTERS, "a meter's row does not fit {right:?} at {size}");
        let pitch = right.height() / meters();
        let first = layout.meter_row(0);
        let last = layout.meter_row(Meter::Gpu.index());
        let fills = (first.min.y - right.min.y).abs() < 0.01 && (last.max.y - right.max.y).abs() < 0.01;
        assert!(fills, "the rows run from {first:?} to {last:?}, not over {right:?} at {size}");
        for meter in Meter::ALL {
            let index = meter.index();
            let (row, at) = (layout.meter_row(index), layout.meter_at(index));
            let top = f32::from(index).mul_add(pitch, right.min.y);
            let even = (row.min.y - top).abs() < 0.01 && (row.height() - pitch).abs() < 0.01;
            let across = row.min.x == right.min.x && row.max.x == right.max.x;
            assert!(even && across, "{meter:?}'s row {row:?} at {size} is off its share of {right:?}");
            let centred = ((at.y + tall / 2.0) - (row.min.y + row.height() / 2.0)).abs() <= 0.5;
            assert!(centred && at.x == row.min.x, "{meter:?}'s text at {at:?} is not centred in {row:?}");

            let runs = memory.row_runs(meter, GPU_BYTES, columns);
            let line: String = runs.iter().map(|run| run.text.as_str()).collect();
            // A line of some tens of characters, which an f32 holds exactly.
            #[allow(clippy::cast_precision_loss)]
            let (ends, label) = (
                (line.chars().count() as f32).mul_add(advance, at.x),
                line.split_whitespace().next().map_or(0.0, |label| label.len() as f32),
            );
            assert!((ends - right.max.x).abs() < 0.01, "{line:?} at {size} ends at {ends}, not at {right:?}'s edge");

            let written = memory.tail_characters(meter, GPU_BYTES, columns);
            let graph = layout.meter_graph(index, written);
            // Counts of some tens of characters, which an f32 holds exactly.
            #[allow(clippy::cast_precision_loss)]
            let (tail_starts, slot_ends) = (
                (written as f32).mul_add(-advance, right.max.x),
                (METER_LABEL_CHARACTERS as f32).mul_add(advance, at.x),
            );
            let label_ends = label.mul_add(advance, at.x);
            let after_label = graph.min.x >= label_ends + advance - 0.5 && (graph.min.x - slot_ends).abs() <= 0.5;
            let before_tail = (graph.max.x - (tail_starts - advance)).abs() <= 0.5;
            assert!(
                after_label && before_tail,
                "{meter:?}'s graph {graph:?} at {size} does not fill between its label ending at {label_ends} and its tail at {tail_starts}"
            );
            let inside = graph.min.y > row.min.y && graph.max.y < row.max.y && graph.height() >= row.height() / 2.0;
            assert!(inside, "{meter:?}'s graph {graph:?} at {size} is not the height of its row {row:?}");
        }
    }
}

/// At 1080, an ultrawide 1373, and a narrow 720, and on a screen of twice the pixels, the line between the band's
/// columns runs the band's content from the frame chart's top to the rows' foot, centred between the gutter's text and
/// the meters'; and the two lines between the left column's sections cross it from the frame chart's left to the
/// gutter's end, each in the space between one section's figures and the next section's top, short of the line
/// between the columns. None of them is drawn over a graph or a line of text, and each is a line thick. So a divider
/// drawn through a section, one that crosses into the meters, one off centre in its space, or one that spills out of
/// the band, is caught.
#[test]
fn the_band_s_dividers_part_its_columns_and_sections_clear_of_what_they_part() {
    for (width, height, scale) in SIZES {
        let layout = Layout::of(Surface { width, height, scale });

        let (band, chart, gpu, rows) = (layout.band(), layout.chart(), layout.gpu_graph(), layout.rows());
        let (right, tall, thick) = (layout.right_column(), layout.line_pixels_tall(), layout.line_pixels(1.0));
        let column = layout.column_divider();
        let sections = layout.section_dividers();
        // A gutter of some tens of characters, which an f32 holds exactly.
        #[allow(clippy::cast_precision_loss)]
        let gutter_end = (GUTTER_CHARACTERS as f32).mul_add(layout.advance(), layout.lines_at().x);
        let line_at = |at: Vec2, left: f32, right: f32, lines: f32| Rect2 {
            min: Vec2::new(left, at.y),
            max: Vec2::new(right, lines.mul_add(tall, at.y)),
        };
        let figures_left = chart.min.x;
        let gutter_left = layout.lines_at().x;
        let drawn = [
            chart,
            gpu,
            rows,
            line_at(layout.figures_at(), figures_left, gutter_end, 1.0),
            line_at(layout.gpu_figures_at(), figures_left, gutter_end, 1.0),
            line_at(layout.frame_median_at(), gutter_left, gutter_end, 2.0),
            line_at(layout.gpu_top_at(), gutter_left, gutter_end, 1.0),
            line_at(layout.lines_at(), gutter_left, gutter_end, 3.0),
        ];
        let memory = memory();
        let meters_drawn = Meter::ALL.into_iter().flat_map(|meter| {
            let index = meter.index();
            let written = memory.tail_characters(meter, GPU_BYTES, layout.right_columns());
            [line_at(layout.meter_at(index), right.min.x, right.max.x, 1.0), layout.meter_graph(index, written)]
        });
        let drawn: Vec<Rect2> = drawn.into_iter().chain(meters_drawn).collect();

        let spans_content = column.min.y == chart.min.y && column.max.y == rows.max.y;
        let in_band = column.min.y >= band.min.y && column.max.y <= band.max.y;
        assert!(spans_content && in_band, "{column:?} at {width}x{height} does not run {band:?}'s content");
        let between = column.min.x > gutter_end && column.max.x < right.min.x;
        let off_centre = ((column.min.x - gutter_end) - (right.min.x - column.max.x)).abs();
        assert!(between && off_centre <= 1.0, "{column:?} at {width}x{height} is not centred after {gutter_end}");
        let figures_foot = layout.figures_at().y + tall;
        let gpu_figures_foot = layout.gpu_figures_at().y + tall;
        for (section, (foot, top)) in
            sections.into_iter().zip([(figures_foot, gpu.min.y), (gpu_figures_foot, rows.min.y)])
        {
            let in_gap = section.min.y > foot && section.max.y < top;
            let to_gutter_end = (section.max.x - gutter_end).abs() <= 0.5;
            let across = section.min.x == chart.min.x && to_gutter_end && section.max.x < column.min.x;
            assert!(in_gap && across, "{section:?} at {width}x{height} is not between {foot} and {top}");
        }
        for divider in [column, sections[0], sections[1]] {
            assert!(divider.width().min(divider.height()) == thick, "{divider:?} is not a line {thick} thick");
            for area in &drawn {
                assert!(!divider.intersects(*area), "{divider:?} at {width}x{height} is drawn over {area:?}");
            }
        }
    }
}

/// The windows the band is laid out on: 1080, an ultrawide 1373, a narrow 720, and 1080 on a screen of twice the
/// pixels.
const SIZES: [(u32, u32, f64); 4] = [(1920, 1080, 1.0), (3440, 1373, 1.0), (1280, 720, 1.0), (1920, 1080, 2.0)];

/// The GPU's memory the meters show: 1.5 GiB.
const GPU_BYTES: u64 = 3 << 29;

/// How many meters the right column holds, a row each.
fn meters() -> f32 {
    u8::try_from(Meter::ALL.len()).map_or(f32::MAX, f32::from)
}

/// The engine's memory as a running program reads it: the program resident, the blob store part full, a backlog held
/// by a connection with a long path, and no guests.
fn memory() -> EngineMemory {
    EngineMemory::Read(MemoryFigures {
        process: ProcessMemory::Resident { bytes: 412 << 20 },
        blobs: Held { bytes: 300 << 20, most: 8 << 30 },
        backlog: Backlog::Held {
            unsent: Held { bytes: 12 << 10, most: 16 << 20 },
            actor: String::from("aether.tcp/connection:1234567890"),
        },
        guest_bytes: 0,
    })
}
