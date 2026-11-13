//! Reading Docker's JSON (ADR-0007): a body read into a tree within a budget, and the members a reply is read by.

use aethereal_data::{Budget, JsonMember, JsonTree, JsonValue, read_json};

/// The bytes of budget each byte of a body may cost as it is read: past what any JSON text costs, as every value is
/// at least a byte of its text.
const BUDGET_PER_BYTE: usize = 64;

/// `text` read as JSON, within a budget it cannot pass: none when it is not JSON.
pub(super) fn tree(text: &str) -> Option<JsonTree<'_>> {
    let mut budget = Budget::new(text.len().saturating_mul(BUDGET_PER_BYTE));

    read_json(text, &mut budget).ok()
}

/// The member `name` of `object`: none when `object` is not an object or has no such member.
pub(super) fn field<'t, 'j>(tree: &'t JsonTree<'j>, object: &JsonValue<'j>, name: &str) -> Option<&'t JsonValue<'j>> {
    let JsonValue::Object(span) = object else {
        return None;
    };

    tree.member(*span, name)
}

/// The members of `object`, in name order: none when it is not an object.
pub(super) fn members<'t, 'j>(tree: &'t JsonTree<'j>, object: &JsonValue<'j>) -> Option<&'t [JsonMember<'j>]> {
    let JsonValue::Object(span) = object else {
        return None;
    };

    Some(tree.members(*span))
}

/// The elements of `array`: none when it is not an array.
pub(super) fn elements<'t, 'j>(tree: &'t JsonTree<'j>, array: &JsonValue<'j>) -> Option<&'t [JsonValue<'j>]> {
    let JsonValue::Array(span) = array else {
        return None;
    };

    Some(tree.elements(*span))
}

/// The text `value` holds: none when it is not a string.
pub(super) fn text<'v>(value: &'v JsonValue<'_>) -> Option<&'v str> {
    let JsonValue::String(text) = value else {
        return None;
    };

    Some(text)
}

/// The integer `value` holds: none when it is not a number, or not a whole one an `i64` holds.
pub(super) fn integer(value: &JsonValue<'_>) -> Option<i64> {
    let JsonValue::Number(number) = value else {
        return None;
    };

    number.parse().ok()
}

/// The boolean `value` holds: none when it is not one.
pub(super) fn boolean(value: &JsonValue<'_>) -> Option<bool> {
    let JsonValue::Boolean(boolean) = value else {
        return None;
    };

    Some(*boolean)
}
