//! The console's tests: the line stepped through its history, a line turned into a call of a real tool, refusals
//! printed in brick, tab completion of a tool, a flag, and a variant and its hint, the scrollback's bound and wrap, the
//! band's count of warnings and errors, and the calls running.

use super::scrollback::{MOST_ROWS, Scrollback, wrapped};
use super::*;
use crate::colour::{BRICK, RUST, Swatch};
use crate::tools::OverlayTools;
use aethereal_actor::Level;
use aethereal_data::from_bytes;
use aethereal_tools::{LogLevel, LogsTool, ReadRecent, Tool};
use core::error::Error;

/// How many characters wide a console's rows are at 1080.
const COLUMNS: usize = 80;

/// A console holding the engine's own log tools and the overlay's, as the tools root of revision 3 sends them.
fn holding_tools() -> Console {
    let mut console = Console::default();
    let tools = [LogsTool::sheets(), OverlayTools::sheets()].concat();
    console.hear(Sheets { revision: 3, tools });

    console
}

/// Each row kept, its text and the colour of its last piece.
fn shown(console: &Console) -> Vec<(String, Swatch)> {
    console.rows().map(|row| (row.text(), row.pieces.last().map_or(BONE, |piece| piece.swatch))).collect()
}

/// Submits `line` at `console`, as its text input does with Enter.
fn enter(console: &mut Console, line: &str) -> Command {
    console.submitted(line, COLUMNS)
}

/// Types `text` on at the end of `console`'s line, as its text input raises the edit, and completes it with the caret
/// at its end, as Tab does.
fn complete(console: &mut Console, text: &str) -> String {
    console.edited(format!("{}{text}", console.line()));
    console.complete(console.line().len(), COLUMNS);

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
    assert_eq!(shown(&console), vec![(String::from("> read_recent --level warn --most 5"), LICHEN)]);
    assert_eq!(console.line(), "", "the line is left for the next");

    Ok(())
}

/// A tool the engine does not hold, a line its flags do not read, and a tool's refusal each print in brick after the
/// command's echo in lichen, an unknown tool with the nearest names, so a refusal that reads as an answer is caught.
#[test]
fn refusals_print_in_brick() {
    let mut console = holding_tools();

    let unknown = enter(&mut console, "memroy");
    enter(&mut console, "read_recent --level loud");
    console.refused("the tool did not answer in time", COLUMNS);

    let rows = shown(&console);
    assert!(matches!(unknown, Command::Refused(_)), "{unknown:?}");
    assert_eq!(rows[0], (String::from("> memroy"), LICHEN));
    assert_eq!(rows[1].1, BRICK);
    assert!(rows[1].0.contains("`memory`"), "{:?} does not name the nearest tool", rows[1].0);
    assert_eq!((rows[2].1, rows[3].1, rows[4].1), (LICHEN, BRICK, BRICK));
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
    assert_eq!(shown(&several), vec![(String::from("mark  memory"), STONE)]);
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

/// The scrollback keeps the newest 2,000 rows, and a line wider than the console wraps at a space past the middle
/// of the row, or at the width when there is none, so a bound off by one or a row wider than the console is caught.
#[test]
fn the_scrollback_keeps_two_thousand_rows_and_wraps_at_the_width() {
    let mut scrollback = Scrollback::default();
    for number in 0..2_500 {
        scrollback.print(&format!("row {number}"), BONE, 80);
    }

    let words = wrapped(&[Piece::new("the quick brown fox jumps", BONE)], 10);
    let unbroken = wrapped(&[Piece::new("abcdefghijklmnopqrstuvwxy", BONE)], 10);

    let first = scrollback.rows().next();
    assert_eq!(scrollback.rows().len(), MOST_ROWS);
    assert_eq!(first.map(Row::text), Some(String::from("row 500")));
    assert_eq!(first.map(|row| row.number), Some(500), "a row keeps the number it was printed under");
    let words: Vec<String> = words.iter().map(Row::text).collect();
    assert_eq!(words, ["the quick", "brown fox", "jumps"]);
    let unbroken: Vec<String> = unbroken.iter().map(Row::text).collect();
    assert_eq!(unbroken, ["abcdefghij", "klmnopqrst", "uvwxy"]);
}

/// Warnings and errors are counted, in rust and brick, flush right in the gutter's line after `alerts`, and nothing
/// else is; and clearing forgets them, leaving `no alerts` rather than a blank line, so a count of the wrong level, one
/// the console's opening does not clear, counts out of line with the gutter's values, or a blank line, is caught.
#[test]
fn warnings_and_errors_are_counted_until_cleared() {
    let mut alerts = Alerts::default();
    for level in [Level::Info, Level::Warn, Level::Warn, Level::Error, Level::Debug] {
        alerts.count(level);
    }

    let counted = alerts.pieces();
    alerts.clear();
    let cleared = alerts.pieces();

    let line: String = counted.iter().map(|piece| piece.text.as_str()).collect();
    assert_eq!(line, "alerts      ! 2  × 1");
    assert_eq!(counted[2..], [Piece::new("! 2", RUST), Piece::new("  ", STONE), Piece::new("× 1", BRICK)]);
    assert_eq!(cleared, vec![Piece::new("no alerts", STONE)]);
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
