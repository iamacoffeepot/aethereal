//! Tab completion at the console (ADR-0053): the word before the cursor completed as a tool's name when it is the
//! first, as a flag's name when it starts with `-`, and as a variant's word when the word before it is a flag whose
//! field is a variant.

use super::command::{CLEAR, HELP};
use aethereal_data::SchemaNode;
use aethereal_data::config::{Form, VariantValue, kebab};
use aethereal_tools::{ToolFlag, ToolSheet, Typed};

/// What the word before the cursor completes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Completion {
    /// Nothing it could be.
    Nothing,
    /// One word: what is typed from the character `from` up to the cursor becomes `word`, followed by a space when
    /// the word is whole, as a variant's name before its colon is not.
    One { from: usize, word: String, whole: bool },
    /// Several: what is typed from the character `from` becomes what they all begin with, `common`, and they are
    /// listed.
    Several { from: usize, common: String, matches: Vec<String> },
}

/// What `before`, the line typed before the cursor, completes to among the tools of `sheets` and the console's own
/// words. Its words are split at whitespace, quotes and all, as only the last is completed.
pub fn complete(before: &str, sheets: &[ToolSheet]) -> Completion {
    let mut words: Vec<&str> = before.split_whitespace().collect();
    let starts_word = before.is_empty() || before.ends_with(char::is_whitespace);
    let partial = if starts_word {
        ""
    } else {
        words.pop().unwrap_or_default()
    };
    let from = before.chars().count() - partial.chars().count();

    let mut matches: Vec<String> =
        candidates(&words, partial, sheets).into_iter().filter(|word| word.starts_with(partial)).collect();
    matches.sort();
    matches.dedup();
    match matches.as_slice() {
        [] => Completion::Nothing,
        [word] => Completion::One { from, whole: !word.ends_with(':'), word: word.clone() },
        _ => Completion::Several { from, common: common_start(&matches), matches },
    }
}

/// What the word after `words` could be, the word typed so far being `partial`: a tool's name or the console's own
/// words first; a tool's name after `help`; a flag of the tool named first, for a word that starts with `-`; and a
/// variant's word after a flag whose field is a variant.
fn candidates(words: &[&str], partial: &str, sheets: &[ToolSheet]) -> Vec<String> {
    let names = sheets.iter().map(|sheet| sheet.name.clone());
    match words {
        [] => [HELP, CLEAR].into_iter().map(String::from).chain(names).collect(),
        [first] if *first == HELP => names.collect(),
        [first, ..] => {
            let Some(sheet) = sheets.iter().find(|sheet| sheet.name == *first) else {
                return Vec::new();
            };
            let flag_typed = partial.starts_with('-');
            if flag_typed {
                return sheet.flags.iter().flat_map(|flag| flag_names(sheet, flag)).collect();
            }
            let previous = words.last().copied().unwrap_or_default();
            sheet
                .flags
                .iter()
                .find(|flag| typed_as(flag, previous))
                .map_or_else(Vec::new, |flag| variant_words(sheet, flag))
        }
    }
}

/// How `flag` is typed by name in full: `--long`, and `--no-long` too for a switch; nothing for a field filled by
/// position.
fn flag_names(sheet: &ToolSheet, flag: &ToolFlag) -> Vec<String> {
    let long = match &flag.typed {
        Typed::Position => return Vec::new(),
        Typed::Long { long } | Typed::LongAndShort { long, .. } => long,
    };
    let switch = form_of(sheet, flag) == Form::Switch;
    if switch {
        return vec![format!("--{long}"), format!("--no-{long}")];
    }

    vec![format!("--{long}")]
}

/// Whether `word` names `flag`: `--long`, or `-s` for its short letter.
fn typed_as(flag: &ToolFlag, word: &str) -> bool {
    match &flag.typed {
        Typed::Position => false,
        Typed::Long { long } => word.strip_prefix("--") == Some(long.as_str()),
        Typed::LongAndShort { long, short } => {
            let by_long = word.strip_prefix("--") == Some(long.as_str());
            let by_short = word.strip_prefix('-').is_some_and(|letter| letter.chars().eq([*short]));
            by_long || by_short
        }
    }
}

/// The words a value of `flag`'s field is typed as, when its form is a variant: each variant's name, and a colon
/// after the name of one that holds a value. Nothing for any other form.
fn variant_words(sheet: &ToolSheet, flag: &ToolFlag) -> Vec<String> {
    let Form::Variant { variants, .. } = form_of(sheet, flag) else {
        return Vec::new();
    };

    variants
        .iter()
        .map(|variant| {
            let name = kebab(&variant.name);
            match variant.value {
                VariantValue::Bare => name,
                VariantValue::Positional(_) | VariantValue::Named { .. } => format!("{name}:"),
            }
        })
        .collect()
}

/// The form `flag`'s field is read in, picked by its schema as a setting's is: JSON when the input has no such field.
fn form_of(sheet: &ToolSheet, flag: &ToolFlag) -> Form {
    let SchemaNode::Struct { fields, .. } = sheet.input.root() else {
        return Form::Json;
    };
    fields
        .iter()
        .find(|field| field.name == flag.field)
        .map_or(Form::Json, |field| Form::of_node(&sheet.input, field.shape))
}

/// What every one of `words` begins with.
fn common_start(words: &[String]) -> String {
    let Some((first, rest)) = words.split_first() else {
        return String::new();
    };
    let mut common: Vec<char> = first.chars().collect();
    for word in rest {
        let shared = common.iter().zip(word.chars()).take_while(|(kept, other)| **kept == *other).count();
        common.truncate(shared);
    }

    common.into_iter().collect()
}
