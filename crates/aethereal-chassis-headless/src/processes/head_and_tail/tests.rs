use super::{CUT, HEAD_BYTES, Kept, LONGEST_LINE_CHARACTERS, TAIL_BYTES};

/// The first line of the panic a program's stderr begins with.
const PANICKED: &str = "thread 'main' panicked at crates/engine/src/main.rs:12:5:";
/// The last line it writes.
const ABORTING: &str = "thread caused non-unwinding panic. aborting.";

/// What `stderr` comes to, read a few bytes at a time, so a line and a character are split across reads.
fn kept(stderr: &str) -> String {
    let mut kept = Kept::default();
    stderr.as_bytes().chunks(7).for_each(|read| kept.keep(read));

    kept.text()
}

/// A panic's stderr: its first line, then `frames` backtrace frames, each a generic type name far past the longest
/// line, then the abort.
fn panic(frames: usize) -> String {
    let name = "aethereal_actor::path::roots::There<".repeat(40);
    let frame = format!("  0: {name}\n      at crates/actor/src/path.rs:1:9\n");

    format!(
        "{PANICKED}\nthe first panic's message\nstack backtrace:\n{}panic in a destructor during cleanup\n{ABORTING}\n",
        frame.repeat(frames)
    )
}

#[test]
fn a_long_panic_keeps_its_first_line_and_its_last_and_says_what_was_left_out() {
    // Catches a head not kept (only the tail, so the first `panicked at` is lost), a tail not kept, a marker missing or
    // counting the lines left out one off, and a text past its bound.
    let stderr = panic(200);

    let text = kept(&stderr);

    let lines: Vec<&str> = text.lines().collect();
    let marker = lines.iter().position(|line| line.starts_with(CUT)).expect("a marker says what was left out");
    let left_out: usize =
        lines[marker].split_whitespace().nth(1).and_then(|lines| lines.parse().ok()).expect("a count");
    let longest = HEAD_BYTES + TAIL_BYTES + lines[marker].len() + 1;
    assert_eq!(lines.first(), Some(&PANICKED));
    assert_eq!(lines.last(), Some(&ABORTING));
    assert_eq!(lines.len() - 1 + left_out, stderr.lines().count());
    assert!(text.len() <= longest, "{} bytes, past {longest}", text.len());
}

#[test]
fn a_short_stderr_is_kept_whole() {
    // Catches a marker or a cut where nothing was left out, and a head or a tail dropped when both fit.
    let middle = format!("{}\n", "x".repeat(LONGEST_LINE_CHARACTERS)).repeat(HEAD_BYTES / LONGEST_LINE_CHARACTERS + 1);
    let stderr = format!("{PANICKED}\n{middle}{ABORTING}\n");

    assert_eq!(kept(&stderr), stderr);
}

#[test]
fn a_character_at_a_cut_is_never_split() {
    // Catches a cut at a byte rather than a character, at a line's end or at the head's or the tail's, which leaves a
    // replaced half of a character.
    let mut stderr = String::new();
    for line in 0..400 {
        stderr.push_str(&"é".repeat(line % 7 + 1));
        stderr.push_str(&"語".repeat(LONGEST_LINE_CHARACTERS + line % 5));
        stderr.push('\n');
    }

    let text = kept(&stderr);

    assert!(text.contains(CUT));
    assert!(!text.contains(char::REPLACEMENT_CHARACTER), "a character was split: {text}");
}

#[test]
fn an_over_long_line_is_cut_and_the_next_kept() {
    // Catches a line kept past its longest, one cut a character off it, and the line after a cut one dropped too.
    let long = "a".repeat(LONGEST_LINE_CHARACTERS * 3);
    let stderr = format!("{long}\nthe next line\n");

    let expected = format!("{}{CUT}\nthe next line\n", "a".repeat(LONGEST_LINE_CHARACTERS));
    assert_eq!(kept(&stderr), expected);
}
