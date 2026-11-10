//! The console's tests: the line edited and stepped through its history, a line turned into a call of a real tool,
//! refusals printed in brick, tab completion of a tool, a flag, and a variant, the scrollback's bound, wrap, and view,
//! and the band's count of warnings and errors.

use super::scrollback::{MOST_ROWS, Scrollback, wrapped};
use super::*;
use crate::colour::{BRICK, RUST, Swatch};
use crate::tools::OverlayTools;
use aethereal_actor::Level;
use aethereal_data::from_bytes;
use aethereal_tools::{LogLevel, LogsTool, ReadRecent, Tool};
use core::error::Error;

/// A console as wide and as tall as one at 1080.
const VIEW: View = View { columns: 80, rows: 10 };

/// A console holding the engine's own log tools and the overlay's, as the tools root of revision 3 sends them.
fn holding_tools() -> Console {
    let mut console = Console::default();
    let tools = [LogsTool::sheets(), OverlayTools::sheets()].concat();
    console.hear(Sheets { revision: 3, tools });

    console
}

/// The line as typed: the prompt's row without the prompt or a completion ahead of the cursor.
fn typed(console: &Console) -> String {
    console.prompt()[1].text.clone()
}

/// Each row in view, its text and the colour of its last piece.
fn shown(console: &Console) -> Vec<(String, Swatch)> {
    console.rows(VIEW).map(|row| (row.text(), row.pieces.last().map_or(BONE, |piece| piece.swatch))).collect()
}

/// Presses `key` at `console`, as the overlay hands it on.
fn press(console: &mut Console, key: Key) -> Command {
    console.pressed(Some(key), VIEW)
}

/// Typing inserts at the cursor, Backspace and Delete take the character either side of it, and Home, End, Left,
/// and Right move it, so an edit at the wrong side of the cursor or a cursor that runs off the line is caught.
#[test]
fn the_line_is_edited_at_the_cursor() {
    let mut console = holding_tools();

    console.typed("mrak");
    press(&mut console, Key::Left);
    press(&mut console, Key::Left);
    press(&mut console, Key::Backspace);
    console.typed("ar");
    press(&mut console, Key::Delete);
    press(&mut console, Key::Home);
    press(&mut console, Key::Left);
    console.typed("[");
    press(&mut console, Key::End);
    press(&mut console, Key::Right);
    console.typed("]");

    assert_eq!(typed(&console), "[mark]");
    assert_eq!(console.cursor_column(), 2 + 6);
}

/// Up steps back through the commands entered, newest first, and Down forward again, back to the line that was being
/// typed, so a history that steps the wrong way, loses the draft, or keeps a repeated command twice is caught.
#[test]
fn up_and_down_step_through_the_history() {
    let mut console = holding_tools();
    for command in ["help", "clear", "clear"] {
        console.typed(command);
        press(&mut console, Key::Enter);
    }
    console.typed("me");

    press(&mut console, Key::Up);
    let newest = typed(&console);
    press(&mut console, Key::Up);
    let older = typed(&console);
    press(&mut console, Key::Up);
    let oldest_kept = typed(&console);
    press(&mut console, Key::Down);
    press(&mut console, Key::Down);
    let draft = typed(&console);

    assert_eq!((newest.as_str(), older.as_str(), oldest_kept.as_str()), ("clear", "help", "help"));
    assert_eq!(draft, "me");
}

/// A line naming one of the engine's own tools is read by its sheet's flags into the bytes of its input, of the
/// sheets' revision, so a flag read into the wrong field, a revision not carried, or an echo left out is caught.
#[test]
fn a_line_becomes_a_call_of_its_tool() -> Result<(), Box<dyn Error>> {
    let mut console = holding_tools();

    console.typed("read_recent --level warn --most 5");
    let command = press(&mut console, Key::Enter);

    let Command::Call { tool, revision, input, .. } = command else {
        return Err(format!("{command:?} is not a call").into());
    };
    let read: ReadRecent = from_bytes(&input, &mut Budget::new(1024))?;
    assert_eq!((tool.as_str(), revision), ("read_recent", 3));
    assert_eq!(read, ReadRecent { since: None, level: Some(LogLevel::Warn), contains: None, most: Some(5) });
    assert_eq!(shown(&console), vec![(String::from("> read_recent --level warn --most 5"), LICHEN)]);

    Ok(())
}

/// A tool the engine does not hold, a line its flags do not read, and a tool's refusal each print in brick after the
/// command's echo in lichen, an unknown tool with the nearest names, so a refusal that reads as an answer is caught.
#[test]
fn refusals_print_in_brick() {
    let mut console = holding_tools();

    console.typed("memroy");
    let unknown = press(&mut console, Key::Enter);
    console.typed("read_recent --level loud");
    press(&mut console, Key::Enter);
    console.refused("the tool did not answer in time", VIEW);

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

    console.typed("read_r");
    press(&mut console, Key::Tab);
    let tool = typed(&console);
    console.typed("--le");
    press(&mut console, Key::Tab);
    let flag = typed(&console);
    console.typed("wa");
    press(&mut console, Key::Tab);
    let variant = typed(&console);

    assert_eq!(tool, "read_recent ");
    assert_eq!(flag, "read_recent --level ");
    assert_eq!(variant, "read_recent --level warn ");

    let mut several = holding_tools();
    several.typed("m");
    press(&mut several, Key::Tab);
    assert_eq!(typed(&several), "m");
    assert_eq!(shown(&several), vec![(String::from("mark  memory"), STONE)]);
}

/// The one word the word before the cursor completes to shows ahead of it in stone, and nothing does once the word
/// is whole, so a hint that repeats what was typed, or shows for several matches, is caught.
#[test]
fn a_completion_shows_ahead_of_the_cursor() {
    let mut console = holding_tools();

    console.typed("mem");
    let hinted = console.prompt();
    console.typed("ory");
    let whole = console.prompt();

    assert_eq!(hinted.last(), Some(&Piece::new("ory", STONE)));
    assert_eq!(whole.len(), 2);
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

    assert_eq!(scrollback.len(), MOST_ROWS);
    assert_eq!(scrollback.visible(MOST_ROWS).next().map(Row::text), Some(String::from("row 500")));
    let words: Vec<String> = words.iter().map(Row::text).collect();
    assert_eq!(words, ["the quick", "brown fox", "jumps"]);
    let unbroken: Vec<String> = unbroken.iter().map(Row::text).collect();
    assert_eq!(unbroken, ["abcdefghij", "klmnopqrst", "uvwxy"]);
}

/// Page Up shows the page above, a row short of a whole page so one row stays in view, and no further than the
/// oldest row; Page Down comes back; and a row printed while scrolled up leaves the view where it was, so a view that
/// jumps, overshoots, or follows new rows while read is caught.
#[test]
fn the_view_follows_page_up_and_page_down() {
    let mut scrollback = Scrollback::default();
    for number in 0..25 {
        scrollback.print(&format!("{number}"), BONE, 80);
    }
    let first_of = |scrollback: &Scrollback| scrollback.visible(10).next().map(Row::text);

    let newest = first_of(&scrollback);
    scrollback.page_up(10);
    let page_up = first_of(&scrollback);
    scrollback.print("25", BONE, 80);
    let kept = first_of(&scrollback);
    scrollback.page_up(10);
    scrollback.page_up(10);
    let oldest = first_of(&scrollback);
    scrollback.page_down(10);
    scrollback.page_down(10);
    scrollback.page_down(10);
    let back = first_of(&scrollback);

    assert_eq!(newest.as_deref(), Some("15"));
    assert_eq!(page_up.as_deref(), Some("6"));
    assert_eq!(kept.as_deref(), Some("6"));
    assert_eq!(oldest.as_deref(), Some("0"));
    assert_eq!(back.as_deref(), Some("16"));
}

/// Warnings and errors are counted, in rust and brick, and nothing else is; and clearing forgets them, so a count
/// of the wrong level, or one the console's opening does not clear, is caught.
#[test]
fn warnings_and_errors_are_counted_until_cleared() {
    let mut alerts = Alerts::default();
    for level in [Level::Info, Level::Warn, Level::Warn, Level::Error, Level::Debug] {
        alerts.count(level);
    }

    let counted = alerts.pieces();
    alerts.clear();
    let cleared = alerts.pieces();

    assert_eq!(counted, vec![Piece::new("! 2", RUST), Piece::new("  ", STONE), Piece::new("× 1", BRICK)]);
    assert_eq!(cleared, vec![Piece::new(" ", STONE)]);
}
