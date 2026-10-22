//! A program's flags: what each section's settings kind suggests, as the program changes it (ADR-0028).

mod table;

pub use table::{Flag, FlagTable, Named};

use super::form::Form;
use super::place::Place;
use super::{Declared, Field, FlagError, FlagName, Parts, Settings, Suggested};
use crate::{JsonPath, JsonStep, KindName};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// The field a change names, in each section read as its handle's settings kind.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    /// The handle's settings kind.
    kind: KindName,
    /// The names of the sections of `parts` read as that kind, in the order they were added.
    sections: Vec<&'static str>,
    /// The field's name.
    field: &'static str,
}

impl Target {
    /// The field `field` names, in each section of `parts` read as its kind.
    fn of<S: Settings, V>(parts: &Parts, field: Field<S, V>) -> Self {
        Self { kind: S::NAME, sections: parts.read_as::<S>(), field: field.name() }
    }

    /// Whether `flag` sets the field, in one of the sections.
    fn sets(&self, flag: &Flag) -> bool {
        self.sections.iter().any(|section| flag.sets(section, self.field))
    }

    /// The field's path from the configuration's root, in the section `section`.
    fn path(&self, section: &str) -> JsonPath {
        JsonPath::of(Vec::from([JsonStep::Field(String::from(section)), JsonStep::Field(String::from(self.field))]))
    }
}

/// One change the program makes to the flags its sections suggest.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Change {
    /// The field's flag takes another long name.
    Rename { target: Target, long: String },
    /// The field has no flag.
    Hide { target: Target },
    /// The field has a flag of the program's own.
    Add { target: Target, long: String, short: char },
}

/// The flags each section's settings kind suggests for its fields, as the program renames, hides, and adds to them
/// (ADR-0028).
///
/// A handle names its field in each section read as its kind, so a change made through it applies to each.
///
/// ```
/// use aethereal_data::Plain;
/// use aethereal_data::config::{Flags, Parts};
///
/// #[derive(Plain, Default)]
/// #[kind(name = "example.engine.settings")]
/// #[settings]
/// struct EngineSettings {
///     /// The threads that run a step.
///     #[flag(long = "workers", short = 'w')]
///     workers: u32,
/// }
///
/// let parts = Parts::new().with::<EngineSettings>("engine")?;
/// let table = Flags::of(&parts).rename(EngineSettings::WORKERS, "threads").finish()?;
/// assert!(table.long("threads").is_some());
/// assert!(table.long("workers").is_none());
/// # Ok::<(), Box<dyn core::error::Error>>(())
/// ```
pub struct Flags<'parts> {
    parts: &'parts Parts,
    changes: Vec<Change>,
}

impl<'parts> Flags<'parts> {
    /// The flags the settings kinds of the sections of `parts` suggest, changed by nothing yet.
    #[must_use]
    pub const fn of(parts: &'parts Parts) -> Self {
        Self { parts, changes: Vec::new() }
    }

    /// The flag of `field` renamed `long`.
    #[must_use]
    pub fn rename<S: Settings, V>(mut self, field: Field<S, V>, long: &str) -> Self {
        let target = Target::of(self.parts, field);
        self.changes.push(Change::Rename { target, long: String::from(long) });
        self
    }

    /// `field` left with no flag.
    #[must_use]
    pub fn hide<S: Settings, V>(mut self, field: Field<S, V>) -> Self {
        let target = Target::of(self.parts, field);
        self.changes.push(Change::Hide { target });
        self
    }

    /// A flag of the program's own, `--long` and `-short`, for `field` of any section.
    #[must_use]
    pub fn flag<S: Settings, V>(mut self, long: &str, short: char, field: Field<S, V>) -> Self {
        let target = Target::of(self.parts, field);
        self.changes.push(Change::Add { target, long: String::from(long), short });
        self
    }

    /// The table, refusing a change to a field of a settings kind no section is read as, a rename or a hide of a
    /// field that has no flag, and two flags left of one name, naming both fields.
    ///
    /// # Errors
    ///
    /// The first refusal, as [`FlagError`] says.
    pub fn finish(self) -> Result<FlagTable, FlagError> {
        let schema = self.parts.schema();
        let form_of = |section: &str, field: &str| match Place::root(&schema).field(section).field(field) {
            Place::Schema(at) => Form::of(at),
            Place::Opaque | Place::Unknown => Form::Json,
        };

        let mut flags = Vec::new();
        for (section, fields) in self.parts.declared() {
            for declared in fields {
                let (long, short) = match declared.flag {
                    Suggested::NoFlag => continue,
                    Suggested::Long { long } => (long, None),
                    Suggested::LongAndShort { long, short } => (long, Some(short)),
                };
                let form = form_of(section, declared.name);
                let long = String::from(long);
                flags.push(Flag { long, short, section, field: declared.name, doc: declared.doc, form });
            }
        }

        for change in self.changes {
            match change {
                Change::Rename { target, long } => {
                    let named = Self::named(&mut flags, &target)?;
                    for flag in named {
                        flag.long.clone_from(&long);
                    }
                }
                Change::Hide { target } => {
                    Self::named(&mut flags, &target)?;
                    flags.retain(|flag| !target.sets(flag));
                }
                Change::Add { target, long, short } => {
                    let uncomposed = target.sections.is_empty();
                    if uncomposed {
                        return Err(FlagError::NoSection { kind: target.kind });
                    }
                    for section in target.sections {
                        let doc = self.parts.doc(section, target.field);
                        let form = form_of(section, target.field);
                        let long = long.clone();
                        flags.push(Flag { long, short: Some(short), section, field: target.field, doc, form });
                    }
                }
            }
        }

        refuse_collisions(&flags)?;
        Ok(FlagTable { flags })
    }

    /// The flags of `target`'s field, refusing a kind no section is read as and a field with no flag.
    fn named<'flags>(flags: &'flags mut [Flag], target: &Target) -> Result<Vec<&'flags mut Flag>, FlagError> {
        let Some(first) = target.sections.first() else {
            return Err(FlagError::NoSection { kind: target.kind.clone() });
        };
        let named: Vec<&mut Flag> = flags.iter_mut().filter(|flag| target.sets(flag)).collect();
        let none = named.is_empty();
        if none {
            return Err(FlagError::NoFlag { field: target.path(first) });
        }

        Ok(named)
    }
}

impl Parts {
    /// The doc comment of the field `field` of the section `section`, empty when it declares none.
    fn doc(&self, section: &str, field: &str) -> &'static str {
        let fields = self.declared().find(|(name, _)| *name == section).map_or(&[][..], |(_, fields)| fields);
        let declared: Option<&Declared> = fields.iter().find(|declared| declared.name == field);
        declared.map_or("", |declared| declared.doc)
    }
}

/// Refuses the first two flags of one name, a switch's `no-<name>` among the names, in the table's order.
fn refuse_collisions(flags: &[Flag]) -> Result<(), FlagError> {
    let mut names: Vec<(FlagName, &Flag)> = Vec::new();
    for flag in flags {
        names.push((FlagName::Long(flag.long.clone()), flag));
        if let Some(letter) = flag.short {
            names.push((FlagName::Short(letter), flag));
        }
        let switch = flag.is_switch();
        if switch {
            let negated = "no-".to_string() + &flag.long;
            names.push((FlagName::Long(negated), flag));
        }
    }

    for (position, (name, first)) in names.iter().enumerate() {
        let later = names.iter().skip(position + 1);
        let found = later.map(|(other, second)| (other, *second)).find(|(other, _)| *other == name);
        if let Some((_, second)) = found {
            return Err(FlagError::Collision { name: name.clone(), first: first.path(), second: second.path() });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Budget;
    use crate::config::samples::{EngineSettings, PoolSettings, RendererSettings, WindowsSettings};
    use crate::config::{Overlay, RepeatedSection};
    use alloc::boxed::Box;
    use core::error::Error;

    type Outcome = Result<(), Box<dyn Error>>;

    /// The engine's and the pool's sections, whose settings both suggest `--workers`.
    fn parts() -> Result<Parts, RepeatedSection> {
        Parts::new().with::<EngineSettings>("engine")?.with::<PoolSettings>("pool")
    }

    /// The path of the field `field` of the section `section`.
    fn path(section: &str, field: &str) -> JsonPath {
        JsonPath::of(Vec::from([JsonStep::Field(String::from(section)), JsonStep::Field(String::from(field))]))
    }

    #[test]
    fn a_collision_is_refused_naming_both_fields_and_a_rename_clears_it() -> Outcome {
        // Catches a table where one of two flags of one name silently wins, and a rename that leaves the old name.
        let parts = parts()?;

        let left = Flags::of(&parts).finish().err();
        let renamed = Flags::of(&parts).rename(PoolSettings::WORKERS, "pool-workers").finish()?;

        let name = FlagName::Long(String::from("workers"));
        let collision =
            FlagError::Collision { name, first: path("engine", "workers"), second: path("pool", "workers") };
        assert_eq!(left, Some(collision));
        let pool = renamed
            .long("pool-workers")
            .map(|named| matches!(named, Named::Flag(flag) if flag.path() == path("pool", "workers")));
        assert_eq!(pool, Some(true));
        let engine = renamed.short('w').map(|flag| (flag.path(), flag.doc()));
        assert_eq!(engine, Some((path("engine", "workers"), "The threads that run a step.")));
        Ok(())
    }

    #[test]
    fn a_switch_negation_and_a_short_letter_collide_too() -> Outcome {
        // Catches a collision check that compares only long names, missing `-w` twice and `--no-fused` beside a
        // flag of that name.
        let parts = parts()?;

        let short =
            Flags::of(&parts).hide(PoolSettings::WORKERS).flag("pool", 'w', PoolSettings::WORKERS).finish().err();
        let negated =
            Flags::of(&parts).hide(PoolSettings::WORKERS).flag("no-fused", 'n', PoolSettings::WORKERS).finish().err();

        let letter = FlagError::Collision {
            name: FlagName::Short('w'),
            first: path("engine", "workers"),
            second: path("pool", "workers"),
        };
        let no_fused = FlagName::Long(String::from("no-fused"));
        let switch =
            FlagError::Collision { name: no_fused, first: path("engine", "fused"), second: path("pool", "workers") };
        assert_eq!(short, Some(letter));
        assert_eq!(negated, Some(switch));
        Ok(())
    }

    #[test]
    fn a_change_names_a_flag_that_exists() -> Outcome {
        // Catches a rename or hide of a field with no flag, or of a section not composed, that is quietly ignored.
        let parts = parts()?;

        let no_flag = Flags::of(&parts).hide(EngineSettings::MAIL_BYTES).finish().err();
        let no_section = Flags::of(&parts).rename(RendererSettings::PRESENTATION, "present").finish().err();

        assert_eq!(no_flag, Some(FlagError::NoFlag { field: path("engine", "mail_bytes") }));
        let renderer = <RendererSettings as crate::Kind>::NAME;
        assert_eq!(no_section, Some(FlagError::NoSection { kind: renderer }));
        Ok(())
    }

    #[test]
    fn no_turns_a_switch_off() -> Outcome {
        // Catches `--no-<name>` that names nothing, or sets the switch on.
        let parts = Parts::new().with::<EngineSettings>("engine")?.with::<WindowsSettings>("windows")?;
        let table = Flags::of(&parts).finish()?;
        let Some(Named::Negated(fused)) = table.long("no-fused") else {
            return Err("no-fused names the switch, negated".into());
        };
        let mut command_line = Overlay::new("command line");
        command_line.switch(fused, false)?;
        let mut budget = Budget::new(1 << 20);

        let mut configuration = parts.configure(&[command_line], &mut budget)?;

        let fused = configuration.take::<EngineSettings>("engine")?.fused;
        assert!(!fused);
        assert_eq!(table.long("no-workers"), None);
        Ok(())
    }

    #[test]
    fn a_handle_names_its_field_in_each_section_of_its_kind() -> Outcome {
        // Catches a change through a handle that reaches only the first section read as its kind, leaving the other
        // section's flag in place.
        let parts = parts()?.with::<PoolSettings>("spare")?;

        let left = Flags::of(&parts).finish().err();
        let hidden = Flags::of(&parts).hide(PoolSettings::WORKERS).finish()?;

        let name = FlagName::Long(String::from("workers"));
        let collision =
            FlagError::Collision { name, first: path("engine", "workers"), second: path("pool", "workers") };
        assert_eq!(left, Some(collision));
        let workers = hidden
            .long("workers")
            .map(|named| matches!(named, Named::Flag(flag) if flag.path() == path("engine", "workers")));
        assert_eq!(workers, Some(true));
        assert_eq!(hidden.flags().len(), 2);
        Ok(())
    }
}
