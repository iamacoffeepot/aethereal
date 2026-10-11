//! The number grammar (RFC 8259).

use super::cursor::Cursor;
use crate::{JsonError, TextFault};

/// Reads one number and returns its text.
pub(super) fn number<'json>(cursor: &mut Cursor<'json>) -> Result<&'json str, JsonError> {
    let start = cursor.offset();
    let first = cursor.peek();
    let negative = first == Some(b'-');
    if negative {
        cursor.advance();
    }

    let int = cursor.peek();
    let is_zero = int == Some(b'0');
    if is_zero {
        cursor.advance();
    } else {
        let leading = int.is_some_and(|byte| (b'1'..=b'9').contains(&byte));
        if !leading {
            let end = cursor.peek().is_none();
            if end {
                return Err(cursor.fault(TextFault::UnexpectedEnd));
            }
            return Err(cursor.fault(TextFault::UnexpectedCharacter));
        }
        cursor.advance();
        loop {
            let next = cursor.peek();
            let digit = next.is_some_and(|byte| byte.is_ascii_digit());
            if !digit {
                break;
            }
            cursor.advance();
        }
    }

    let dot = cursor.peek() == Some(b'.');
    if dot {
        cursor.advance();
        let mut count = 0;
        loop {
            let next = cursor.peek();
            let digit = next.is_some_and(|byte| byte.is_ascii_digit());
            if !digit {
                break;
            }
            cursor.advance();
            count += 1;
        }
        let missing = count == 0;
        if missing {
            let end = cursor.peek().is_none();
            if end {
                return Err(cursor.fault(TextFault::UnexpectedEnd));
            }
            return Err(cursor.fault(TextFault::UnexpectedCharacter));
        }
    }

    let marker = cursor.peek();
    let exponent = marker == Some(b'e') || marker == Some(b'E');
    if exponent {
        cursor.advance();
        let sign = cursor.peek();
        let signed = sign == Some(b'+') || sign == Some(b'-');
        if signed {
            cursor.advance();
        }
        let mut count = 0;
        loop {
            let next = cursor.peek();
            let digit = next.is_some_and(|byte| byte.is_ascii_digit());
            if !digit {
                break;
            }
            cursor.advance();
            count += 1;
        }
        let missing = count == 0;
        if missing {
            let end = cursor.peek().is_none();
            if end {
                return Err(cursor.fault(TextFault::UnexpectedEnd));
            }
            return Err(cursor.fault(TextFault::UnexpectedCharacter));
        }
    }

    Ok(cursor.slice(start))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_return_whole_text() {
        // Catches a number read that stops early or runs past its end.
        let texts = ["0", "-0", "10", "1.5", "1e5", "1E+5", "1.5e-3"];
        for text in texts {
            let mut cursor = Cursor::new(text);
            assert_eq!(number(&mut cursor), Ok(text));
            assert_eq!(cursor.offset(), text.len());
        }
    }

    #[test]
    fn bad_numbers_are_refused() {
        // Catches a number grammar that accepts a missing integer, fraction, or exponent.
        let texts = ["-", "1.", ".5", "+1", "1e", "1e+"];
        for text in texts {
            let mut cursor = Cursor::new(text);
            assert!(number(&mut cursor).is_err());
        }
    }

    #[test]
    fn leading_zero_stops_before_second_digit() {
        // Catches a number read that consumes `01` as one number.
        let mut cursor = Cursor::new("01");
        assert_eq!(number(&mut cursor), Ok("0"));
        assert_eq!(cursor.offset(), 1);
    }
}
