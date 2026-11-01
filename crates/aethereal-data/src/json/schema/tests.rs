//! Tests for the JSON Schema walk: each schema is held to what `from_json` reads.

use super::json_schema;
use crate::json::reader;
use crate::json::rows::rows;
use crate::json::tree::{Span, Tree, Value};
use crate::{Budget, DecodeError, JsonFault, Plain, Schema, SchemaNode, SchemaNodes, from_json};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

#[derive(Plain)]
struct Player {
    id: u64,
    name: Option<String>,
}

#[derive(Plain)]
enum Shape {
    Dot,
    Circle(f32),
    Box { w: f32, h: f32 },
}

/// JSON no row holds, each close to a form some shape is written in.
const NEAR_MISSES: &[&str] = &[
    "null",
    "true",
    "0",
    "300",
    "-1",
    "1.5",
    "\"\"",
    "\"abc\"",
    "\"00ff\"",
    "\"ABCD\"",
    "\"007\"",
    "\"-0\"",
    "\"Dot\"",
    "\"NaN\"",
    "[]",
    "[null]",
    "[[null]]",
    "[1,2]",
    "[1,2,3]",
    "{}",
    "{\"Dot\":1}",
    "{\"Circle\":1.5,\"Dot\":1}",
    "{\"Box\":{\"w\":1}}",
    "{\"id\":\"1\"}",
    "{\"id\":\"1\",\"name\":null}",
    "{\"id\":\"1\",\"other\":1}",
    "{\"name\":\"a\"}",
];

/// `schema`'s JSON Schema.
fn schema_text(schema: &SchemaNodes) -> String {
    let mut budget = Budget::new(1 << 20);
    let Ok(text) = json_schema(schema, &mut budget) else {
        panic!("the schema is written");
    };

    text
}

/// `text` read as JSON.
fn tree(text: &str) -> Tree<'_> {
    let mut budget = Budget::new(1 << 20);
    let Ok(read) = reader::read(text, &mut budget) else {
        panic!("{text} is JSON");
    };

    read
}

/// A number's value as a float, exact for every bound written here but the 32-bit ends, which need no more.
fn number(text: &str) -> f64 {
    let Ok(value) = text.parse() else {
        panic!("{text} is a number");
    };

    value
}

/// Whether a pattern this walk writes matches `text`.
fn matches_pattern(pattern: &str, text: &str) -> bool {
    let is_hex = text.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    let digits = text.strip_prefix('-').unwrap_or(text);
    let is_negative = digits.len() != text.len();
    let is_digits = !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit());
    let is_plain = digits == "0" || !digits.starts_with('0');
    match pattern {
        "^[0-9a-f]*$" => is_hex,
        "^([0-9a-f]{2})*$" => is_hex && text.len().is_multiple_of(2),
        "^(0|[1-9][0-9]*)$" => !is_negative && is_digits && is_plain,
        "^(0|-?[1-9][0-9]*)$" => is_digits && is_plain && text != "-0",
        other => panic!("the walk wrote a pattern no test reads: {other}"),
    }
}

/// A validator of the keywords the walk writes, which panics on any other, so none goes unread.
struct Check<'text> {
    schema: &'text Tree<'text>,
    value: &'text Tree<'text>,
}

impl Check<'_> {
    /// Whether `value` meets `schema`.
    fn fits(&self, schema: &Value<'_>, value: &Value<'_>) -> bool {
        let Value::Object(span) = schema else {
            panic!("a schema is an object");
        };
        self.schema.members(*span).iter().all(|member| self.keyword(&member.name, &member.value, schema, value))
    }

    /// Whether `value` meets one keyword of `schema`.
    fn keyword(&self, name: &str, keyword: &Value<'_>, schema: &Value<'_>, value: &Value<'_>) -> bool {
        match (name, keyword) {
            ("type", Value::String(wanted)) => is_type(wanted, value),
            ("const", Value::String(wanted)) => matches!(value, Value::String(text) if text == wanted),
            ("enum", Value::Array(span)) => self.schema.elements(*span).iter().any(|wanted| match (wanted, value) {
                (Value::String(wanted), Value::String(text)) => wanted == text,
                _ => false,
            }),
            ("minimum", Value::Number(bound)) => !matches!(value, Value::Number(text) if number(text) < number(bound)),
            ("maximum", Value::Number(bound)) => !matches!(value, Value::Number(text) if number(text) > number(bound)),
            ("pattern", Value::String(pattern)) => {
                !matches!(value, Value::String(text) if !matches_pattern(pattern, text))
            }
            ("minLength", Value::Number(bound)) => length(value).is_none_or(|count| count >= number(bound)),
            ("maxLength", Value::Number(bound)) => length(value).is_none_or(|count| count <= number(bound)),
            ("minItems", Value::Number(bound)) => self.count(value).is_none_or(|count| count >= number(bound)),
            ("maxItems", Value::Number(bound)) => self.count(value).is_none_or(|count| count <= number(bound)),
            ("items", child) => self.items(child, value),
            ("prefixItems", Value::Array(span)) => self.prefix(self.schema.elements(*span), value),
            ("properties", Value::Object(span)) => self.properties(*span, value),
            ("required", Value::Array(span)) => self.required(self.schema.elements(*span), value),
            ("additionalProperties", child) => self.additional(child, schema, value),
            ("propertyNames", child) => self.property_names(child, value),
            ("anyOf", Value::Array(span)) => self.schema.elements(*span).iter().any(|child| self.fits(child, value)),
            ("oneOf", Value::Array(span)) => {
                self.schema.elements(*span).iter().filter(|child| self.fits(child, value)).count() == 1
            }
            ("not", child) => !self.fits(child, value),
            (other, _) => panic!("the walk wrote a keyword no test reads: {other}"),
        }
    }

    /// An array's length, as a bound is compared.
    fn count(&self, value: &Value<'_>) -> Option<f64> {
        let Value::Array(span) = value else {
            return None;
        };

        Some(f64::from(u32::try_from(self.value.elements(*span).len()).unwrap_or(u32::MAX)))
    }

    /// Whether each element of an array meets `child`.
    fn items(&self, child: &Value<'_>, value: &Value<'_>) -> bool {
        let Value::Array(span) = value else {
            return true;
        };

        self.value.elements(*span).iter().all(|element| self.fits(child, element))
    }

    /// Whether each element of an array meets the schema at its place.
    fn prefix(&self, schemas: &[Value<'_>], value: &Value<'_>) -> bool {
        let Value::Array(span) = value else {
            return true;
        };

        schemas.iter().zip(self.value.elements(*span)).all(|(child, element)| self.fits(child, element))
    }

    /// Whether each member of an object that `properties` names meets its schema.
    fn properties(&self, properties: Span, value: &Value<'_>) -> bool {
        let Value::Object(span) = value else {
            return true;
        };

        self.schema.members(properties).iter().all(|property| {
            let member = self.value.member(*span, &property.name);
            member.is_none_or(|member| self.fits(&property.value, member))
        })
    }

    /// Whether an object holds each name listed.
    fn required(&self, names: &[Value<'_>], value: &Value<'_>) -> bool {
        let Value::Object(span) = value else {
            return true;
        };

        names.iter().all(|name| {
            let Value::String(name) = name else {
                panic!("a required name is a string");
            };
            self.value.member(*span, name).is_some()
        })
    }

    /// Whether each member `schema`'s `properties` does not name meets `child`, which `false` refuses.
    fn additional(&self, child: &Value<'_>, schema: &Value<'_>, value: &Value<'_>) -> bool {
        let Value::Object(span) = value else {
            return true;
        };
        let Value::Object(own) = schema else {
            panic!("a schema is an object");
        };
        let named = self.schema.member(*own, "properties");

        self.value.members(*span).iter().all(|member| {
            let is_named = matches!(named, Some(Value::Object(properties))
                if self.schema.member(*properties, &member.name).is_some());
            match child {
                _ if is_named => true,
                Value::Boolean(allowed) => *allowed,
                child => self.fits(child, &member.value),
            }
        })
    }

    /// Whether each member's name meets `child`.
    fn property_names(&self, child: &Value<'_>, value: &Value<'_>) -> bool {
        let Value::Object(span) = value else {
            return true;
        };

        self.value.members(*span).iter().all(|member| self.fits(child, &Value::String(member.name.clone())))
    }
}

/// A string's length in characters, as a bound is compared.
fn length(value: &Value<'_>) -> Option<f64> {
    let Value::String(text) = value else {
        return None;
    };

    Some(f64::from(u32::try_from(text.chars().count()).unwrap_or(u32::MAX)))
}

/// Whether `value` is of the JSON Schema type `wanted`.
fn is_type(wanted: &str, value: &Value<'_>) -> bool {
    match (wanted, value) {
        ("null", Value::Null)
        | ("boolean", Value::Boolean(_))
        | ("number", Value::Number(_))
        | ("string", Value::String(_))
        | ("array", Value::Array(_))
        | ("object", Value::Object(_)) => true,
        ("integer", Value::Number(text)) => number(text).fract() == 0.0,
        _ => false,
    }
}

/// Whether `json` meets `schema`'s JSON Schema.
fn meets(schema: &SchemaNodes, json: &str) -> bool {
    let text = schema_text(schema);
    let written = tree(&text);
    let value = tree(json);
    let check = Check { schema: &written, value: &value };

    check.fits(written.root(), value.root())
}

/// Every schema a row holds, one of each.
fn schemas() -> Vec<SchemaNodes> {
    let mut all: Vec<SchemaNodes> = Vec::new();
    for row in rows() {
        if !all.contains(&row.schema) {
            all.push(row.schema);
        }
    }
    all.push(SchemaNodes::of(&<Player as Schema>::SHAPE));
    all.push(SchemaNodes::of(&<Shape as Schema>::SHAPE));

    all
}

/// Every JSON text the tests try.
fn samples() -> Vec<String> {
    let mut all: Vec<String> = rows().into_iter().map(|row| row.json).collect();
    all.extend(NEAR_MISSES.iter().map(|text| String::from(*text)));

    all
}

/// Whether a refusal is of a range JSON Schema cannot say: digits in a string, which only a pattern reads.
fn is_range_unsaid(fault: JsonFault, json: &str) -> bool {
    fault == JsonFault::OutOfRange && json.contains('"')
}

#[test]
fn a_schema_takes_what_from_json_reads_and_refuses_the_rest() {
    // Catches a form written differently from how `from_json` reads it, in either direction: every schema against
    // every text. Only a range JSON Schema cannot say may pass here and be refused there.
    for schema in schemas() {
        for json in samples() {
            let mut budget = Budget::new(1 << 20);
            let read = from_json(&schema, &json, &mut budget);
            let met = meets(&schema, &json);
            let agrees = match &read {
                Ok(_) => met,
                Err(error) => !met || is_range_unsaid(error.fault(), &json),
            };
            assert!(agrees, "{schema:?} on {json}: read {read:?}, met {met}");
        }
    }
}

#[test]
fn an_option_field_is_not_required_and_the_rest_are() {
    // Catches `required` listing an option, which `from_json` reads as none when left out, or leaving out a field.
    let text = schema_text(&SchemaNodes::of(&<Player as Schema>::SHAPE));

    assert!(text.contains("\"required\":[\"id\"]"), "{text}");
}

#[test]
fn an_empty_tuple_and_enum_write_no_empty_keyword() {
    // Catches `prefixItems` or `oneOf` written empty, which draft 2020-12 refuses.
    let tuple = SchemaNodes::of(&<() as Schema>::SHAPE);
    assert_eq!(schema_text(&tuple), "{\"type\":\"array\",\"maxItems\":0}");

    let bytes = crate::to_bytes(&vec![SchemaNode::Enum { name: String::from("Never"), variants: Vec::new() }]);
    let Ok(bytes) = bytes else {
        panic!("the nodes encode");
    };
    let mut budget = Budget::new(4096);
    let Ok(never) = crate::from_bytes::<SchemaNodes>(&bytes, &mut budget) else {
        panic!("the nodes decode");
    };
    assert_eq!(schema_text(&never), "{\"not\":{}}");
}

#[test]
fn a_schema_that_shares_nodes_is_bounded_by_the_budget() {
    // Catches a walk that writes shared nodes unpaid: each level names the one below twice, so the text doubles.
    let mut nodes = vec![SchemaNode::Boolean];
    for child in 0..40 {
        nodes.push(SchemaNode::Tuple { elements: vec![child, child] });
    }
    let Ok(bytes) = crate::to_bytes(&nodes) else {
        panic!("the nodes encode");
    };
    let mut budget = Budget::new(1 << 16);
    let Ok(shared) = crate::from_bytes::<SchemaNodes>(&bytes, &mut budget) else {
        panic!("the nodes decode");
    };

    let mut budget = Budget::new(1 << 16);
    let refused = json_schema(&shared, &mut budget).map(|_| ());

    let Err(error) = refused else {
        panic!("the text is past the budget");
    };
    assert!(matches!(error.fault(), JsonFault::Decode(DecodeError::BudgetExceeded { .. })), "{error:?}");
}

#[test]
fn a_schema_past_the_depth_limit_is_refused() {
    // Catches a walk that follows a schema received as bytes without a cap.
    let mut nodes = vec![SchemaNode::Boolean];
    for child in 0..200 {
        nodes.push(SchemaNode::Sequence(child));
    }
    let Ok(bytes) = crate::to_bytes(&nodes) else {
        panic!("the nodes encode");
    };
    let mut budget = Budget::new(1 << 16);
    let Ok(deep) = crate::from_bytes::<SchemaNodes>(&bytes, &mut budget) else {
        panic!("the nodes decode");
    };

    let mut budget = Budget::new(1 << 20);

    assert_eq!(json_schema(&deep, &mut budget).map(|_| ()), Err(JsonFault::SchemaTooDeep.into()));
}
