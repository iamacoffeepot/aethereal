//! JSON text read into a tree, without recursion.

mod cursor;
mod number;
mod string;

use super::tree::{Arena, Member, Tree, Value};
use super::{JsonError, TextFault};
use crate::Budget;
use alloc::string::String;
use alloc::vec::Vec;
use core::mem::{replace, size_of};
use cursor::Cursor;

/// Reads one JSON value and nothing after it.
pub(in crate::json) fn read<'json>(json: &'json str, budget: &mut Budget) -> Result<Tree<'json>, JsonError> {
    let mut cursor = Cursor::new(json);
    let mut arena = Arena::new();
    let mut frames: Vec<Frame<'_>> = Vec::new();
    loop {
        let value = start(&mut cursor, &mut arena, &mut frames, budget)?;
        let placed = place(value, &mut cursor, &mut arena, &mut frames, budget)?;
        match placed {
            Placed::More => {}
            Placed::Done(tree) => {
                return Ok(tree);
            }
        }
    }
}

/// An open array or object waiting for its values.
enum Frame<'json> {
    /// An open array and the elements read so far.
    Array(Vec<Value<'json>>),
    /// An open object, the members read so far, and the name whose value is being read.
    Object { members: Vec<Member<'json>>, name: String },
}

/// What placing a value asks for next.
enum Placed<'json> {
    /// Go on to the next value.
    More,
    /// Finished with the tree's root.
    Done(Tree<'json>),
}

/// Reads the next value, pushing a frame for an open array or object.
fn start<'json>(
    cursor: &mut Cursor<'json>,
    arena: &mut Arena<'json>,
    frames: &mut Vec<Frame<'json>>,
    budget: &mut Budget,
) -> Result<Value<'json>, JsonError> {
    loop {
        cursor.skip_whitespace();
        budget.charge(size_of::<Member<'_>>())?;
        let next = cursor.peek();
        let Some(byte) = next else {
            return Err(cursor.fault(TextFault::UnexpectedEnd));
        };
        let open_array = byte == b'[';
        if open_array {
            cursor.advance();
            cursor.skip_whitespace();
            let empty = cursor.peek() == Some(b']');
            if empty {
                cursor.advance();
                return Ok(arena.array(Vec::new()));
            }
            frames.push(Frame::Array(Vec::new()));
            continue;
        }
        let open_object = byte == b'{';
        if open_object {
            cursor.advance();
            cursor.skip_whitespace();
            let empty = cursor.peek() == Some(b'}');
            if empty {
                cursor.advance();
                let empty_members: Vec<Member<'_>> = Vec::new();
                let object = arena.object(empty_members).map_err(|fault| cursor.fault(fault))?;
                return Ok(object);
            }
            let name = string::string(cursor, budget)?;
            cursor.skip_whitespace();
            let colon = cursor.peek();
            let is_colon = colon == Some(b':');
            if !is_colon {
                let end = colon.is_none();
                if end {
                    return Err(cursor.fault(TextFault::UnexpectedEnd));
                }
                return Err(cursor.fault(TextFault::UnexpectedCharacter));
            }
            cursor.advance();
            frames.push(Frame::Object { members: Vec::new(), name });
            continue;
        }
        let quote = byte == b'"';
        if quote {
            let text = string::string(cursor, budget)?;
            return Ok(Value::String(text));
        }
        let dash = byte == b'-';
        let low = byte >= b'0';
        let high = byte <= b'9';
        let digit = low && high;
        let number_start = dash || digit;
        if number_start {
            let text = number::number(cursor)?;
            return Ok(Value::Number(text));
        }
        let is_true = byte == b't';
        if is_true {
            literal(cursor, "true")?;
            return Ok(Value::Boolean(true));
        }
        let is_false = byte == b'f';
        if is_false {
            literal(cursor, "false")?;
            return Ok(Value::Boolean(false));
        }
        let is_null = byte == b'n';
        if is_null {
            literal(cursor, "null")?;
            return Ok(Value::Null);
        }
        return Err(cursor.fault(TextFault::UnexpectedCharacter));
    }
}

/// Reads exactly `word` after its first letter.
fn literal(cursor: &mut Cursor<'_>, word: &str) -> Result<(), JsonError> {
    let bytes = word.as_bytes();
    let Some(first) = bytes.first() else {
        return Err(cursor.fault(TextFault::UnexpectedCharacter));
    };
    let at = cursor.peek();
    let matches_first = at == Some(*first);
    if !matches_first {
        let end = at.is_none();
        if end {
            return Err(cursor.fault(TextFault::UnexpectedEnd));
        }
        return Err(cursor.fault(TextFault::UnexpectedCharacter));
    }
    cursor.advance();
    let Some(rest) = bytes.get(1..) else {
        return Err(cursor.fault(TextFault::UnexpectedCharacter));
    };
    for expected in rest {
        let next = cursor.peek();
        let Some(byte) = next else {
            return Err(cursor.fault(TextFault::UnexpectedEnd));
        };
        let matches = byte == *expected;
        if !matches {
            return Err(cursor.fault(TextFault::UnexpectedCharacter));
        }
        cursor.advance();
    }

    Ok(())
}

/// Places `value` into its frame, asking for the next value or finishing.
fn place<'json>(
    mut value: Value<'json>,
    cursor: &mut Cursor<'json>,
    arena: &mut Arena<'json>,
    frames: &mut Vec<Frame<'json>>,
    budget: &mut Budget,
) -> Result<Placed<'json>, JsonError> {
    loop {
        let Some(frame) = frames.pop() else {
            cursor.skip_whitespace();
            let end = cursor.peek().is_none();
            if !end {
                return Err(cursor.fault(TextFault::TrailingText));
            }
            let empty = Arena::new();
            let owned = replace(arena, empty);
            return Ok(Placed::Done(owned.finish(value)));
        };
        match frame {
            Frame::Array(mut elements) => {
                elements.push(value);
                cursor.skip_whitespace();
                let next = cursor.peek();
                let Some(byte) = next else {
                    return Err(cursor.fault(TextFault::UnexpectedEnd));
                };
                let comma = byte == b',';
                if comma {
                    cursor.advance();
                    frames.push(Frame::Array(elements));
                    return Ok(Placed::More);
                }
                let close = byte == b']';
                if close {
                    cursor.advance();
                    value = arena.array(elements);
                    continue;
                }
                return Err(cursor.fault(TextFault::UnexpectedCharacter));
            }
            Frame::Object { mut members, name } => {
                members.push(Member { name, value });
                cursor.skip_whitespace();
                let next = cursor.peek();
                let Some(byte) = next else {
                    return Err(cursor.fault(TextFault::UnexpectedEnd));
                };
                let comma = byte == b',';
                if comma {
                    cursor.advance();
                    cursor.skip_whitespace();
                    let quote = cursor.peek() == Some(b'"');
                    if !quote {
                        let end = cursor.peek().is_none();
                        if end {
                            return Err(cursor.fault(TextFault::UnexpectedEnd));
                        }
                        return Err(cursor.fault(TextFault::UnexpectedCharacter));
                    }
                    let next_name = string::string(cursor, budget)?;
                    cursor.skip_whitespace();
                    let colon = cursor.peek();
                    let is_colon = colon == Some(b':');
                    if !is_colon {
                        let end = colon.is_none();
                        if end {
                            return Err(cursor.fault(TextFault::UnexpectedEnd));
                        }
                        return Err(cursor.fault(TextFault::UnexpectedCharacter));
                    }
                    cursor.advance();
                    frames.push(Frame::Object { members, name: next_name });
                    return Ok(Placed::More);
                }
                let close = byte == b'}';
                if close {
                    let object = arena.object(members).map_err(|fault| cursor.fault(fault))?;
                    cursor.advance();
                    value = object;
                    continue;
                }
                return Err(cursor.fault(TextFault::UnexpectedCharacter));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DecodeError, JsonFault};

    #[test]
    fn top_level_values_with_whitespace() {
        // Catches a reader that forgets leading or trailing whitespace.
        let cases: [(&str, Value<'_>); 5] = [
            ("  true  ", Value::Boolean(true)),
            ("\tfalse\n", Value::Boolean(false)),
            (" null ", Value::Null),
            (" 123 ", Value::Number("123")),
            (" \"a\" ", Value::String(String::from("a"))),
        ];
        for (text, expected) in cases {
            let mut budget = Budget::new(1024);
            let tree = read(text, &mut budget);
            let is_ok = tree.is_ok();
            assert!(is_ok);
            if let Ok(tree) = tree {
                assert_eq!(tree.root(), &expected);
            }
        }
    }

    #[test]
    fn nested_array_and_object_read_back() {
        // Catches a frame that drops an element or a member.
        let mut budget = Budget::new(4096);
        let tree = read("[1,[2,3],{\"a\":null}]", &mut budget);
        let is_ok = tree.is_ok();
        assert!(is_ok);
        if let Ok(tree) = tree {
            let Value::Array(outer) = tree.root() else {
                panic!("root is an array");
            };
            let outer_span = *outer;
            let elements = tree.elements(outer_span);
            assert_eq!(elements.len(), 3);
            assert_eq!(elements.first(), Some(&Value::Number("1")));
            let Some(Value::Array(inner_span)) = elements.get(1) else {
                panic!("second element is an array");
            };
            assert_eq!(tree.elements(*inner_span), &[Value::Number("2"), Value::Number("3")]);
            let Some(Value::Object(object_span)) = elements.get(2) else {
                panic!("third element is an object");
            };
            assert_eq!(tree.member(*object_span, "a"), Some(&Value::Null));
        }
    }

    #[test]
    fn repeated_names_are_refused() {
        // Catches a table that keeps the later of two equal names.
        let mut budget = Budget::new(1024);
        let first = read("{\"a\":1,\"a\":2}", &mut budget);
        assert!(matches!(
            first.map_err(|error| error.fault()),
            Err(JsonFault::Text { fault: TextFault::RepeatedName, .. })
        ));
        let mut budget = Budget::new(1024);
        let second = read("{\"a\":1,\"\\u0061\":2}", &mut budget);
        assert!(matches!(
            second.map_err(|error| error.fault()),
            Err(JsonFault::Text { fault: TextFault::RepeatedName, .. })
        ));
        let mut budget = Budget::new(4096);
        let nested = read("{\"a\":{\"b\":1,\"b\":2}}", &mut budget);
        assert!(matches!(
            nested.map_err(|error| error.fault()),
            Err(JsonFault::Text { fault: TextFault::RepeatedName, .. })
        ));
    }

    #[test]
    fn trailing_text_is_refused() {
        // Catches a reader that stops after the value and ignores the rest.
        let texts = ["1 2", "{} x", "[]]", "nulll"];
        for text in texts {
            let mut budget = Budget::new(1024);
            let result = read(text, &mut budget);
            assert!(matches!(
                result.map_err(|error| error.fault()),
                Err(JsonFault::Text { fault: TextFault::TrailingText, .. })
            ));
        }
        let mut budget = Budget::new(1024);
        let extra = read("01", &mut budget);
        let offset = extra.map_err(|error| error.fault());
        assert!(matches!(offset, Err(JsonFault::Text { offset: 1, fault: TextFault::TrailingText })));
    }

    #[test]
    fn bad_text_is_refused() {
        // Catches a value start that accepts what RFC 8259 never lists.
        let missing_colon = concat!("{\"a\"", " 1}");
        let bare_name = concat!("{a", ":1}");
        let texts =
            ["[1,]", "{\"a\":1,}", missing_colon, bare_name, "[1 2]", "tru", "NaN", "Infinity", "'a'", "\u{FEFF}1"];
        for text in texts {
            let mut budget = Budget::new(1024);
            assert!(read(text, &mut budget).is_err());
        }
    }

    #[test]
    fn short_text_is_unexpected_end() {
        // Catches a reader that reports a character past the end.
        let texts = ["", "   ", "[1", "{\"a\":"];
        for text in texts {
            let mut budget = Budget::new(1024);
            let result = read(text, &mut budget);
            assert!(matches!(
                result.map_err(|error| error.fault()),
                Err(JsonFault::Text { fault: TextFault::UnexpectedEnd, .. })
            ));
        }
    }

    #[test]
    fn unpaid_frames_exceed_budget() {
        // Catches open arrays held without charging the budget.
        let text = "[".repeat(100_000);
        let mut budget = Budget::new(1024);
        let result = read(&text, &mut budget);
        assert!(matches!(
            result.map_err(|error| error.fault()),
            Err(JsonFault::Decode(DecodeError::BudgetExceeded { .. }))
        ));
    }

    #[test]
    fn deep_nesting_reads_without_recursion() {
        // Catches a reader that recurses on its input and a tree that drops by recursion.
        let mut text = "[".repeat(100_000);
        text.push_str(&"]".repeat(100_000));
        let mut budget = Budget::new(usize::MAX);
        let result = read(&text, &mut budget);
        assert!(result.is_ok());
        drop(result);
    }

    #[test]
    fn reading_charges_members() {
        // Catches a reader that holds values it never charged.
        let mut budget = Budget::new(usize::MAX);
        let before = budget.remaining();
        let result = read("[1,2,3]", &mut budget);
        assert!(result.is_ok());
        let after = budget.remaining();
        let charged = before.saturating_sub(after);
        let each = size_of::<Member<'_>>();
        assert!(charged >= 4 * each);
    }
}
