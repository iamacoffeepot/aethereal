//! The table of a program's flags, checked: one flag of each name (ADR-0028).

use crate::JsonPath;
use crate::config::document::{Step, path_of};
use crate::config::field::Within;
use crate::config::form::Form;
use alloc::string::String;
use alloc::vec::Vec;

/// One flag: its names, the field it sets, the field's doc comment, and how its value is read (ADR-0028).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flag {
    pub(super) long: String,
    /// The short flag's letter, fixed as the flag is made; `None` when the flag has no short form.
    pub(super) short: Option<char>,
    pub(super) path: FieldPath<'static>,
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

    /// The path of the field the flag sets, from the configuration's root: `engine.workers`, or `windows[0].size`.
    #[must_use]
    pub fn path(&self) -> JsonPath {
        self.path.path()
    }

    /// The steps of the path of the field the flag sets.
    pub(in crate::config) fn steps(&self) -> Vec<Step> {
        self.path.steps()
    }

    /// Whether the flag sets the field `field`, `within` the section `section`.
    pub(super) fn sets(&self, section: &str, within: Within, field: &str) -> bool {
        self.path == FieldPath::new(section, within, field)
    }

    /// Whether the flag is a switch, which `--no-<name>` also turns off.
    pub(super) fn is_switch(&self) -> bool {
        self.form == Form::Switch
    }
}

/// The field a flag sets: its section, where in the section, and its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FieldPath<'name> {
    section: &'name str,
    within: Within,
    field: &'name str,
}

impl<'name> FieldPath<'name> {
    /// The field `field`, `within` the section `section`.
    pub(super) const fn new(section: &'name str, within: Within, field: &'name str) -> Self {
        Self { section, within, field }
    }

    /// The steps from the configuration's root: the section, the element's index when it is in one, and the field.
    pub(super) fn steps(&self) -> Vec<Step> {
        let section = Step::Field(String::from(self.section));
        let field = Step::Field(String::from(self.field));
        match self.within {
            Within::Section => Vec::from([section, field]),
            Within::Element(index) => Vec::from([section, Step::Index(index), field]),
        }
    }

    /// The path from the configuration's root.
    pub(super) fn path(&self) -> JsonPath {
        path_of(&self.steps())
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
