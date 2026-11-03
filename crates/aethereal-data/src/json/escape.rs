//! A string written as a JSON string.

use alloc::string::String;
use core::fmt::Write as _;

/// The bytes the scan tests in one step.
const WORD: usize = 8;
/// A one in every byte of a word.
const ONES: u64 = u64::from_ne_bytes([0x01; WORD]);
/// The low seven bits of every byte.
const LOW_SEVEN: u64 = ONES * 0x7F;
/// The high bit of every byte.
const HIGH: u64 = ONES * 0x80;
/// A quote in every byte.
const QUOTES: u64 = u64::from_ne_bytes([b'"'; WORD]);
/// A backslash in every byte.
const BACKSLASHES: u64 = u64::from_ne_bytes([b'\\'; WORD]);

/// Appends `text` as a JSON string, quotes included.
///
/// The text between escapes is copied a run at a time. Every byte that needs one is ASCII, so it never falls inside a
/// character and each run starts and ends on a character boundary.
pub fn write(text: &str, out: &mut String) {
    out.reserve(text.len() + 2);
    out.push('"');
    let bytes = text.as_bytes();
    let mut run = 0;
    while let Some(at) = next_escape(bytes, run) {
        out.push_str(&text[run..at]);
        escape(bytes[at], out);
        run = at + 1;
    }
    out.push_str(&text[run..]);
    out.push('"');
}

/// The place of the first byte at or after `from` that needs an escape, or none when the rest is clean.
///
/// The rest is tested a word at a time, and the bytes past its last whole word one at a time.
fn next_escape(bytes: &[u8], from: usize) -> Option<usize> {
    let rest = &bytes[from..];
    let (words, tail) = rest.as_chunks::<WORD>();
    for (index, word) in words.iter().enumerate() {
        let marked = marks(u64::from_le_bytes(*word));
        if marked != 0 {
            // Read little-endian, the first byte is the lowest, so the lowest mark is the first byte to escape.
            let within = (marked.trailing_zeros() / 8) as usize;
            return Some(from + index * WORD + within);
        }
    }
    let tail_at = from + words.len() * WORD;

    tail.iter().position(|&byte| needs_escape(byte)).map(|at| tail_at + at)
}

/// Whether `byte` needs an escape: a quote, a backslash, or a control character.
const fn needs_escape(byte: u8) -> bool {
    byte < 0x20 || byte == b'"' || byte == b'\\'
}

/// The high bit of each byte of `word` that needs an escape, and no other bit.
///
/// Each sum adds at most `0x7F` to a byte's low seven bits, so it never carries into the next byte: every byte's mark
/// is its own, and none is set by its neighbour.
const fn marks(word: u64) -> u64 {
    // A byte's high bit, once the low seven are raised by `0x60`, is set when they are at least `0x20`.
    let space_or_more = (word & LOW_SEVEN) + ONES * (0x80 - 0x20);
    let control = !(space_or_more | word);
    let quote = zero_bytes(word ^ QUOTES);
    let backslash = zero_bytes(word ^ BACKSLASHES);

    (control | quote | backslash) & HIGH
}

/// The high bit of each byte of `word` that is zero.
const fn zero_bytes(word: u64) -> u64 {
    // A byte's high bit, once the low seven are raised by `0x7F`, is set when any of them is.
    let low_set = (word & LOW_SEVEN) + LOW_SEVEN;

    !(low_set | word) & HIGH
}

/// Appends the escape of `byte`: a quote, a backslash, or a control character.
fn escape(byte: u8, out: &mut String) {
    match byte {
        b'"' => out.push_str("\\\""),
        b'\\' => out.push_str("\\\\"),
        0x08 => out.push_str("\\b"),
        0x0C => out.push_str("\\f"),
        b'\n' => out.push_str("\\n"),
        b'\r' => out.push_str("\\r"),
        b'\t' => out.push_str("\\t"),
        _ => {
            let _ = write!(out, "\\u{byte:04x}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    /// Longer than two words, so a place lands in each byte of a word and in the tail past the last one.
    const SPAN: usize = 2 * WORD + 5;

    /// `special` at `at` among clean bytes, `SPAN` bytes and `special` in all.
    fn placed(special: char, at: usize) -> String {
        let after = SPAN - at - special.len_utf8();
        format!("{}{special}{}", "a".repeat(at), "a".repeat(after))
    }

    #[test]
    fn escapes_only_what_adr_0007_lists() {
        // Catches a writer that escapes `/`, `U+007F`, or non-ASCII, or misses a short escape.
        let text = "\"\\\n\u{1}/\u{7f}é";
        let mut out = String::new();
        write(text, &mut out);
        assert_eq!(out, "\"\\\"\\\\\\n\\u0001/\u{7f}é\"");
    }

    #[test]
    fn copies_text_around_escapes_whole() {
        // Catches a run copy that drops or repeats bytes beside an escape, or splits a character next to one.
        let text = "é\"ab\u{1f}\u{1f600}\n";
        let mut out = String::new();
        write(text, &mut out);
        assert_eq!(out, "\"é\\\"ab\\u001f\u{1f600}\\n\"");
    }

    #[test]
    fn escapes_a_byte_at_every_place_in_a_word_and_the_tail() {
        // Catches a word scan that reads its mark as the wrong byte, skips a word, or misses the tail.
        for (special, escaped) in [('"', "\\\""), ('\\', "\\\\"), ('\u{0}', "\\u0000"), ('\u{1f}', "\\u001f")] {
            for at in 0..SPAN {
                let mut out = String::new();
                write(&placed(special, at), &mut out);
                let expected = format!("\"{}{escaped}{}\"", "a".repeat(at), "a".repeat(SPAN - at - 1));
                assert_eq!(out, expected, "{special:?} at {at}");
            }
        }
    }

    #[test]
    fn leaves_the_bytes_beside_an_escaped_one_at_every_place() {
        // Catches a word test whose sums carry or borrow between bytes, or that marks a byte one off what it tests. The
        // last three are bytes above `0x7F` whose low seven bits are a control, a quote, and a backslash.
        for neighbour in [' ', '!', '#', '[', ']', '\u{7f}', '\u{80}', '\u{a2}', '\u{700}'] {
            for at in 0..=SPAN - neighbour.len_utf8() {
                let text = placed(neighbour, at);
                let mut out = String::new();
                write(&text, &mut out);
                assert_eq!(out, format!("\"{text}\""), "{neighbour:?} at {at}");
            }
        }
    }
}
