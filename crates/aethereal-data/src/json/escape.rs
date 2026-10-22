//! A string written as a JSON string.

use alloc::string::String;
use core::fmt::Write as _;

/// Appends `text` as a JSON string, quotes included.
pub fn write(text: &str, out: &mut String) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000C}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => {
                let value = u32::from(ch);
                let control = value < 0x20;
                if control {
                    let _ = write!(out, "\\u{value:04x}");
                } else {
                    out.push(ch);
                }
            }
        }
    }
    out.push('"');
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
}
