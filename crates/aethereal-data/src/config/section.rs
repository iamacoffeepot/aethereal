//! A section: a config kind under the short name its part declares (ADR-0028).

use crate::{Bare, Decode, Encode, Kind};

/// A config kind under a short name, one section of a program's configuration (ADR-0028).
///
/// `#[section(name = "…")]` on a struct that derives `Plain` or `Bound` writes this, and a handle for each named
/// field. A section's defaults are its `Default`. It is plain data, crossing `Bare`: only what a person could type,
/// never a value the host makes.
///
/// ```
/// use aethereal_data::Plain;
/// use aethereal_data::config::Section;
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
/// assert_eq!(EngineConfig::SECTION, "engine");
/// assert_eq!(EngineConfig::WORKERS.name(), "workers");
/// ```
///
/// A section is a kind, so `#[section]` without `#[kind]` stops the build.
///
/// ```compile_fail
/// use aethereal_data::Plain;
///
/// #[derive(Plain, Default)]
/// #[section(name = "engine")]
/// struct EngineConfig {
///     workers: u32,
/// }
/// ```
///
/// A flag is suggested only for a field of a section.
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
pub trait Section: Kind + Encode<Bare> + Decode<Bare> + Default + 'static {
    /// The section's name in the configuration: `engine`, `windows`.
    const SECTION: &'static str;
    /// Each named field, in declared order, with its doc comment and the flag its part suggests.
    const FIELDS: &'static [Declared];
}

/// One named field of a section as its part declares it (ADR-0028).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Declared {
    /// The field's name.
    pub name: &'static str,
    /// The field's doc comment, its lines joined by newlines.
    pub doc: &'static str,
    /// The flag the part suggests for it.
    pub flag: Suggested,
}

/// The flag a part suggests for a field, written `#[flag(long = "…", short = '…')]` beside it (ADR-0028).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suggested {
    /// The part suggests no flag.
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
