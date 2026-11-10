//! An answer printed as it would be typed (ADR-0053).

use aethereal_data::config::{Form, kebab};
use aethereal_data::{
    Budget, FieldNode, FieldsNode, JsonError, JsonMember, JsonTree, JsonValue, SchemaNode, SchemaNodes, read_json,
    to_json,
};
use std::iter;

/// The deepest a value is printed field by field; below it, a value is printed as JSON. A schema can arrive as bytes,
/// so the walk's depth is capped.
const MOST_DEPTH: usize = 32;

/// How a value prints: one word, which fits after a field's name, or lines of its own.
enum Printed {
    /// A value as its form writes it, or as JSON.
    Word(String),
    /// A struct's fields, one a line, or a sequence of them as blocks parted by a blank line.
    Lines(Vec<String>),
}

/// The value `bytes` holds of `schema`, printed in the forms a tool's flags read (ADR-0053): a leaf as its flag's text
/// would be, `capped:60` or `1920x1080`; a struct one `field: value` a line, a field left out where it is none; a
/// sequence of structs as blocks parted by a blank line, indented inside a field; a variant of fields as its name
/// and a colon, its fields beneath; and JSON where no word reads the value, as a map. What the walk to and from JSON
/// holds is charged to `budget`.
///
/// # Errors
///
/// A [`JsonError`] when `bytes` is not a value of `schema`, or the budget is spent.
pub fn print_value(schema: &SchemaNodes, bytes: &[u8], budget: &mut Budget) -> Result<String, JsonError> {
    let text = to_json(schema, bytes, budget)?;
    let tree = read_json(&text, budget)?;
    let root = schema.count().saturating_sub(1);
    let printed = Printer { schema, tree: &tree }.value(root, tree.root(), 0);

    let printed = match printed {
        Printed::Word(word) => word,
        Printed::Lines(lines) => lines.join("\n"),
    };
    Ok(printed)
}

/// An image a tool answered, printed as one line: its size when it is a PNG, its bytes, and the text beside it
/// (ADR-0053).
#[must_use]
pub fn print_image(png: &[u8], text: &str) -> String {
    let bytes = png.len();
    let Some((width, height)) = png_size(png) else {
        return format!("an image of {bytes} bytes: {text}");
    };

    format!("an image {width}x{height}, {bytes} bytes: {text}")
}

/// The width and height a PNG's header states: none when `png` does not begin as a PNG does.
fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    let signature = png.get(..8)? == b"\x89PNG\r\n\x1a\n";
    let header = png.get(12..16)? == b"IHDR";
    let named = signature && header;
    if !named {
        return None;
    }
    let width = u32::from_be_bytes(png.get(16..20)?.try_into().ok()?);
    let height = u32::from_be_bytes(png.get(20..24)?.try_into().ok()?);

    Some((width, height))
}

/// A walk over a value of a schema, as JSON read into a tree.
struct Printer<'walk, 'json> {
    schema: &'walk SchemaNodes,
    tree: &'walk JsonTree<'json>,
}

impl Printer<'_, '_> {
    /// How `value`, of the node at `node`, prints, `depth` values below the root.
    ///
    /// It recurses once per value it prints field by field, and prints as JSON past [`MOST_DEPTH`].
    fn value(&self, node: u32, value: &JsonValue<'_>, depth: usize) -> Printed {
        let deep = depth > MOST_DEPTH;
        let Some(shape) = self.schema.node(node).filter(|_| !deep) else {
            return self.json(value);
        };
        let word = Form::of_node(self.schema, node).write(self.tree, value);
        if let Some(word) = word {
            return Printed::Word(word);
        }

        match (shape, value) {
            (SchemaNode::Option(_), JsonValue::Null) => Printed::Word(String::from("none")),
            (SchemaNode::Option(inner), value) => self.value(*inner, value, depth + 1),
            (SchemaNode::Struct { fields, .. }, JsonValue::Object(span)) => {
                Printed::Lines(self.fields(fields, self.tree.members(*span), depth))
            }
            (SchemaNode::Struct { fields, .. }, value) => match fields.as_slice() {
                [only] => self.value(only.shape, value, depth + 1),
                _ => self.json(value),
            },
            (SchemaNode::Sequence(element) | SchemaNode::Array { element, .. }, JsonValue::Array(span)) => {
                self.blocks(*element, value, self.tree.elements(*span), depth)
            }
            (SchemaNode::Enum { variants, .. }, JsonValue::String(name)) => {
                let known = variants.iter().any(|variant| variant.name == *name);
                if !known {
                    return self.json(value);
                }
                Printed::Word(kebab(name))
            }
            (SchemaNode::Enum { variants, .. }, JsonValue::Object(span)) => {
                let [member] = self.tree.members(*span) else {
                    return self.json(value);
                };
                let Some(variant) = variants.iter().find(|variant| variant.name == member.name) else {
                    return self.json(value);
                };
                let name = kebab(&variant.name);
                let held = match (&variant.fields, &member.value) {
                    (FieldsNode::Unnamed(indices), held) => match indices.as_slice() {
                        [only] => self.value(*only, held, depth + 1),
                        _ => return self.json(value),
                    },
                    (FieldsNode::Named(fields), JsonValue::Object(span)) => {
                        Printed::Lines(self.fields(fields, self.tree.members(*span), depth))
                    }
                    (FieldsNode::Named(_), _) => return self.json(value),
                };
                match held {
                    Printed::Word(word) => Printed::Word(format!("{name}:{word}")),
                    Printed::Lines(lines) => Printed::Lines(beneath(format!("{name}:"), lines)),
                }
            }
            _ => self.json(value),
        }
    }

    /// The lines of a struct's `fields` whose values are `members`: each `field: value`, or `field:` with its lines
    /// indented beneath; a field that is none, or absent, left out.
    fn fields(&self, fields: &[FieldNode], members: &[JsonMember<'_>], depth: usize) -> Vec<String> {
        let mut lines = Vec::new();
        for field in fields {
            let Some(member) = members.iter().find(|member| member.name == field.name) else {
                continue;
            };
            let none = member.value == JsonValue::Null;
            let optional = matches!(self.schema.node(field.shape), Some(SchemaNode::Option(_)));
            let left_out = none && optional;
            if left_out {
                continue;
            }
            match self.value(field.shape, &member.value, depth + 1) {
                Printed::Word(word) => lines.push(format!("{}: {word}", field.name)),
                Printed::Lines(beneath_it) => lines.extend(beneath(format!("{}:", field.name), beneath_it)),
            }
        }

        lines
    }

    /// The elements of a sequence, each of the node at `element`, as blocks parted by a blank line, when each prints
    /// as lines; the whole sequence, `value`, as JSON otherwise.
    fn blocks(&self, element: u32, value: &JsonValue<'_>, elements: &[JsonValue<'_>], depth: usize) -> Printed {
        let mut lines: Vec<String> = Vec::new();
        for item in elements {
            let Printed::Lines(block) = self.value(element, item, depth + 1) else {
                return self.json(value);
            };
            let later = !lines.is_empty();
            if later {
                lines.push(String::new());
            }
            lines.extend(block);
        }
        let empty = lines.is_empty();
        if empty {
            return self.json(value);
        }

        Printed::Lines(lines)
    }

    /// `value` as compact JSON.
    fn json(&self, value: &JsonValue<'_>) -> Printed {
        let mut text = String::new();
        self.tree.write(value, &mut text);

        Printed::Word(text)
    }
}

/// `header` with `lines` beneath it, each indented two spaces.
fn beneath(header: String, lines: Vec<String>) -> Vec<String> {
    iter::once(header).chain(lines.into_iter().map(indented)).collect()
}

/// `line` indented two spaces; a blank line stays blank.
fn indented(line: String) -> String {
    let blank = line.is_empty();
    if blank {
        return line;
    }

    format!("  {line}")
}
