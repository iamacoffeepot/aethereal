//! What a control the page marks holds, as its element reads.

use super::Value;

/// What an input element of type `kind` holds, its value `value` and whether it is `checked`: a checkbox or radio
/// button by whether it is checked, a range or number input by its number when its value reads as a finite one, and
/// any other by its value as written.
pub fn value_of(kind: &str, value: String, checked: bool) -> Value {
    match kind {
        "checkbox" | "radio" => Value::Checked(checked),
        "range" | "number" => {
            let number = value.parse::<f64>().ok().filter(|number| number.is_finite());
            number.map_or(Value::Text(value), Value::Number)
        }
        _ => Value::Text(value),
    }
}
