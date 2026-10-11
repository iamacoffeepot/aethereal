//! The string grammar and its escapes.

use super::cursor::Cursor;
use crate::{Budget, JsonError, TextFault};
use alloc::string::String;

/// Reads one string, from its opening quote to its closing quote, with escapes resolved.
pub(super) fn string(cursor: &mut Cursor<'_>, budget: &mut Budget) -> Result<String, JsonError> {
    let opening = cursor.peek();
    let is_quote = opening == Some(b'"');
    if !is_quote {
        let end = opening.is_none();
        if end {
            return Err(cursor.fault(TextFault::UnexpectedEnd));
        }
        return Err(cursor.fault(TextFault::UnexpectedCharacter));
    }
    cursor.advance();

    let mut out = String::new();
    let mut run_start = cursor.offset();
    loop {
        let next = cursor.peek();
        let Some(byte) = next else {
            return Err(cursor.fault(TextFault::UnexpectedEnd));
        };
        let closing = byte == b'"';
        if closing {
            let run = cursor.slice(run_start);
            budget.charge(run.len())?;
            out.push_str(run);
            cursor.advance();
            return Ok(out);
        }
        let escaping = byte == b'\\';
        if escaping {
            let run = cursor.slice(run_start);
            budget.charge(run.len())?;
            out.push_str(run);
            cursor.advance();
            escape(cursor, budget, &mut out)?;
            run_start = cursor.offset();
            continue;
        }
        let control = byte < 0x20;
        if control {
            return Err(cursor.fault(TextFault::ControlCharacter));
        }
        cursor.advance();
    }
}

/// Resolves one escape after the backslash, appending what it gives.
fn escape(cursor: &mut Cursor<'_>, budget: &mut Budget, out: &mut String) -> Result<(), JsonError> {
    let next = cursor.peek();
    let Some(byte) = next else {
        return Err(cursor.fault(TextFault::UnexpectedEnd));
    };
    match byte {
        b'"' => {
            budget.charge(1)?;
            out.push('"');
            cursor.advance();
            Ok(())
        }
        b'\\' => {
            budget.charge(1)?;
            out.push('\\');
            cursor.advance();
            Ok(())
        }
        b'/' => {
            budget.charge(1)?;
            out.push('/');
            cursor.advance();
            Ok(())
        }
        b'b' => {
            budget.charge(1)?;
            out.push('\u{0008}');
            cursor.advance();
            Ok(())
        }
        b'f' => {
            budget.charge(1)?;
            out.push('\u{000C}');
            cursor.advance();
            Ok(())
        }
        b'n' => {
            budget.charge(1)?;
            out.push('\n');
            cursor.advance();
            Ok(())
        }
        b'r' => {
            budget.charge(1)?;
            out.push('\r');
            cursor.advance();
            Ok(())
        }
        b't' => {
            budget.charge(1)?;
            out.push('\t');
            cursor.advance();
            Ok(())
        }
        b'u' => {
            cursor.advance();
            unicode(cursor, budget, out)
        }
        _ => Err(cursor.fault(TextFault::InvalidEscape)),
    }
}

/// Resolves `\uXXXX`, possibly a surrogate pair, after the `u`.
fn unicode(cursor: &mut Cursor<'_>, budget: &mut Budget, out: &mut String) -> Result<(), JsonError> {
    let Some(first) = take_unit(cursor)? else {
        return Err(cursor.fault(TextFault::InvalidEscape));
    };
    let value = u32::from(first);
    let Some(ch) = char::from_u32(value) else {
        return pair(cursor, budget, out, first);
    };
    budget.charge(ch.len_utf8())?;
    out.push(ch);
    Ok(())
}

/// Reads four hexadecimal digits, or `None` when they are not there.
fn take_unit(cursor: &mut Cursor<'_>) -> Result<Option<u16>, JsonError> {
    let start = cursor.offset();
    for _ in 0..4 {
        let next = cursor.peek();
        let Some(byte) = next else {
            return Ok(None);
        };
        let hex = byte.is_ascii_hexdigit();
        if !hex {
            return Ok(None);
        }
        cursor.advance();
    }
    let hex_text = cursor.slice(start);
    let parsed = u16::from_str_radix(hex_text, 16).map_err(|_| cursor.fault(TextFault::InvalidEscape))?;
    Ok(Some(parsed))
}

/// Resolves the second half of a surrogate pair headed by `first`.
fn pair(cursor: &mut Cursor<'_>, budget: &mut Budget, out: &mut String, first: u16) -> Result<(), JsonError> {
    let backslash = cursor.peek();
    let is_backslash = backslash == Some(b'\\');
    if !is_backslash {
        return Err(cursor.fault(TextFault::LoneSurrogate));
    }
    cursor.advance();
    let letter = cursor.peek();
    let is_u = letter == Some(b'u');
    if !is_u {
        return Err(cursor.fault(TextFault::LoneSurrogate));
    }
    cursor.advance();
    let Some(second) = take_unit(cursor)? else {
        return Err(cursor.fault(TextFault::LoneSurrogate));
    };
    let mut decoded = char::decode_utf16([first, second]);
    let Some(first_out) = decoded.next() else {
        return Err(cursor.fault(TextFault::LoneSurrogate));
    };
    let Ok(ch) = first_out else {
        return Err(cursor.fault(TextFault::LoneSurrogate));
    };
    let more = decoded.next();
    let single = more.is_none();
    if !single {
        return Err(cursor.fault(TextFault::LoneSurrogate));
    }
    budget.charge(ch.len_utf8())?;
    out.push(ch);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DecodeError, JsonFault};

    #[test]
    fn short_escapes_resolve() {
        // Catches an escape table that misses or swaps a short escape.
        let texts = [
            ("\"\\\"\"", "\""),
            ("\"\\\\\"", "\\"),
            ("\"\\/\"", "/"),
            ("\"\\b\"", "\u{0008}"),
            ("\"\\f\"", "\u{000C}"),
            ("\"\\n\"", "\n"),
            ("\"\\r\"", "\r"),
            ("\"\\t\"", "\t"),
        ];
        for (json, expected) in texts {
            let mut cursor = Cursor::new(json);
            let mut budget = Budget::new(64);
            assert_eq!(string(&mut cursor, &mut budget), Ok(String::from(expected)));
        }
    }

    #[test]
    fn unicode_escapes_resolve() {
        // Catches a `\u` read that rejects uppercase or misplaces a pair.
        let mut cursor = Cursor::new("\"\\u00e9\"");
        let mut budget = Budget::new(64);
        assert_eq!(string(&mut cursor, &mut budget), Ok(String::from("é")));
        let mut lower = Cursor::new("\"\\u00E9\"");
        let mut lower_budget = Budget::new(64);
        assert_eq!(string(&mut lower, &mut lower_budget), Ok(String::from("é")));
        let mut pair = Cursor::new("\"\\ud83d\\ude00\"");
        let mut pair_budget = Budget::new(64);
        assert_eq!(string(&mut pair, &mut pair_budget), Ok(String::from("\u{1F600}")));
    }

    #[test]
    fn lone_surrogates_are_refused() {
        // Catches a surrogate read that keeps half a pair.
        let texts = ["\"\\ud83d\"", "\"\\ude00\"", "\"\\ud83dA\""];
        for text in texts {
            let mut cursor = Cursor::new(text);
            let mut budget = Budget::new(64);
            let result = string(&mut cursor, &mut budget);
            let fault = result.map(|_| ()).map_err(|error| error.fault());
            let is_lone = matches!(fault, Err(JsonFault::Text { fault: TextFault::LoneSurrogate, .. }));
            assert!(is_lone);
        }
    }

    #[test]
    fn bad_escapes_are_refused() {
        // Catches an escape read that leans on `from_str_radix` alone, which accepts `+`.
        let texts = ["\"\\u+123\"", "\"\\u12\"", "\"\\x\""];
        for text in texts {
            let mut cursor = Cursor::new(text);
            let mut budget = Budget::new(64);
            let result = string(&mut cursor, &mut budget);
            let fault = result.map(|_| ()).map_err(|error| error.fault());
            let is_escape = matches!(fault, Err(JsonFault::Text { fault: TextFault::InvalidEscape, .. }));
            assert!(is_escape);
        }
    }

    #[test]
    fn control_and_end_are_refused() {
        // Catches a string read that lets a raw control through or runs past the end.
        let mut tab = Cursor::new("\"\t\"");
        let mut tab_budget = Budget::new(64);
        let tab_result = string(&mut tab, &mut tab_budget);
        let tab_fault = tab_result.map(|_| ()).map_err(|error| error.fault());
        assert!(matches!(tab_fault, Err(JsonFault::Text { fault: TextFault::ControlCharacter, .. })));

        let mut open = Cursor::new("\"abc");
        let mut open_budget = Budget::new(64);
        let open_result = string(&mut open, &mut open_budget);
        let open_fault = open_result.map(|_| ()).map_err(|error| error.fault());
        assert!(matches!(open_fault, Err(JsonFault::Text { fault: TextFault::UnexpectedEnd, .. })));
    }

    #[test]
    fn budget_charges_runs_and_escapes() {
        // Catches a string read that holds bytes it never charged.
        let mut cursor = Cursor::new("\"0123456789abcdef\"");
        let mut budget = Budget::new(8);
        let result = string(&mut cursor, &mut budget);
        let fault = result.map(|_| ()).map_err(|error| error.fault());
        assert!(matches!(fault, Err(JsonFault::Decode(DecodeError::BudgetExceeded { .. }))));
    }

    #[test]
    fn unescaped_non_ascii_passes_through() {
        // Catches a string read that escapes what ADR-0007 leaves alone.
        let mut cursor = Cursor::new("\"é\u{1F600}\"");
        let mut budget = Budget::new(64);
        assert_eq!(string(&mut cursor, &mut budget), Ok(String::from("é\u{1F600}")));
    }
}
