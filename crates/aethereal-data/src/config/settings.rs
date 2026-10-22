//! A settings kind: what a root reads its section of the configuration as (ADR-0028, ADR-0029).

use crate::{Bare, Decode, Encode, Kind};

/// A config kind a section of a program's configuration is read as (ADR-0028).
///
/// `#[settings]` on a struct that derives `Plain` or `Bound` writes this, and a handle for each named field. Its
/// defaults are its `Default`. It is plain data, crossing `Bare`: only what a person could type, never a value the
/// host makes. It carries no name: the section's name is given where the section is added, by the root that reads
/// it (ADR-0029).
///
/// ```
/// use aethereal_data::Plain;
/// use aethereal_data::config::Settings;
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
/// assert_eq!(EngineSettings::WORKERS.name(), "workers");
/// assert_eq!(EngineSettings::FIELDS[0].doc, "The threads that run a step.");
/// ```
///
/// Settings are a kind, so `#[settings]` without `#[kind]` stops the build.
///
/// ```compile_fail
/// use aethereal_data::Plain;
///
/// #[derive(Plain, Default)]
/// #[settings]
/// struct EngineSettings {
///     workers: u32,
/// }
/// ```
///
/// The mark carries no name.
///
/// ```compile_fail
/// use aethereal_data::Plain;
///
/// #[derive(Plain, Default)]
/// #[kind(name = "example.engine.settings")]
/// #[settings(name = "engine")]
/// struct EngineSettings {
///     workers: u32,
/// }
/// ```
///
/// A flag is suggested only for a field of a settings kind.
///
/// ```compile_fail
/// use aethereal_data::Plain;
///
/// #[derive(Plain, Default)]
/// struct Loose {
///     #[flag(long = "workers")]
///     workers: u32,
/// }
/// ```
pub trait Settings: Kind + Encode<Bare> + Decode<Bare> + Default + 'static {
    /// Each named field, in declared order, with its doc comment and the flag it suggests.
    const FIELDS: &'static [Declared];
}

/// One named field of a settings kind as it declares it (ADR-0028).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Declared {
    /// The field's name.
    pub name: &'static str,
    /// The field's doc comment, its lines joined by newlines.
    pub doc: &'static str,
    /// The flag it suggests for the field.
    pub flag: Suggested,
}

/// The flag a settings kind suggests for a field, written `#[flag(long = "…", short = '…')]` beside it (ADR-0028).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suggested {
    /// No flag is suggested.
    NoFlag,
    /// A long flag, `--workers`.
    Long {
        /// The flag's name, without the dashes.
        long: &'static str,
    },
    /// A long flag and a short one, `--workers` and `-w`.
    LongAndShort {
        /// The long flag's name, without the dashes.
        long: &'static str,
        /// The short flag's letter.
        short: char,
    },
}
