//! A string written as a JSON string.

use alloc::string::String;
use core::fmt::Write as _;

/// Appends `text` as a JSON string, quotes included.
///
/// The text between escapes is copied a run at a time. Every byte that needs one is ASCII, so it never falls inside a
/// character and each run starts and ends on a character boundary.
pub fn write(text: &str, out: &mut String) {
    out.reserve(text.len() + 2);
    out.push('"');
    let mut run = 0;
    for (at, byte) in text.bytes().enumerate() {
        let control = byte < 0x20;
        let needs_escape = control || byte == b'"' || byte == b'\\';
        if needs_escape {
            out.push_str(&text[run..at]);
            escape(byte, out);
            run = at + 1;
        }
    }
    out.push_str(&text[run..]);
    out.push('"');
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
}
