//! The fields an agent gives laid over a tuning as it stands, field by field, as a layer is laid over the defaults
//! (ADR-0028, ADR-0050).

use aethereal_data::{Budget, JsonTree, JsonValue, read_json, write_json_string};

/// Why the fields given could not be laid over the tuning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unlaid {
    /// What was given, or the tuning, is not JSON, or is past the budget: the reader's words.
    Unread(String),
    /// What was given is not an object of fields.
    NotFields,
}

/// The JSON object `base`, each of its fields that `fields` names replaced whole by the value given there, and each
/// field `fields` names that `base` has not added, for the decode to refuse by its path.
///
/// # Errors
///
/// When either is not JSON, or `fields` is not an object.
pub fn overlay(base: &str, fields: &str, budget: &mut Budget) -> Result<String, Unlaid> {
    let base = read_json(base, budget).map_err(|why| Unlaid::Unread(why.to_string()))?;
    let given = read_json(fields, budget).map_err(|why| Unlaid::Unread(why.to_string()))?;
    let (&JsonValue::Object(had), &JsonValue::Object(set)) = (base.root(), given.root()) else {
        return Err(Unlaid::NotFields);
    };

    let mut out = String::from("{");
    let mut first = true;
    for member in base.members(had) {
        let (tree, value) = given.member(set, &member.name).map_or((&base, &member.value), |value| (&given, value));
        separate(&mut out, &mut first);
        write_json_string(&member.name, &mut out);
        out.push(':');
        write(tree, value, &mut out);
    }
    for member in given.members(set) {
        let is_new = base.member(had, &member.name).is_none();
        if is_new {
            separate(&mut out, &mut first);
            write_json_string(&member.name, &mut out);
            out.push(':');
            write(&given, &member.value, &mut out);
        }
    }
    out.push('}');

    Ok(out)
}

/// The names of the fields of `fields`, a JSON object.
///
/// # Errors
///
/// When it is not JSON, or not an object.
pub fn named(fields: &str, budget: &mut Budget) -> Result<Vec<String>, Unlaid> {
    let given = read_json(fields, budget).map_err(|why| Unlaid::Unread(why.to_string()))?;
    let &JsonValue::Object(set) = given.root() else {
        return Err(Unlaid::NotFields);
    };

    Ok(given.members(set).iter().map(|member| member.name.clone()).collect())
}

/// The JSON object `base` with only the fields `names` names, in `base`'s order: what a `tune` set, as it then
/// stands. A name `base` has not is left out.
///
/// # Errors
///
/// When `base` is not JSON, or not an object.
pub fn picked(base: &str, names: &[String], budget: &mut Budget) -> Result<String, Unlaid> {
    let base = read_json(base, budget).map_err(|why| Unlaid::Unread(why.to_string()))?;
    let &JsonValue::Object(had) = base.root() else {
        return Err(Unlaid::NotFields);
    };

    let mut out = String::from("{");
    let mut first = true;
    for member in base.members(had) {
        let was_set = names.contains(&member.name);
        if was_set {
            separate(&mut out, &mut first);
            write_json_string(&member.name, &mut out);
            out.push(':');
            write(&base, &member.value, &mut out);
        }
    }
    out.push('}');

    Ok(out)
}

/// Writes a comma before each member but the first.
fn separate(out: &mut String, first: &mut bool) {
    if !*first {
        out.push(',');
    }
    *first = false;
}

/// What is left to write of a value: a value of the tree, a member's name, or a mark between them.
enum Next<'tree, 'json> {
    Value(&'tree JsonValue<'json>),
    Name(&'tree str),
    Mark(char),
}

/// Writes `value`, a part of `tree`, as JSON text: by a list of what is left rather than by recursion, as a value given
/// by an agent may nest as deep as the reader lets it.
fn write(tree: &JsonTree<'_>, value: &JsonValue<'_>, out: &mut String) {
    let mut left = vec![Next::Value(value)];
    while let Some(next) = left.pop() {
        match next {
            Next::Mark(mark) => out.push(mark),
            Next::Name(name) => {
                write_json_string(name, out);
                out.push(':');
            }
            Next::Value(JsonValue::Null) => out.push_str("null"),
            Next::Value(JsonValue::Boolean(on)) => {
                let text = if *on {
                    "true"
                } else {
                    "false"
                };
                out.push_str(text);
            }
            Next::Value(JsonValue::Number(text)) => out.push_str(text),
            Next::Value(JsonValue::String(text)) => write_json_string(text, out),
            Next::Value(JsonValue::Array(span)) => {
                out.push('[');
                left.push(Next::Mark(']'));
                for (at, element) in tree.elements(*span).iter().enumerate().rev() {
                    left.push(Next::Value(element));
                    if at > 0 {
                        left.push(Next::Mark(','));
                    }
                }
            }
            Next::Value(JsonValue::Object(span)) => {
                out.push('{');
                left.push(Next::Mark('}'));
                for (at, member) in tree.members(*span).iter().enumerate().rev() {
                    left.push(Next::Value(&member.value));
                    left.push(Next::Name(&member.name));
                    if at > 0 {
                        left.push(Next::Mark(','));
                    }
                }
            }
        }
    }
}
