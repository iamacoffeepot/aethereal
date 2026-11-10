//! A tool's help: how its line is typed, what it does, and each flag with its field's doc comment (ADR-0053).

use super::CommandLine;
use super::flags::Entry;
use aethereal_data::SchemaNode;

impl CommandLine<'_> {
    /// The tool's help: its line as it is typed, its description, then each flag, what it takes, and its field's doc
    /// comment beneath, indented.
    ///
    /// ```text
    /// describe_engine <engine> --show <show>
    ///
    /// Describes an engine's tools. …
    ///
    /// <engine>: a number
    ///     The engine to describe.
    /// -s, --show: `names` or `sheets:<a,b,…>`
    ///     Which tools are shown, and how much of each.
    /// ```
    #[must_use]
    pub fn help(&self) -> String {
        let mut usage = self.sheet.name.clone();
        for entry in &self.entries {
            usage.push(' ');
            usage.push_str(&typed(entry));
        }
        let flagless = self.entries.is_empty();
        let fields = matches!(self.sheet.input.root(), SchemaNode::Struct { fields, .. } if !fields.is_empty());
        let json = flagless && fields;
        if json {
            usage.push_str(" <json>");
        }

        let mut lines = Vec::from([usage, String::new(), self.sheet.description.clone()]);
        if json {
            lines.push(String::new());
            lines.push(String::from("<json>: the input, as one JSON word of its schema"));
        }
        if !flagless {
            lines.push(String::new());
        }
        for entry in &self.entries {
            lines.push(format!("{}: {}", names(entry), entry.form.expects()));
            lines.extend(entry.flag.doc.lines().map(|line| format!("    {line}")));
        }

        lines.join("\n")
    }
}

/// How the flag of `entry` is typed in a line: `<engine>`, `--show <show>`, or `--full`, in brackets when it may be
/// left out.
fn typed(entry: &Entry<'_>) -> String {
    let field = &entry.flag.field;
    let typed = match entry.long() {
        None => format!("<{field}>"),
        Some(long) if entry.switch() => format!("--{long}"),
        Some(long) => format!("--{long} <{field}>"),
    };
    if entry.required {
        return typed;
    }

    format!("[{typed}]")
}

/// The names of the flag of `entry`: `<engine>`, `--show`, or `-s, --show`, with `--no-full` for a switch.
fn names(entry: &Entry<'_>) -> String {
    let Some(long) = entry.long() else {
        return format!("<{}>", entry.flag.field);
    };
    let short = entry.short().map(|short| format!("-{short}"));
    let negated = entry.switch().then(|| format!("--no-{long}"));

    short.into_iter().chain([format!("--{long}")]).chain(negated).collect::<Vec<String>>().join(", ")
}
