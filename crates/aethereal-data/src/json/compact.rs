//! A value of a read tree written back as compact JSON text.

use super::escape;
use super::tree::{Member, Tree, Value};
use alloc::string::String;
use alloc::vec::Vec;

/// What is still to be written, the next on top.
enum Pending<'tree, 'json> {
    /// A value, whole.
    Value(&'tree Value<'json>),
    /// An object member: its name, a colon, and its value.
    Member(&'tree Member<'json>),
    /// A comma or a closing bracket.
    Mark(char),
}

/// Appends `value`, a part of `tree`, as JSON text with no whitespace.
///
/// A number is its text as it stood in the input, a string is written as `escape::write` writes it, and an object's
/// members are in name order, as the tree holds them. The walk keeps its own stack, so a value nested as deep as the
/// reader reads is written without recursion.
pub(super) fn write<'tree, 'json>(tree: &'tree Tree<'json>, value: &'tree Value<'json>, out: &mut String) {
    let mut pending = Vec::from([Pending::Value(value)]);
    while let Some(next) = pending.pop() {
        match next {
            Pending::Mark(mark) => out.push(mark),
            Pending::Member(member) => {
                escape::write(&member.name, out);
                out.push(':');
                pending.push(Pending::Value(&member.value));
            }
            Pending::Value(Value::Null) => out.push_str("null"),
            Pending::Value(Value::Boolean(true)) => out.push_str("true"),
            Pending::Value(Value::Boolean(false)) => out.push_str("false"),
            Pending::Value(Value::Number(text)) => out.push_str(text),
            Pending::Value(Value::String(text)) => escape::write(text, out),
            Pending::Value(Value::Array(span)) => {
                out.push('[');
                pending.push(Pending::Mark(']'));
                for (position, element) in tree.elements(*span).iter().enumerate().rev() {
                    pending.push(Pending::Value(element));
                    if position > 0 {
                        pending.push(Pending::Mark(','));
                    }
                }
            }
            Pending::Value(Value::Object(span)) => {
                out.push('{');
                pending.push(Pending::Mark('}'));
                for (position, member) in tree.members(*span).iter().enumerate().rev() {
                    pending.push(Pending::Member(member));
                    if position > 0 {
                        pending.push(Pending::Mark(','));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Budget;
    use crate::json::reader;

    /// `text` read and written back.
    fn rewritten(text: &str) -> String {
        let mut budget = Budget::new(1 << 30);
        let Ok(tree) = reader::read(text, &mut budget) else {
            panic!("{text} is JSON");
        };
        let mut out = String::new();
        write(&tree, tree.root(), &mut out);

        out
    }

    #[test]
    fn nested_values_are_written_whole_and_compact() {
        // Catches a comma before the first element or after the last, a member without its colon, a lost empty
        // container, and whitespace kept.
        let text = " { \"b\" : [ 1 , [ ] , { } , null ] , \"a\" : { \"c\" : true , \"d\" : \"x\\ny\" } } ";
        assert_eq!(rewritten(text), "{\"a\":{\"c\":true,\"d\":\"x\\ny\"},\"b\":[1,[],{},null]}");
    }

    #[test]
    fn numbers_keep_their_text() {
        // Catches a writer that parses a number and prints it again, losing its exponent or its digits.
        assert_eq!(rewritten("[1e400,-0.10,18446744073709551617]"), "[1e400,-0.10,18446744073709551617]");
    }

    #[test]
    fn deep_nesting_is_written_without_recursion() {
        // Catches a recursive writer: a hundred thousand levels overflow its stack.
        let depth = 100_000;
        let text = ["[".repeat(depth), "]".repeat(depth)].concat();
        assert_eq!(rewritten(&text), text);
    }
}
