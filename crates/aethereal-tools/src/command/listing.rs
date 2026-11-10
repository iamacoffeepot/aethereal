//! The bare `help` command: every tool, one row each, in columns (ADR-0053).

use super::CommandLine;
use super::layout::{GAP, cut, padded, width_of};
use crate::ToolSheet;

/// The fewest characters a summary is given beside the usage; with less room, it goes on a line beneath.
const LEAST_SUMMARY: usize = 24;

/// How far a summary on a line beneath its usage is indented.
const SUMMARY_INDENT: &str = "    ";

/// One tool's row: its usage's three pieces and its summary.
struct Row {
    /// The name, the fields filled by position, and the flags, each padded to its column.
    cells: [String; 3],
    /// The first sentence of the description.
    summary: String,
}

impl Row {
    /// The row of `sheet`; one whose flags do not fit its input says so in place of its summary.
    fn of(sheet: &ToolSheet) -> Self {
        match CommandLine::of(sheet) {
            Ok(line) => {
                let usage = line.usage_parts();
                let cells = [String::from(usage.name), usage.positionals, usage.flags];
                Self { cells, summary: String::from(line.summary()) }
            }
            Err(fault) => {
                let cells = [sheet.name.clone(), String::new(), String::new()];
                Self { cells, summary: format!("its flags do not fit its input: {fault}") }
            }
        }
    }

    /// The row's usage, each cell padded to its column's width in `widths`, a column empty in every row left out.
    fn usage(&self, widths: &[usize; 3]) -> String {
        let mut line = String::new();
        for (cell, width) in self.cells.iter().zip(widths).filter(|(_, width)| **width > 0) {
            if !line.is_empty() {
                line.push_str(GAP);
            }
            line.push_str(&padded(cell, *width));
        }

        line
    }
}

/// The bare `help` command's text for `sheets`, laid out for a console `width` characters wide: one row per tool,
/// sorted by name, in three columns, each padded to its widest: the name, the fields filled by position, and the
/// flags. Where the line leaves room, a fourth column holds the description's first sentence, cut with `…` at the
/// width; where it does not, the sentence goes on an indented line beneath, cut at the width.
///
/// ```text
/// actor_logs     <actor>          [--since N] [--level debug|info|warn|error] …  Reads what the actor at …
/// capture_frame  <window>         [-c whole] [-l N]                              Captures the next frame …
/// record_clip    <window> <name>  -n N [-r N] [-c whole] [-l N]                  Records frames drawn …
/// ```
#[must_use]
pub fn help_listing(sheets: &[ToolSheet], width: usize) -> String {
    let mut rows: Vec<Row> = sheets.iter().map(Row::of).collect();
    rows.sort_by(|first, second| first.cells[0].cmp(&second.cells[0]));
    let mut widths = [0; 3];
    for row in &rows {
        for (widest, cell) in widths.iter_mut().zip(&row.cells) {
            *widest = (*widest).max(width_of(cell));
        }
    }
    let shown = widths.iter().filter(|width| **width > 0).count();
    let usage_width = widths.iter().sum::<usize>() + GAP.len() * shown.saturating_sub(1);
    let room = width.saturating_sub(usage_width + GAP.len());
    let summary_beside = room >= LEAST_SUMMARY;

    let mut lines = Vec::new();
    for row in &rows {
        let usage = row.usage(&widths);
        if summary_beside {
            lines.push(format!("{usage}{GAP}{}", cut(&row.summary, room)).trim_end().to_owned());
            continue;
        }
        lines.push(usage.trim_end().to_owned());
        let summarised = !row.summary.is_empty();
        if summarised {
            let beneath = width.saturating_sub(SUMMARY_INDENT.len());
            lines.push(format!("{SUMMARY_INDENT}{}", cut(&row.summary, beneath)));
        }
    }

    lines.join("\n")
}
