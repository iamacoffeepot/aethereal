//! How a flag's value is read, picked by its field's schema type, each written once (ADR-0028).
//!
//! A form reads the text a person typed into the JSON value the field holds, so the command line and a page's query
//! read values the same way. A form exists only where text is typed: what it sets is the typed value, in the
//! document.

use super::ConfigFault;
use super::document::{Json, Leaf};
use crate::json::layout::{At, Layout};
use crate::json::reader;
use crate::json::tree::Value;
use crate::{Budget, Float, Integer, SchemaNode, VariantNode};
use alloc::string::String;
use alloc::vec::Vec;

/// How a flag's value is read, by its field's schema type (ADR-0028).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Form {
    /// Two numbers joined by `x`, `1280x720`: a field of two numbers, as an array, a tuple, or a struct of two
    /// positional fields.
    Size {
        /// How the first is written.
        width: NumberForm,
        /// How the second is written.
        height: NumberForm,
    },
    /// A variant's name in kebab case, `very-hard`, and for a variant of one field, the field's value after a
    /// colon, `at-most:2`: an enum, by its variants of no fields or one.
    Variant {
        /// The variants a flag's value names, each of no fields or one, in declared order.
        variants: Vec<VariantForm>,
    },
    /// A `bool`: the flag alone turns it on and `--no-<name>` off, and text is `true` or `false`.
    Switch,
    /// A number.
    Number(NumberForm),
    /// The text itself: a string, or bytes in hexadecimal.
    Text,
    /// JSON text: a structure, a sequence, a map, and anything no other form reads.
    Json,
}

/// One variant of an enum as a flag's value names it (ADR-0028).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantForm {
    /// The variant's name, as the schema spells it; a flag's value spells it in kebab case.
    pub name: String,
    /// What follows the name.
    pub value: VariantValue,
}

/// What follows a variant's name in a flag's value, and how the variant is written in the document (ADR-0007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VariantValue {
    /// Nothing: a variant of no fields, `screen`, written as its name.
    Bare,
    /// A colon and the value of the variant's one positional field, `at-most:2`, written as the field's value alone
    /// under the variant's name.
    Positional(Form),
    /// A colon and the value of the variant's one named field, `for:3`, written as an object of that field under the
    /// variant's name.
    Named {
        /// The field's name.
        field: String,
        /// How the field's value is read.
        form: Form,
    },
}

impl VariantForm {
    /// How a flag's value names `variant`, an enum's at `at`, or nothing when it has several fields, which only JSON
    /// sets.
    fn of(at: At<'_>, variant: &VariantNode) -> Option<Self> {
        let value = match Layout::of_variant(&variant.fields) {
            Layout::Empty => VariantValue::Bare,
            Layout::Single(index) => VariantValue::Positional(Form::of(at.child(index).ok()?)),
            Layout::Named([field]) => {
                let form = Form::of(at.child(field.shape).ok()?);
                VariantValue::Named { field: field.name.clone(), form }
            }
            Layout::Several(_) | Layout::Named(_) => return None,
        };

        Some(Self { name: variant.name.clone(), value })
    }

    /// The value `text` states when it names this variant, or nothing when it names another.
    fn read(&self, text: &str) -> Option<Result<Json, ConfigFault>> {
        let name = kebab(&self.name);
        let form = match &self.value {
            VariantValue::Bare => {
                let named = text == name;
                return named.then(|| Ok(Json::Leaf(Leaf::String(self.name.clone()))));
            }
            VariantValue::Positional(form) | VariantValue::Named { form, .. } => form,
        };

        let (word, value) = text.split_once(':')?;
        let named = word == name;
        if !named {
            return None;
        }

        Some(form.read(value).map(|value| self.holding(value)))
    }

    /// This variant holding `value` in its one field, as the document writes it.
    fn holding(&self, value: Json) -> Json {
        let held = match &self.value {
            VariantValue::Named { field, .. } => Json::Object(Vec::from([(field.clone(), value)])),
            VariantValue::Bare | VariantValue::Positional(_) => value,
        };

        Json::Object(Vec::from([(self.name.clone(), held)]))
    }
}

/// How a number a person typed is written in the document (ADR-0007).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberForm {
    /// An integer of 8, 16, or 32 bits: a JSON number.
    Narrow,
    /// An integer of 64 bits: a string of its digits.
    Wide,
    /// A float: a JSON number, or `NaN`, `Infinity`, or `-Infinity`.
    Float,
}

impl NumberForm {
    /// How a number of `node` is written, or nothing when `node` is no number.
    fn of(node: &SchemaNode) -> Option<Self> {
        match node {
            SchemaNode::Integer(Integer::U64 | Integer::I64) => Some(Self::Wide),
            SchemaNode::Integer(_) => Some(Self::Narrow),
            SchemaNode::Float(Float::F32 | Float::F64) => Some(Self::Float),
            SchemaNode::Boolean
            | SchemaNode::String
            | SchemaNode::Bytes
            | SchemaNode::Option(_)
            | SchemaNode::Sequence(_)
            | SchemaNode::Array { .. }
            | SchemaNode::Map { .. }
            | SchemaNode::Struct { .. }
            | SchemaNode::Tuple { .. }
            | SchemaNode::Enum { .. } => None,
        }
    }

    /// The number `text` states, written as this form writes it.
    fn read(self, text: &str) -> Result<Json, ConfigFault> {
        let named = matches!(text, "NaN" | "Infinity" | "-Infinity");
        let float_name = named && self == Self::Float;
        if float_name {
            return Ok(Json::Leaf(Leaf::String(String::from(text))));
        }

        let number = number_text(text)?;
        let leaf = match self {
            Self::Narrow | Self::Float => Leaf::Number(number),
            Self::Wide => Leaf::String(number),
        };

        Ok(Json::Leaf(leaf))
    }
}

impl Form {
    /// The form a field of the schema node at `at` is read in.
    pub(super) fn of(at: At<'_>) -> Self {
        let node = at.node();
        let number = NumberForm::of(node);
        if let Some(number) = number {
            return Self::Number(number);
        }

        match node {
            SchemaNode::Boolean => Self::Switch,
            SchemaNode::String | SchemaNode::Bytes => Self::Text,
            SchemaNode::Enum { variants, .. } => {
                let variants: Vec<VariantForm> =
                    variants.iter().filter_map(|variant| VariantForm::of(at, variant)).collect();
                let no_word = variants.is_empty();
                if no_word {
                    return Self::Json;
                }
                Self::Variant { variants }
            }
            SchemaNode::Array { element, length } => {
                let index = *element;
                let Ok(element) = at.child(index) else {
                    return Self::Json;
                };
                let bytes = element.node() == &SchemaNode::Integer(Integer::U8);
                if bytes {
                    return Self::Text;
                }
                let pair = *length == 2;
                if !pair {
                    return Self::Json;
                }
                Self::size(at, index, index)
            }
            SchemaNode::Tuple { elements } => {
                let [first, second] = elements.as_slice() else {
                    return Self::Json;
                };
                Self::size(at, *first, *second)
            }
            SchemaNode::Struct { fields, .. } => {
                let Layout::Several(indices) = Layout::of_struct(fields) else {
                    return Self::Json;
                };
                let [first, second] = indices.as_slice() else {
                    return Self::Json;
                };
                Self::size(at, *first, *second)
            }
            SchemaNode::Integer(_)
            | SchemaNode::Float(_)
            | SchemaNode::Option(_)
            | SchemaNode::Sequence(_)
            | SchemaNode::Map { .. } => Self::Json,
        }
    }

    /// A size when the nodes `first` and `second`, below `at`, are numbers; JSON otherwise.
    fn size(at: At<'_>, first: u32, second: u32) -> Self {
        let width = at.child(first).ok().and_then(|child| NumberForm::of(child.node()));
        let height = at.child(second).ok().and_then(|child| NumberForm::of(child.node()));
        let (Some(width), Some(height)) = (width, height) else {
            return Self::Json;
        };

        Self::Size { width, height }
    }

    /// The value `text` states for a field of this form.
    pub(super) fn read(&self, text: &str) -> Result<Json, ConfigFault> {
        match self {
            Self::Size { width, height } => {
                let Some((first, second)) = text.split_once('x') else {
                    return Err(ConfigFault::NotASize);
                };
                let width = width.read(first).map_err(|_| ConfigFault::NotASize)?;
                let height = height.read(second).map_err(|_| ConfigFault::NotASize)?;
                Ok(Json::Array(Vec::from([width, height])))
            }
            Self::Variant { variants } => {
                let found = variants.iter().find_map(|variant| variant.read(text));
                found.unwrap_or(Err(ConfigFault::NotAVariant))
            }
            Self::Switch => match text {
                "true" => Ok(Json::Leaf(Leaf::Boolean(true))),
                "false" => Ok(Json::Leaf(Leaf::Boolean(false))),
                _ => Err(ConfigFault::NotABoolean),
            },
            Self::Number(number) => number.read(text),
            Self::Text => Ok(Json::Leaf(Leaf::String(String::from(text)))),
            Self::Json => Json::read(text),
        }
    }
}

/// The number JSON text states, as it reads without the whitespace around it, refusing text that states none.
///
/// The text is in hand, so what reading it holds grows with its length and needs no budget of its own.
fn number_text(text: &str) -> Result<String, ConfigFault> {
    let mut budget = Budget::new(usize::MAX);
    let tree = reader::read(text, &mut budget).map_err(|_| ConfigFault::NotANumber)?;
    let Value::Number(number) = tree.root() else {
        return Err(ConfigFault::NotANumber);
    };

    Ok(String::from(*number))
}

/// A variant's name in kebab case: `VeryHard` is `very-hard`, `HDR10` is `hdr10`, and `_` is `-`.
pub(super) fn kebab(name: &str) -> String {
    let characters: Vec<char> = name.chars().collect();
    let mut out = String::new();
    for (position, character) in characters.iter().enumerate() {
        let character = *character;
        let underscore = character == '_';
        if underscore {
            out.push('-');
            continue;
        }
        let upper = character.is_ascii_uppercase();
        let previous = position.checked_sub(1).and_then(|before| characters.get(before)).copied();
        let next = characters.get(position + 1).copied();
        let after_lower = previous.is_some_and(|before| before.is_ascii_lowercase() || before.is_ascii_digit());
        let ends_capitals = previous.is_some_and(|before| before.is_ascii_uppercase())
            && next.is_some_and(|after| after.is_ascii_lowercase());
        let starts_word = upper && (after_lower || ends_capitals);
        let joined = out.ends_with('-') || out.is_empty();
        if starts_word && !joined {
            out.push('-');
        }
        out.push(character.to_ascii_lowercase());
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::samples::{Density, Presentation};
    use crate::{Schema, SchemaNodes};

    /// The form a field of type `T` is read in.
    fn form_of<T: Schema>() -> Form {
        let schema = SchemaNodes::of(&T::SHAPE);
        Form::of(At::root(&schema))
    }

    /// A leaf of the number `text`.
    fn number(text: &str) -> Json {
        Json::Leaf(Leaf::Number(String::from(text)))
    }

    /// A leaf of the string `text`.
    fn string(text: &str) -> Json {
        Json::Leaf(Leaf::String(String::from(text)))
    }

    #[test]
    fn a_field_type_picks_its_form() {
        // Catches a size taken for a structure, bytes for a size, an enum read as JSON, or a 64-bit integer as a number.
        let size = Form::Size { width: NumberForm::Narrow, height: NumberForm::Narrow };
        assert_eq!(form_of::<[u32; 2]>(), size);
        assert_eq!(form_of::<(u16, f32)>(), Form::Size { width: NumberForm::Narrow, height: NumberForm::Float });
        assert_eq!(form_of::<[u8; 2]>(), Form::Text);
        assert_eq!(form_of::<[u32; 3]>(), Form::Json);
        let bare = |name: &str| VariantForm { name: String::from(name), value: VariantValue::Bare };
        assert_eq!(form_of::<Presentation>(), Form::Variant { variants: Vec::from([bare("Fifo"), bare("VeryFast")]) });
        assert_eq!(form_of::<bool>(), Form::Switch);
        assert_eq!(form_of::<u64>(), Form::Number(NumberForm::Wide));
        assert_eq!(form_of::<String>(), Form::Text);
        assert_eq!(form_of::<Vec<u32>>(), Form::Json);
    }

    #[test]
    fn a_size_is_two_numbers_joined_by_x() {
        // Catches a size read with its numbers swapped, a lone number taken, or a third number dropped.
        let size = Form::Size { width: NumberForm::Narrow, height: NumberForm::Wide };
        assert_eq!(size.read("1280x720"), Ok(Json::Array(Vec::from([number("1280"), string("720")]))));
        for bad in ["1280", "1280x", "x720", "axb", "1280x720x3", "1280X720"] {
            assert_eq!(size.read(bad), Err(ConfigFault::NotASize), "{bad}");
        }
    }

    #[test]
    fn an_enum_is_its_variant_in_kebab_case() {
        // Catches a variant read by its schema spelling, or a word that names none taken as a variant.
        let variant = form_of::<Presentation>();
        assert_eq!(variant.read("very-fast"), Ok(string("VeryFast")));
        assert_eq!(variant.read("fifo"), Ok(string("Fifo")));
        assert_eq!(variant.read("VeryFast"), Err(ConfigFault::NotAVariant));
        assert_eq!(kebab("HDR10"), "hdr10");
        assert_eq!(kebab("HTTPServer"), "http-server");
        assert_eq!(kebab("Two_Words"), "two-words");
    }

    #[test]
    fn a_variant_of_one_field_is_its_name_a_colon_and_the_field() {
        // Catches a variant of one field left to JSON, its value written beside its name rather than under it, a named
        // field's value written without its name, a value read by the wrong form, and a variant of two fields offered
        // as a word.
        let density = form_of::<Density>();
        let names: Vec<&str> = match &density {
            Form::Variant { variants } => variants.iter().map(|variant| variant.name.as_str()).collect(),
            Form::Size { .. } | Form::Switch | Form::Number(_) | Form::Text | Form::Json => Vec::new(),
        };
        let at_most = Json::Object(Vec::from([(String::from("AtMost"), number("2.5"))]));
        let pixels = Json::Object(Vec::from([(String::from("pixels"), number("3"))]));
        let at_least = Json::Object(Vec::from([(String::from("AtLeast"), pixels)]));

        assert_eq!(names, ["Screen", "AtMost", "AtLeast"]);
        assert_eq!(density.read("screen"), Ok(string("Screen")));
        assert_eq!(density.read("at-most:2.5"), Ok(at_most));
        assert_eq!(density.read("at-least:3"), Ok(at_least));
        assert_eq!(density.read("at-most:many"), Err(ConfigFault::NotANumber));
        for bad in ["at-most", "screen:1", "between:1", "AtMost:2"] {
            assert_eq!(density.read(bad), Err(ConfigFault::NotAVariant), "{bad}");
        }
    }

    #[test]
    fn a_switch_number_and_string_read_their_text() {
        // Catches a switch that takes any word, a number form that lets text through as JSON, or a string quoted.
        assert_eq!(Form::Switch.read("false"), Ok(Json::Leaf(Leaf::Boolean(false))));
        assert_eq!(Form::Switch.read("yes"), Err(ConfigFault::NotABoolean));
        assert_eq!(Form::Number(NumberForm::Narrow).read("4"), Ok(number("4")));
        assert_eq!(Form::Number(NumberForm::Narrow).read("four"), Err(ConfigFault::NotANumber));
        assert_eq!(Form::Number(NumberForm::Narrow).read("NaN"), Err(ConfigFault::NotANumber));
        assert_eq!(Form::Number(NumberForm::Wide).read("5"), Ok(string("5")));
        assert_eq!(Form::Number(NumberForm::Float).read("-Infinity"), Ok(string("-Infinity")));
        assert_eq!(Form::Text.read("a \"title\""), Ok(string("a \"title\"")));
        assert_eq!(
            Form::Json.read("{\"a\":[1]}"),
            Ok(Json::Object(Vec::from([(String::from("a"), Json::Array(Vec::from([number("1")])))])))
        );
        assert!(matches!(Form::Json.read("{"), Err(ConfigFault::Json(_))));
    }
}
