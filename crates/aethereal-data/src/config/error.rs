//! The refusals of a configuration as the program is put together and set up (ADR-0028).

use crate::{DecodeError, JsonFault, JsonPath, KindName};
use alloc::string::String;
use core::{error::Error, fmt};

/// Why a configuration is refused as the program is set up: it stops the program (ADR-0028).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// A layer set a field that does not exist, a value of the wrong shape, or a value its form does not read.
    Layer {
        /// The name of the layer that set it: `defaults`, `command line`.
        layer: String,
        /// The path of what it set, from the configuration's root.
        path: JsonPath,
        /// What was wrong there.
        fault: ConfigFault,
    },
    /// A section's value breaks a rule its type adds beyond its shape (ADR-0007), such as a `NonZero` set to zero.
    Section {
        /// The section's name.
        section: &'static str,
        /// The refusal of its bytes as its type.
        error: DecodeError,
    },
    /// No section of this name, read as this kind, is left to take: none was composed, or it was taken already.
    Untaken {
        /// The section's name.
        section: &'static str,
        /// The kind it would be read as.
        kind: KindName,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Layer { layer, path, fault } => {
                write!(f, "configuration: {path}, set by {layer}: {fault}")
            }
            Self::Section { section, error } => {
                write!(f, "configuration: section {section}: {error}")
            }
            Self::Untaken { section, kind } => {
                write!(f, "configuration: no section {section} read as {kind} is left to take")
            }
        }
    }
}

impl Error for ConfigError {}

/// What was wrong at a path a layer set (ADR-0028).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFault {
    /// The one decode refused it, or JSON text given for a structure is not JSON (ADR-0007).
    Json(JsonFault),
    /// An index past the end of the sequence it is in, which has `length` elements.
    NoElement {
        /// How many elements the sequence has.
        length: usize,
    },
    /// An index into a value that is not a sequence.
    NotASequence,
    /// The path and the value together nest deeper than any schema a walk follows.
    TooDeep,
    /// `--set` text whose path is not names and indices, `renderer.presentation`, `windows[0].size`.
    NotAPath,
    /// `--set` text with no `=` between its path and its value.
    NoValue,
    /// Text that is not a number, where the field holds one.
    NotANumber,
    /// Text that is not two numbers joined by `x`, `1280x720`, where the field holds a size.
    NotASize,
    /// Text that is not `true` or `false`, where the field holds a `bool`.
    NotABoolean,
    /// Text that names no variant in kebab case, where the field holds an enum.
    NotAVariant,
}

impl fmt::Display for ConfigFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(inner) => write!(f, "{inner}"),
            Self::NoElement { length } => write!(f, "no such element: the sequence has {length}"),
            Self::NotASequence => f.write_str("an index into a value that is not a sequence"),
            Self::TooDeep => f.write_str("nested too deeply"),
            Self::NotAPath => f.write_str("not a path"),
            Self::NoValue => f.write_str("no `=` before a value"),
            Self::NotANumber => f.write_str("not a number"),
            Self::NotASize => f.write_str("not a size such as 1280x720"),
            Self::NotABoolean => f.write_str("not true or false"),
            Self::NotAVariant => f.write_str("names no variant"),
        }
    }
}

/// Two sections of one name, refused as the program is put together (ADR-0028).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepeatedSection {
    /// The name both are given.
    pub name: &'static str,
    /// The kind of the section already composed under it.
    pub first: KindName,
    /// The kind of the section also given it.
    pub second: KindName,
}

impl fmt::Display for RepeatedSection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { name, first, second } = self;
        write!(f, "configuration: section {name} is given to both {first} and {second}")
    }
}

impl Error for RepeatedSection {}

/// Why a program's flags are refused as it is put together (ADR-0028).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlagError {
    /// Two flags of one name, left so by the program. Neither wins.
    Collision {
        /// The name both flags have.
        name: FlagName,
        /// The field of the first flag of that name.
        first: JsonPath,
        /// The field of the second.
        second: JsonPath,
    },
    /// A handle names a field of a settings kind no section of the program is read as.
    NoSection {
        /// The settings kind.
        kind: KindName,
    },
    /// A rename or a hide of a field that has no flag.
    NoFlag {
        /// The field.
        field: JsonPath,
    },
}

impl fmt::Display for FlagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Collision { name, first, second } => {
                write!(f, "flags: {name} is the flag of both {first} and {second}")
            }
            Self::NoSection { kind } => write!(f, "flags: no section read as {kind} is composed"),
            Self::NoFlag { field } => write!(f, "flags: {field} has no flag"),
        }
    }
}

impl Error for FlagError {}

/// A flag's name as typed: `--workers`, or `-w`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlagName {
    /// A long flag's name, without the dashes.
    Long(String),
    /// A short flag's letter.
    Short(char),
}

impl fmt::Display for FlagName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Long(name) => write!(f, "--{name}"),
            Self::Short(letter) => write!(f, "-{letter}"),
        }
    }
}
