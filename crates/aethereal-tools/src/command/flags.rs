//! A sheet's flags, each with its field's form, checked against the input (ADR-0053).

use super::FlagsFault;
use crate::{ToolFlag, ToolSheet, Typed};
use aethereal_data::config::Form;
use aethereal_data::{FieldNode, SchemaNode};

/// One flag of a sheet, with the form its field is read in and whether the field must be given.
pub(super) struct Entry<'sheet> {
    pub(super) flag: &'sheet ToolFlag,
    pub(super) form: Form,
    pub(super) required: bool,
}

impl Entry<'_> {
    /// Whether the field is filled by position.
    pub(super) fn positional(&self) -> bool {
        self.flag.typed == Typed::Position
    }

    /// Whether the field is a switch: typed by its name alone, `--name` on and `--no-name` off.
    pub(super) fn switch(&self) -> bool {
        self.form == Form::Switch
    }

    /// The flag's long name: none for a field filled by position.
    pub(super) fn long(&self) -> Option<&str> {
        match &self.flag.typed {
            Typed::Position => None,
            Typed::Long { long } | Typed::LongAndShort { long, .. } => Some(long),
        }
    }

    /// The flag's short letter, when it has one.
    pub(super) const fn short(&self) -> Option<char> {
        match &self.flag.typed {
            Typed::LongAndShort { short, .. } => Some(*short),
            Typed::Position | Typed::Long { .. } => None,
        }
    }

    /// The flag as a person names it: `--show`, or the field's name for one filled by position.
    pub(super) fn named(&self) -> String {
        self.long().map_or_else(|| self.flag.field.clone(), |long| format!("--{long}"))
    }
}

/// A tool's command line: its sheet, and the sheet's flags, each with the form its field is read in (ADR-0053).
///
/// Made only of flags that fit the input, so a line read by it is read as the input's schema reads it.
pub struct CommandLine<'sheet> {
    pub(super) sheet: &'sheet ToolSheet,
    /// The flags, in the sheet's order: none for a tool that takes its input as one JSON word.
    pub(super) entries: Vec<Entry<'sheet>>,
}

impl<'sheet> CommandLine<'sheet> {
    /// The command line of `sheet`, refusing flags that do not fit its input: a flag naming a field it lacks, two
    /// flags of one field or one name, a name that cannot be typed, a field filled by position that is a switch or
    /// must be given after one that may be left out, and a field that must be given with no flag.
    ///
    /// # Errors
    ///
    /// The first misfit found, as [`FlagsFault`] says.
    pub fn of(sheet: &'sheet ToolSheet) -> Result<Self, FlagsFault> {
        let flagless = sheet.flags.is_empty();
        if flagless {
            return Ok(Self { sheet, entries: Vec::new() });
        }
        let SchemaNode::Struct { fields, .. } = sheet.input.root() else {
            return Err(FlagsFault::NotAStruct);
        };

        let mut entries: Vec<Entry<'sheet>> = Vec::new();
        for flag in &sheet.flags {
            let Some(field) = fields.iter().find(|field| field.name == flag.field) else {
                return Err(FlagsFault::NoField { field: flag.field.clone() });
            };
            let twice = entries.iter().any(|entry| entry.flag.field == flag.field);
            if twice {
                return Err(FlagsFault::TwoFlags { field: flag.field.clone() });
            }
            let optional = matches!(sheet.input.node(field.shape), Some(SchemaNode::Option(_)));
            let form = Form::of_node(&sheet.input, field.shape);
            entries.push(Entry { flag, form, required: !optional });
        }

        refuse_names(&entries)?;
        refuse_positions(&entries)?;
        refuse_unflagged(fields, &entries, sheet)?;

        Ok(Self { sheet, entries })
    }

    /// The sheet the command line is of.
    #[must_use]
    pub const fn sheet(&self) -> &'sheet ToolSheet {
        self.sheet
    }
}

/// Refuses a name that cannot be typed, and two flags typed alike.
fn refuse_names(entries: &[Entry<'_>]) -> Result<(), FlagsFault> {
    let mut names: Vec<String> = Vec::new();
    for entry in entries {
        if let Some(long) = entry.long() {
            let typable = !long.is_empty() && !long.starts_with('-') && !long.contains(['=', ' ', '\t', '\n']);
            if !typable {
                return Err(FlagsFault::BadName { name: format!("--{long}") });
            }
            names.push(format!("--{long}"));
            if entry.switch() {
                names.push(format!("--no-{long}"));
            }
        }
        if let Some(short) = entry.short() {
            let letter = short.is_ascii_alphabetic();
            if !letter {
                return Err(FlagsFault::BadName { name: format!("-{short}") });
            }
            names.push(format!("-{short}"));
        }
    }

    for (position, name) in names.iter().enumerate() {
        let repeated = names.iter().skip(position + 1).any(|other| other == name);
        if repeated {
            return Err(FlagsFault::SameName { name: name.clone() });
        }
    }

    Ok(())
}

/// Refuses a switch filled by position, and a field filled by position that must be given after one that may be left
/// out.
fn refuse_positions(entries: &[Entry<'_>]) -> Result<(), FlagsFault> {
    let mut left_out = false;
    for entry in entries.iter().filter(|entry| entry.positional()) {
        if entry.switch() {
            return Err(FlagsFault::PositionalSwitch { field: entry.flag.field.clone() });
        }
        let after = left_out && entry.required;
        if after {
            return Err(FlagsFault::RequiredAfterOptional { field: entry.flag.field.clone() });
        }
        left_out = !entry.required;
    }

    Ok(())
}

/// Refuses a field of the input that must be given and has no flag.
fn refuse_unflagged(fields: &[FieldNode], entries: &[Entry<'_>], sheet: &ToolSheet) -> Result<(), FlagsFault> {
    for field in fields {
        let optional = matches!(sheet.input.node(field.shape), Some(SchemaNode::Option(_)));
        let flagged = entries.iter().any(|entry| entry.flag.field == field.name);
        let given = optional || flagged;
        if !given {
            return Err(FlagsFault::Unflagged { field: field.name.clone() });
        }
    }

    Ok(())
}
