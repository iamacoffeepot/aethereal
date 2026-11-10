//! A tool's help: how its line is typed, what it does, and each argument beside its field's doc comment, in columns
//! that line up in the console's monospace font (ADR-0053).

use super::CommandLine;
use super::flags::Entry;
use super::layout::{GAP, padded, width_of, wrapped};
use aethereal_data::SchemaNode;
use aethereal_data::config::{Form, VariantForm, VariantValue, kebab};

/// The most characters a flag's value placeholder spells out as its words, `debug|info|warn|error`; a longer set is
/// named by its field, `<show>`, and its help says what it takes.
const MOST_SPELLED: usize = 24;

/// The fewest characters a doc column is given: on a narrower line it runs past the line's width rather than wrap a
/// word to a line.
const LEAST_DOC: usize = 20;

/// What a listing of many tools writes in place of the flags that may be left out.
const LEFT_OUT: &str = "[…]";

/// The pieces of a tool's usage line, apart, so a listing of many tools can pad each to its column and leave out the
/// flags that may be left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Usage<'sheet> {
    /// The tool's name.
    pub name: &'sheet str,
    /// The fields filled by position, in declared order, `<window> <name>`; `<json>` for a tool that takes its input
    /// as one JSON word; empty for none.
    pub positionals: String,
    /// The flags that must be given, in declared order, each by its short name where it has one, with its value's
    /// placeholder: `-n N`; empty for none.
    pub required: String,
    /// The flags that may be left out, each in brackets, in declared order, typed as the required ones are:
    /// `[-r N] [-c whole]`; empty for none.
    pub optional: String,
}

impl Usage<'_> {
    /// The usage line: its pieces parted by a space, an empty piece left out.
    #[must_use]
    pub fn line(&self) -> String {
        joined(&[self.name, &self.positionals, &self.required, &self.optional])
    }

    /// The flags, those that must be given first, then those that may be left out: `-n N [-r N] [-c whole]`; empty for
    /// none.
    #[must_use]
    pub fn flags(&self) -> String {
        joined(&[&self.required, &self.optional])
    }

    /// The arguments as a listing of many tools shows them: the fields filled by position, the flags that must be
    /// given, and `[…]` for those that may be left out, where there are any: `<window> <name> -n N […]`.
    #[must_use]
    pub fn arguments(&self) -> String {
        let left_out = if self.optional.is_empty() {
            ""
        } else {
            LEFT_OUT
        };

        joined(&[&self.positionals, &self.required, left_out])
    }
}

/// `pieces` parted by a space, an empty piece left out.
fn joined(pieces: &[&str]) -> String {
    pieces.iter().filter(|piece| !piece.is_empty()).copied().collect::<Vec<&str>>().join(" ")
}

impl<'sheet> CommandLine<'sheet> {
    /// The usage line's pieces: the tool's name, the fields filled by position, the flags that must be given, and
    /// those that may be left out.
    #[must_use]
    pub fn usage_parts(&self) -> Usage<'sheet> {
        let positionals: Vec<String> = if self.typed_as_json() {
            Vec::from([String::from("<json>")])
        } else {
            self.entries
                .iter()
                .filter(|entry| entry.positional())
                .map(|entry| format!("<{}>", entry.flag.field))
                .collect()
        };
        let flags = self.entries.iter().filter(|entry| !entry.positional());
        let required: Vec<String> = flags.clone().filter(|entry| entry.required).map(typed).collect();
        let optional: Vec<String> =
            flags.filter(|entry| !entry.required).map(|entry| format!("[{}]", typed(entry))).collect();

        Usage {
            name: &self.sheet.name,
            positionals: positionals.join(" "),
            required: required.join(" "),
            optional: optional.join(" "),
        }
    }

    /// The first sentence of the tool's description, which a listing of tools shows beside its usage.
    #[must_use]
    pub fn summary(&self) -> &'sheet str {
        first_sentence(&self.sheet.description)
    }

    /// The tool's help, `help <tool>`, its prose wrapped to `width` characters: the usage line; the description's
    /// first sentence, then the rest of it as a paragraph; a blank line; and one row per argument, in the usage's
    /// order, in two columns: what is typed, padded to the widest, and the field's doc comment, wrapped beneath its
    /// own column. A doc says what its field takes where the usage names it only by the field, and what of its form
    /// cannot be typed.
    ///
    /// ```text
    /// record_clip <window> <name> -n N [-r N] [-c whole] [-l N]
    /// Records …
    ///
    /// <window>          The window, by its number. Takes a number.
    /// -c, --crop whole  What of each frame is recorded: none for `whole`.
    ///                   Only `whole` can be typed; `Region` has several …
    /// ```
    #[must_use]
    pub fn help(&self, width: usize) -> String {
        let usage = self.usage_parts().line();
        let summary = self.summary();
        let rest = self.sheet.description[summary.len()..].trim();
        let rows: Vec<(String, String)> = if self.typed_as_json() {
            Vec::from([(String::from("<json>"), String::from("The input, as one JSON word of its schema."))])
        } else {
            self.ordered().map(|entry| (names(entry), about(entry))).collect()
        };

        let mut lines = Vec::from([usage]);
        lines.extend(wrapped(summary, width));
        lines.extend(wrapped(rest, width));
        let argued = !rows.is_empty();
        if argued {
            lines.push(String::new());
            lines.extend(beside(&rows, width));
        }

        lines.join("\n")
    }

    /// Whether the tool takes its input as one JSON word: it has no flags, and its input has fields.
    fn typed_as_json(&self) -> bool {
        let flagless = self.entries.is_empty();
        let fields = matches!(self.sheet.input.root(), SchemaNode::Struct { fields, .. } if !fields.is_empty());

        flagless && fields
    }

    /// The entries in the usage's order: those filled by position, then the flags that must be given, then those that
    /// may be left out, each in declared order.
    fn ordered(&self) -> impl Iterator<Item = &Entry<'sheet>> {
        let positional = self.entries.iter().filter(|entry| entry.positional());
        let required = self.entries.iter().filter(|entry| !entry.positional() && entry.required);
        let optional = self.entries.iter().filter(|entry| !entry.positional() && !entry.required);

        positional.chain(required).chain(optional)
    }
}

/// The first sentence of `text`: up to and including its first `.` that whitespace or the end of the text follows,
/// so the point of `0.5` ends nothing. Without one, the whole text.
#[must_use]
pub fn first_sentence(text: &str) -> &str {
    text.match_indices('.')
        .map(|(at, _)| text.split_at(at + 1))
        .find(|(_, rest)| rest.chars().next().is_none_or(char::is_whitespace))
        .map_or(text, |(sentence, _)| sentence)
}

/// How the flag of `entry` is typed in the usage: its short name where it has one, its long one otherwise, and its
/// value's placeholder, `-n N` or `--level debug|info|warn|error`; a switch by its name alone.
fn typed(entry: &Entry<'_>) -> String {
    let name = entry.short().map_or_else(|| entry.named(), |short| format!("-{short}"));
    if entry.switch() {
        return name;
    }

    format!("{name} {}", placeholder(entry))
}

/// What is typed for an argument, as the left column of its help row: `<window>`, `-c, --crop whole`, or
/// `--full, --no-full` for a switch.
fn names(entry: &Entry<'_>) -> String {
    let Some(long) = entry.long() else {
        return format!("<{}>", entry.flag.field);
    };
    let short = entry.short().map_or_else(String::new, |short| format!("-{short}, "));
    if entry.switch() {
        return format!("{short}--{long}, --no-{long}");
    }

    format!("{short}--{long} {}", placeholder(entry))
}

/// The placeholder of a flag's value: its form's words where they are few, `N`, `WxH`, or `whole`; the field's name,
/// `<contains>`, otherwise.
fn placeholder(entry: &Entry<'_>) -> String {
    spelled(&entry.form).unwrap_or_else(|| format!("<{}>", entry.flag.field))
}

/// The words a value of `form` is typed as, when they are few enough to stand in the usage: none for text, JSON, a
/// list, and a set of variants longer than [`MOST_SPELLED`].
fn spelled(form: &Form) -> Option<String> {
    match form {
        Form::Number(_) => Some(String::from("N")),
        Form::Size { .. } => Some(String::from("WxH")),
        Form::Switch => Some(String::from("true|false")),
        Form::Variant { variants, .. } => {
            let words = variants.iter().map(variant_word).collect::<Vec<String>>().join("|");
            let few = width_of(&words) <= MOST_SPELLED;
            few.then_some(words)
        }
        Form::Text | Form::List { .. } | Form::Json => None,
    }
}

/// How `variant` is typed in a placeholder: its name, `whole`, or its name, a colon, and what its field takes,
/// `sheets:a,b`.
fn variant_word(variant: &VariantForm) -> String {
    let name = kebab(&variant.name);
    let held = match &variant.value {
        VariantValue::Bare => return name,
        VariantValue::Positional(form) | VariantValue::Named { form, .. } => form,
    };
    let held = match held {
        Form::Number(_) => "N",
        Form::Size { .. } => "WxH",
        Form::Switch => "true|false",
        Form::Text => "<text>",
        Form::List { .. } => "a,b",
        Form::Variant { .. } => "<variant>",
        Form::Json => "<json>",
    };

    format!("{name}:{held}")
}

/// The right column of an argument's help row: its field's doc comment; what it takes, where what is typed names it
/// only by the field; and what of its form cannot be typed.
fn about(entry: &Entry<'_>) -> String {
    let mut sentences = Vec::from([entry.flag.doc.split_whitespace().collect::<Vec<&str>>().join(" ")]);
    let by_field = entry.positional() || spelled(&entry.form).is_none();
    let plain = matches!(entry.form, Form::Text | Form::Switch);
    let told = by_field && !plain;
    if told {
        sentences.push(format!("Takes {}.", entry.form.expects()));
    }
    sentences.extend(untypable(&entry.form));

    sentences.into_iter().filter(|sentence| !sentence.is_empty()).collect::<Vec<String>>().join(" ")
}

/// What of `form` cannot be typed, when something cannot: the variants of several fields, which only JSON sets.
fn untypable(form: &Form) -> Option<String> {
    let Form::Variant { variants, json_only } = form else {
        return None;
    };
    if json_only.is_empty() {
        return None;
    }
    let typable: Vec<String> = variants.iter().map(|variant| format!("`{}`", variant_word(variant))).collect();
    let held: Vec<String> = json_only.iter().map(|name| format!("`{name}`")).collect();
    let (has, is) = if held.len() == 1 {
        ("has", "is")
    } else {
        ("have", "are")
    };

    Some(format!(
        "Only {} can be typed; {} {has} several fields and {is} given as JSON, through `call_tool` or an agent.",
        listed(&typable, "or"),
        listed(&held, "and"),
    ))
}

/// `words` as prose: `a`, `a or b`, or `a, b, or c`, joined by `conjunction`.
fn listed(words: &[String], conjunction: &str) -> String {
    match words.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, [first])) => format!("{first} {conjunction} {last}"),
        Some((last, rest)) => format!("{}, {conjunction} {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// `rows` in two columns: the left padded to the widest, the right wrapped to what `width` leaves it, each line after
/// a row's first indented under the right column.
fn beside(rows: &[(String, String)], width: usize) -> Vec<String> {
    let left = rows.iter().map(|(names, _)| width_of(names)).max().unwrap_or(0);
    let indent = left + GAP.len();
    let room = width.saturating_sub(indent).max(LEAST_DOC);

    let mut lines = Vec::new();
    for (names, doc) in rows {
        let mut wrapped = wrapped(doc, room).into_iter();
        let first = wrapped.next().unwrap_or_default();
        lines.push(format!("{}{GAP}{first}", padded(names, left)).trim_end().to_owned());
        lines.extend(wrapped.map(|line| format!("{}{line}", " ".repeat(indent))));
    }

    lines
}
