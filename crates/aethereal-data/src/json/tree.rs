//! JSON text read into a flat tree.

use super::TextFault;
use alloc::string::String;
use alloc::vec::Vec;

/// A run of entries in one of the tree's two lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    start: usize,
    count: usize,
}

/// One JSON value, never owning another value.
#[derive(Debug, PartialEq, Eq)]
pub enum Value<'json> {
    /// JSON `null`.
    Null,
    /// A JSON boolean.
    Boolean(bool),
    /// The number's text, as it stood in the input. It becomes an integer or a float only in the walk.
    Number(&'json str),
    /// The string, escapes resolved.
    String(String),
    /// Its elements, in `Tree::elements`.
    Array(Span),
    /// Its members, in `Tree::members`, sorted by name.
    Object(Span),
}

/// One object member: a name and its value.
#[derive(Debug, PartialEq, Eq)]
pub struct Member<'json> {
    /// The member name, escapes resolved.
    pub name: String,
    /// The member value.
    pub value: Value<'json>,
}

/// The lists a tree is built in: children are stored before the value that holds them.
pub(super) struct Arena<'json> {
    values: Vec<Value<'json>>,
    members: Vec<Member<'json>>,
}

impl<'json> Arena<'json> {
    /// Empty lists to build a tree in.
    pub(super) fn new() -> Self {
        Self { values: Vec::new(), members: Vec::new() }
    }

    /// Stores `elements` and returns the array holding them.
    pub(super) fn array(&mut self, elements: Vec<Value<'json>>) -> Value<'json> {
        let start = self.values.len();
        let count = elements.len();
        self.values.extend(elements);
        Value::Array(Span { start, count })
    }

    /// Sorts `members` by name, stores them, and returns the object; `RepeatedName` when two names are equal.
    pub(super) fn object(&mut self, mut members: Vec<Member<'json>>) -> Result<Value<'json>, TextFault> {
        members.sort_by(|left, right| left.name.cmp(&right.name));
        for window in members.windows(2) {
            let first = window.first();
            let second = window.get(1);
            let same = match (first, second) {
                (Some(first), Some(second)) => first.name == second.name,
                _ => false,
            };
            if same {
                return Err(TextFault::RepeatedName);
            }
        }

        let start = self.members.len();
        let count = members.len();
        self.members.extend(members);
        Ok(Value::Object(Span { start, count }))
    }

    /// The tree holding `root` and everything stored so far.
    pub(super) fn finish(self, root: Value<'json>) -> Tree<'json> {
        Self::tree(self.values, self.members, root)
    }

    /// Builds the tree from its two lists and its root.
    fn tree(values: Vec<Value<'json>>, members: Vec<Member<'json>>, root: Value<'json>) -> Tree<'json> {
        Tree { values, members, root }
    }
}

/// JSON text read into two flat lists and a root.
pub struct Tree<'json> {
    values: Vec<Value<'json>>,
    members: Vec<Member<'json>>,
    root: Value<'json>,
}

impl<'json> Tree<'json> {
    /// The root value.
    #[must_use]
    pub fn root(&self) -> &Value<'json> {
        &self.root
    }

    /// The elements an array holds.
    #[must_use]
    pub fn elements(&self, span: Span) -> &[Value<'json>] {
        let end = span.start.saturating_add(span.count);
        self.values.get(span.start..end).unwrap_or_default()
    }

    /// The members an object holds, sorted by name.
    #[must_use]
    pub fn members(&self, span: Span) -> &[Member<'json>] {
        let end = span.start.saturating_add(span.count);
        self.members.get(span.start..end).unwrap_or_default()
    }

    /// The member named `name`, by binary search.
    #[must_use]
    pub fn member(&self, span: Span, name: &str) -> Option<&Value<'json>> {
        let members = self.members(span);
        let found = members.binary_search_by(|member| member.name.as_str().cmp(name));
        let Ok(position) = found else {
            return None;
        };
        let member = members.get(position)?;
        Some(&member.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn members_are_sorted_and_found() {
        // Catches a table that keeps members in input order or searches linearly past a miss.
        let mut arena = Arena::new();
        let members = vec![
            Member { name: String::from("b"), value: Value::Null },
            Member { name: String::from("a"), value: Value::Boolean(true) },
            Member { name: String::from("c"), value: Value::Boolean(false) },
        ];
        let object = arena.object(members);
        let is_ok = object.is_ok();
        assert!(is_ok);
        if let Ok(Value::Object(span)) = object {
            let tree = arena.finish(Value::Object(span));
            let listed: Vec<&str> = tree.members(span).iter().map(|member| member.name.as_str()).collect();
            assert_eq!(listed, vec!["a", "b", "c"]);
            assert_eq!(tree.member(span, "a"), Some(&Value::Boolean(true)));
            assert_eq!(tree.member(span, "d"), None);
        } else {
            panic!("object keeps its members");
        }
    }

    #[test]
    fn repeated_name_is_refused() {
        // Catches a table that keeps the later of two equal names.
        let mut arena = Arena::new();
        let members = vec![
            Member { name: String::from("a"), value: Value::Null },
            Member { name: String::from("a"), value: Value::Null },
        ];
        assert_eq!(arena.object(members).map(|_| ()), Err(TextFault::RepeatedName));
    }

    #[test]
    fn array_inside_array_reads_back() {
        // Catches a span that points past the inner array's elements.
        let mut arena = Arena::new();
        let inner = arena.array(vec![Value::Boolean(false), Value::Boolean(true)]);
        let outer = arena.array(vec![Value::Boolean(true), inner]);
        let tree = arena.finish(outer);
        let Value::Array(outer_span) = tree.root() else {
            panic!("root is an array");
        };
        let outer_span = *outer_span;
        let outer_elements = tree.elements(outer_span);
        assert_eq!(outer_elements.len(), 2);
        let Some(second) = outer_elements.get(1) else {
            panic!("outer has two elements");
        };
        let Value::Array(inner_span) = second else {
            panic!("second element is an array");
        };
        let inner_span = *inner_span;
        assert_eq!(tree.elements(inner_span), &[Value::Boolean(false), Value::Boolean(true)]);
    }
}
