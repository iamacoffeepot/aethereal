//! A program's flags: what each part suggests, as the program changes it (ADR-0028).

mod table;

pub use table::{Flag, FlagTable, Named};

use super::form::Form;
use super::place::Place;
use super::{Declared, Field, FlagError, FlagName, Parts, Section, Suggested};
use crate::{JsonPath, JsonStep};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// The field a change names: its section and its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Target {
    /// The section's name.
    section: &'static str,
    /// Whether the composite's section of that name is the handle's type.
    composed: bool,
    /// The field's name.
    field: &'static str,
}

impl Target {
    /// The field `field` names, and whether `parts` holds its section.
    fn of<S: Section, V>(parts: &Parts, field: Field<S, V>) -> Self {
        let section = S::SECTION;
        Self { section, composed: parts.holds::<S>(section), field: field.name() }
    }

    /// The field's path from the configuration's root.
    fn path(self) -> JsonPath {
        JsonPath::of(Vec::from([
            JsonStep::Field(String::from(self.section)),
            JsonStep::Field(String::from(self.field)),
        ]))
    }
}

/// One change the program makes to the flags its parts suggest.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Change {
    /// The field's flag takes another long name.
    Rename { target: Target, long: String },
    /// The field has no flag.
    Hide { target: Target },
    /// The field has a flag of the program's own.
    Add { target: Target, long: String, short: char },
}

/// The flags each part suggests for its fields, as the program renames, hides, and adds to them (ADR-0028).
///
/// ```
/// use aethereal_data::config::{Flags, Parts};
/// use aethereal_data::{Budget, Plain};
///
/// #[derive(Plain, Default)]
/// #[kind(name = "example.engine.config")]
/// #[section(name = "engine")]
/// struct EngineConfig {
///     /// The threads that run a step.
///     #[flag(long = "workers", short = 'w')]
///     workers: u32,
/// }
///
/// let parts = Parts::new().with::<EngineConfig>()?;
/// let table = Flags::of(&parts).rename(EngineConfig::WORKERS, "threads").finish()?;
/// assert!(table.long("threads").is_some());
/// assert!(table.long("workers").is_none());
/// # Ok::<(), Box<dyn core::error::Error>>(())
/// ```
pub struct Flags<'parts> {
    parts: &'parts Parts,
    changes: Vec<Change>,
}

impl<'parts> Flags<'parts> {
    /// The flags the sections of `parts` suggest, changed by nothing yet.
    #[must_use]
    pub const fn of(parts: &'parts Parts) -> Self {
        Self { parts, changes: Vec::new() }
    }

    /// The flag of `field` renamed `long`.
    #[must_use]
    pub fn rename<S: Section, V>(mut self, field: Field<S, V>, long: &str) -> Self {
        let target = Target::of(self.parts, field);
        self.changes.push(Change::Rename { target, long: String::from(long) });
        self
    }

    /// `field` left with no flag.
    #[must_use]
    pub fn hide<S: Section, V>(mut self, field: Field<S, V>) -> Self {
        let target = Target::of(self.parts, field);
        self.changes.push(Change::Hide { target });
        self
    }

    /// A flag of the program's own, `--long` and `-short`, for `field` of any part.
    #[must_use]
    pub fn flag<S: Section, V>(mut self, long: &str, short: char, field: Field<S, V>) -> Self {
        let target = Target::of(self.parts, field);
        self.changes.push(Change::Add { target, long: String::from(long), short });
        self
    }

    /// The table, refusing a change to a field of a section the program does not compose, a rename or a hide of a
    /// field that has no flag, and two flags left of one name, naming both fields.
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
                    let named = Self::named(&mut flags, target)?;
                    for flag in named {
                        flag.long.clone_from(&long);
                    }
                }
                Change::Hide { target } => {
                    Self::named(&mut flags, target)?;
                    flags.retain(|flag| !flag.sets(target.section, target.field));
                }
                Change::Add { target, long, short } => {
                    if !target.composed {
                        return Err(FlagError::NoSection { section: target.section });
                    }
                    let doc = self.parts.doc(target.section, target.field);
                    let form = form_of(target.section, target.field);
                    let field = target.field;
                    let section = target.section;
                    flags.push(Flag { long, short: Some(short), section, field, doc, form });
                }
            }
        }

        refuse_collisions(&flags)?;
        Ok(FlagTable { flags })
    }

    /// The flags of `target`'s field, refusing a section the program does not compose and a field with no flag.
    fn named(flags: &mut [Flag], target: Target) -> Result<Vec<&mut Flag>, FlagError> {
        if !target.composed {
            return Err(FlagError::NoSection { section: target.section });
        }
        let named: Vec<&mut Flag> = flags.iter_mut().filter(|flag| flag.sets(target.section, target.field)).collect();
        let none = named.is_empty();
        if none {
            return Err(FlagError::NoFlag { field: target.path() });
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
    use crate::config::samples::{EngineConfig, PoolConfig, RendererConfig, WindowsConfig};
    use crate::config::{Overlay, RepeatedSection};
    use alloc::boxed::Box;
    use core::error::Error;

    type Outcome = Result<(), Box<dyn Error>>;

    /// The engine's and the pool's sections, whose parts both suggest `--workers`.
    fn parts() -> Result<Parts, RepeatedSection> {
        Parts::new().with::<EngineConfig>()?.with::<PoolConfig>()
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
        let renamed = Flags::of(&parts).rename(PoolConfig::WORKERS, "pool-workers").finish()?;

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

        let short = Flags::of(&parts).hide(PoolConfig::WORKERS).flag("pool", 'w', PoolConfig::WORKERS).finish().err();
        let negated =
            Flags::of(&parts).hide(PoolConfig::WORKERS).flag("no-fused", 'n', PoolConfig::WORKERS).finish().err();

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

        let no_flag = Flags::of(&parts).hide(EngineConfig::MAIL_BYTES).finish().err();
        let no_section = Flags::of(&parts).rename(RendererConfig::PRESENTATION, "present").finish().err();

        assert_eq!(no_flag, Some(FlagError::NoFlag { field: path("engine", "mail_bytes") }));
        assert_eq!(no_section, Some(FlagError::NoSection { section: "renderer" }));
        Ok(())
    }

    #[test]
    fn no_turns_a_switch_off() -> Outcome {
        // Catches `--no-<name>` that names nothing, or sets the switch on.
        let parts = Parts::new().with::<EngineConfig>()?.with::<WindowsConfig>()?;
        let table = Flags::of(&parts).finish()?;
        let Some(Named::Negated(fused)) = table.long("no-fused") else {
            return Err("no-fused names the switch, negated".into());
        };
        let mut command_line = Overlay::new("command line");
        command_line.switch(fused, false)?;
        let mut budget = Budget::new(1 << 20);

        let configuration = parts.configure(&[command_line], &mut budget)?;

        let fused = configuration.get::<EngineConfig>().map(|engine| engine.fused);
        assert_eq!(fused, Some(false));
        assert_eq!(table.long("no-workers"), None);
        Ok(())
    }
}
