//! The console's tests: the line stepped through its history, a line turned into a call of a real tool, refusals
//! printed in the error's ink, tab completion of a tool, a flag, and a variant and its hint, the scrollback's bound and lines, the
//! band's count of warnings and errors, and the calls running.

use super::scrollback::{MOST_ROWS, Scrollback};
use super::*;
use crate::colour::Ink;
use crate::tools::OverlayTools;
use aethereal_actor::Level;
use aethereal_data::from_bytes;
use aethereal_tools::{LogLevel, LogsTool, ReadRecent, Tool};
use core::error::Error;

/// A console holding the engine's own log tools and the overlay's, as the tools root of revision 3 sends them.
fn holding_tools() -> Console {
    let mut console = Console::default();
    let tools = [LogsTool::sheets(), OverlayTools::sheets()].concat();
    console.hear(Sheets { revision: 3, tools });

    console
}

/// Each row kept, its text and the ink of its last piece.
fn shown(console: &Console) -> Vec<(String, Ink)> {
    console.rows().map(|row| (row.text(), row.pieces.last().map_or(Ink::Text, |piece| piece.ink))).collect()
}

/// Submits `line` at `console`, as its text input does with Enter.
fn enter(console: &mut Console, line: &str) -> Command {
    console.submitted(line)
}

/// Types `text` on at the end of `console`'s line, as its text input raises the edit, and completes it with the caret
/// at its end, as Tab does.
fn complete(console: &mut Console, text: &str) -> String {
    console.edited(format!("{}{text}", console.line()));
    console.complete(console.line().len());

    String::from(console.line())
}

/// Up steps back through the commands entered, newest first, and Down forward again, back to the line that was being
/// typed, so a history that steps the wrong way, loses the draft, or keeps a repeated command twice is caught.
#[test]
fn up_and_down_step_through_the_history() {
    let mut console = holding_tools();
    for command in ["help", "clear", "clear"] {
        enter(&mut console, command);
    }
    console.edited(String::from("me"));

    console.older();
    let newest = String::from(console.line());
    console.older();
    let older = String::from(console.line());
    console.older();
    let oldest_kept = String::from(console.line());
    console.newer();
    console.newer();
    let draft = String::from(console.line());

    assert_eq!((newest.as_str(), older.as_str(), oldest_kept.as_str()), ("clear", "help", "help"));
    assert_eq!(draft, "me");
}

/// A line naming one of the engine's own tools is read by its sheet's flags into the bytes of its input, of the
/// sheets' revision, so a flag read into the wrong field, a revision not carried, or an echo left out is caught.
#[test]
fn a_line_becomes_a_call_of_its_tool() -> Result<(), Box<dyn Error>> {
    let mut console = holding_tools();

    let command = enter(&mut console, "read_recent --level warn --most 5");

    let Command::Call { tool, revision, input, .. } = command else {
        return Err(format!("{command:?} is not a call").into());
    };
    let read: ReadRecent = from_bytes(&input, &mut Budget::new(1024))?;
    assert_eq!((tool.as_str(), revision), ("read_recent", 3));
    assert_eq!(read, ReadRecent { since: None, level: Some(LogLevel::Warn), contains: None, most: Some(5) });
    assert_eq!(shown(&console), vec![(String::from("local ! read_recent --level warn --most 5"), Ink::Lichen)]);
    assert_eq!(console.line(), "", "the line is left for the next");

    Ok(())
}

/// A tool the engine does not hold, a line its flags do not read, and a tool's refusal each print in the error's ink
/// after the command's echo, the owner's line on the raised band, its tag in the label's ink and its text in lichen;
/// an unknown tool with the nearest names. So a refusal that reads as an answer, or an echo that reads as one, is
/// caught.
#[test]
fn refusals_print_in_the_error_s_ink_after_the_owner_s_line() {
    let mut console = holding_tools();

    let unknown = enter(&mut console, "memroy");
    enter(&mut console, "read_recent --level loud");
    console.refused("the tool did not answer in time");

    let rows = shown(&console);
    assert!(matches!(unknown, Command::Refused(_)), "{unknown:?}");
    let echo = console.rows().next();
    assert_eq!(rows[0], (String::from("local ! memroy"), Ink::Lichen));
    assert_eq!(echo.map(|row| (row.shade, row.pieces[0].ink)), Some((Shade::Raised, Ink::Label)));
    assert_eq!(rows[1].1, Ink::Error);
    assert!(rows[1].0.contains("`memory`"), "{:?} does not name the nearest tool", rows[1].0);
    assert_eq!((rows[2].1, rows[3].1, rows[4].1), (Ink::Lichen, Ink::Error, Ink::Error));
}

/// Tab completes the first word as a tool's name, a word that starts with `-` as one of the tool's flags, and the
/// word after a flag whose field is a variant as one of its words; and several matches complete as far as they agree
/// and print on one line, so a completion from the wrong list, or one that loses what was typed, is caught.
#[test]
fn tab_completes_a_tool_a_flag_and_a_variant() {
    let mut console = holding_tools();

    let tool = complete(&mut console, "read_r");
    let flag = complete(&mut console, "--le");
    let variant = complete(&mut console, "wa");

    assert_eq!(tool, "read_recent ");
    assert_eq!(flag, "read_recent --level ");
    assert_eq!(variant, "read_recent --level warn ");

    let mut several = holding_tools();
    assert_eq!(complete(&mut several, "m"), "m");
    assert_eq!(shown(&several), vec![(String::from("mark  memory"), Ink::Dim)]);
}

/// The one word the word before the caret completes to is what shows ahead of it, and nothing does once the word is
/// whole or with the caret short of the line's end, so a hint that repeats what was typed, shows for several matches,
/// or stands in the middle of the line is caught.
#[test]
fn a_completion_shows_ahead_of_the_caret() {
    let mut console = holding_tools();

    console.edited(String::from("mem"));
    let hinted = console.completion_ahead(3);
    let short = console.completion_ahead(2);
    console.edited(String::from("memory"));
    let whole = console.completion_ahead(6);

    assert_eq!(hinted, "ory");
    assert_eq!(short, "");
    assert_eq!(whole, "");
}

/// The scrollback keeps the newest 2,000 rows, and a text printed is a row for each of its lines, however long, which
/// the scroll wraps as it lays them out, so a bound off by one, a line lost or joined to the next, or a line cut into
/// rows here is caught.
#[test]
fn the_scrollback_keeps_two_thousand_rows_a_row_for_each_line() {
    let mut scrollback = Scrollback::default();
    for number in 0..2_500 {
        scrollback.print(&format!("row {number}"), Ink::Text);
    }
    let long = "the quick brown fox jumps ".repeat(20);
    let mut lines = Scrollback::default();
    lines.print(&format!("{long}\n\nlast"), Ink::Text);

    let first = scrollback.rows().next();
    assert_eq!(scrollback.rows().len(), MOST_ROWS);
    assert_eq!(first.map(Row::text), Some(String::from("row 500")));
    assert_eq!(first.map(|row| row.number), Some(500), "a row keeps the number it was printed under");
    let lines: Vec<String> = lines.rows().map(Row::text).collect();
    assert_eq!(lines, [long.as_str(), "", "last"]);
}

/// Warnings and errors are counted, in the warning's and the error's inks, flush right in the gutter's line after `alerts`, and nothing
/// else is; and clearing forgets them, leaving `no alerts` rather than a blank line, so a count of the wrong level, one
/// the console's opening does not clear, counts out of line with the gutter's values, a line that what follows the
/// counts makes wider than the gutter, or a blank line, is caught.
#[test]
fn warnings_and_errors_are_counted_until_cleared() {
    let mut alerts = Alerts::default();
    for level in [Level::Info, Level::Warn, Level::Warn, Level::Error, Level::Debug] {
        alerts.count(level);
    }

    let counted = alerts.pieces(Vec::new());
    let waiting = alerts.pieces(vec![Piece::new(" 3?", Ink::Mark)]);
    alerts.clear();
    let cleared = alerts.pieces(Vec::new());

    let line: String = counted.iter().map(|piece| piece.text.as_str()).collect();
    let waiting: String = waiting.iter().map(|piece| piece.text.as_str()).collect();
    assert_eq!(line, "alerts      ! 2  × 1");
    assert_eq!(waiting, "alerts   ! 2  × 1 3?", "what follows the counts keeps the line the gutter's width");
    assert_eq!(counted[2..], [Piece::new("! 2", Ink::Warn), Piece::new("  ", Ink::Dim), Piece::new("× 1", Ink::Error)]);
    assert_eq!(cleared, vec![Piece::new("no alerts", Ink::Dim)]);
}

/// A call is shown running from when it is asked until it settles, one by its tool's name and several by their count,
/// each settling letting go of one call only, so a running line that never clears, clears at the first of two calls
/// of one tool, or counts a call twice, is caught.
#[test]
fn calls_run_until_each_settles() {
    let mut console = Console::default();
    let shown = |console: &Console| console.running().iter().map(|piece| piece.text.clone()).collect::<String>();

    console.called("read_recent");
    let one = shown(&console);
    console.called("read_recent");
    console.called("memory");
    let three = shown(&console);
    console.settled("read_recent");
    console.settled("unknown");
    let two = shown(&console);
    console.settled("memory");
    let last = shown(&console);
    console.settled("read_recent");

    assert_eq!((one.as_str(), three.as_str(), two.as_str()), ("running read_recent…", "running 3…", "running 2…"));
    assert_eq!(last, "running read_recent…");
    assert_eq!(shown(&console), "");
}
