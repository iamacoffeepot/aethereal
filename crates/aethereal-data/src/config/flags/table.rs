//! The table of a program's flags, checked: one flag of each name (ADR-0028).

use crate::config::document::Step;
use crate::config::form::Form;
use crate::{JsonPath, JsonStep};
use alloc::string::String;
use alloc::vec::Vec;

/// One flag: its names, the field it sets, the field's doc comment, and how its value is read (ADR-0028).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flag {
    pub(super) long: String,
    /// The short flag's letter, fixed as the flag is made; `None` when the flag has no short form.
    pub(super) short: Option<char>,
    pub(super) section: &'static str,
    pub(super) field: &'static str,
    pub(super) doc: &'static str,
    pub(super) form: Form,
}

impl Flag {
    /// The long flag's name, without the dashes: `workers` for `--workers`.
    #[must_use]
    pub fn long(&self) -> &str {
        &self.long
    }

    /// The short flag's letter, `w` for `-w`, when the flag has one.
    #[must_use]
    pub const fn short(&self) -> Option<char> {
        self.short
    }

    /// The doc comment of the field the flag sets.
    #[must_use]
    pub const fn doc(&self) -> &'static str {
        self.doc
    }

    /// How the flag's value is read.
    #[must_use]
    pub const fn form(&self) -> &Form {
        &self.form
    }

    /// The path of the field the flag sets, from the configuration's root: `engine.workers`.
    #[must_use]
    pub fn path(&self) -> JsonPath {
        let section = JsonStep::Field(String::from(self.section));
        let field = JsonStep::Field(String::from(self.field));
        JsonPath::of(Vec::from([section, field]))
    }

    /// The steps of the path of the field the flag sets.
    pub(in crate::config) fn steps(&self) -> Vec<Step> {
        Vec::from([Step::Field(String::from(self.section)), Step::Field(String::from(self.field))])
    }

    /// Whether the flag sets the field `field` of the section `section`.
    pub(super) fn sets(&self, section: &str, field: &str) -> bool {
        let same_section = self.section == section;
        let same_field = self.field == field;
        same_section && same_field
    }

    /// Whether the flag is a switch, which `--no-<name>` also turns off.
    pub(super) fn is_switch(&self) -> bool {
        self.form == Form::Switch
    }
}

/// What a long name names in a table: a flag, or a switch it turns off (ADR-0028).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Named<'table> {
    /// The flag of that name.
    Flag(&'table Flag),
    /// `no-<name>`: the switch of that name, turned off.
    Negated(&'table Flag),
}

/// A program's flags, one of each name, a switch's `--no-<name>` among them (ADR-0028).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagTable {
    pub(super) flags: Vec<Flag>,
}

impl FlagTable {
    /// Every flag, each section's in the order the sections were added, then those the program added.
    #[must_use]
    pub fn flags(&self) -> &[Flag] {
        &self.flags
    }

    /// What the long name `name` names: a flag, a switch's `no-<name>`, or nothing.
    #[must_use]
    pub fn long(&self, name: &str) -> Option<Named<'_>> {
        let found = self.flags.iter().find(|flag| flag.long == name);
        if let Some(flag) = found {
            return Some(Named::Flag(flag));
        }

        let switched = name.strip_prefix("no-")?;
        let negated = self.flags.iter().find(|flag| flag.is_switch() && flag.long == switched)?;
        Some(Named::Negated(negated))
    }

    /// The flag whose short letter is `letter`.
    #[must_use]
    pub fn short(&self, letter: char) -> Option<&Flag> {
        self.flags.iter().find(|flag| flag.short == Some(letter))
    }
}
