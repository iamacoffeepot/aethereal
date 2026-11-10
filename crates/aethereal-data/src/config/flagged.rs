//! A kind whose fields a command line fills: a tool's input, as it declares its flags (ADR-0053).

use super::Declared;
use crate::Schema;

/// A struct whose named fields a command line fills, by the flags `#[flag]` marks on them: a tool's input, typed at
/// the dev overlay's console as settings are typed on a program's command line (ADR-0053).
///
/// `#[flag]` on a field of a struct that is not `#[settings]` writes this. A field is marked `#[flag]`, for
/// `--<name>` with `_` written `-`; `#[flag(long = "…", short = '…')]`, for names of its own; or
/// `#[flag(position)]`, for the next bare word, in declared order. A field's doc comment is its help.
///
/// ```
/// use aethereal_data::Plain;
/// use aethereal_data::config::{Flagged, Suggested};
///
/// #[derive(Plain)]
/// #[kind(name = "example.tools.describe")]
/// struct Describe {
///     /// The engine to describe.
///     #[flag(position)]
///     engine: u32,
///     /// Whether each tool's whole description is shown.
///     #[flag(short = 'f')]
///     full: bool,
/// }
///
/// assert_eq!(Describe::FIELDS[0].flag, Suggested::Position);
/// assert_eq!(Describe::FIELDS[1].flag, Suggested::LongAndShort { long: "full", short: 'f' });
/// assert_eq!(Describe::FIELDS[0].doc, "The engine to describe.");
/// ```
///
/// A flag filled by position has no name.
///
/// ```compile_fail
/// use aethereal_data::Plain;
///
/// #[derive(Plain)]
/// struct Describe {
///     #[flag(position, short = 'e')]
///     engine: u32,
/// }
/// ```
///
/// A flag marks a named field.
///
/// ```compile_fail
/// use aethereal_data::Plain;
///
/// #[derive(Plain)]
/// struct Describe(#[flag(position)] u32);
/// ```
pub trait Flagged: Schema {
    /// Each named field, in declared order, with its doc comment and the flag it suggests.
    const FIELDS: &'static [Declared];
}
