//! Why a sheet's flags do not fit its input, and why a typed line is not a call of its tool (ADR-0053).

use aethereal_data::JsonError;
use core::{error::Error, fmt};

/// Why a sheet's flags do not fit its input: the holder refuses the offer, [`Offered::BadFlags`](crate::Offered).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlagsFault {
    /// The sheet has flags, and its input is not a struct of named fields.
    NotAStruct,
    /// A flag names a field the input lacks.
    NoField { field: String },
    /// Two flags fill one field.
    TwoFlags { field: String },
    /// Two flags are typed alike: `--name`, a switch's `--no-name`, or `-n`.
    SameName { name: String },
    /// A flag's name could not be typed as one: a long name that is empty or holds whitespace or `=`, or a short
    /// letter that is not an ASCII letter.
    BadName { name: String },
    /// A field filled by position is a switch, which is typed by its name alone.
    PositionalSwitch { field: String },
    /// A field filled by position must be given, after one that may be left out, so a word could fill either.
    RequiredAfterOptional { field: String },
    /// A field that must be given has no flag, so no line could give it.
    Unflagged { field: String },
}

impl fmt::Display for FlagsFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAStruct => f.write_str("flags fill named fields, and the input has none"),
            Self::NoField { field } => write!(f, "a flag fills `{field}`, which the input lacks"),
            Self::TwoFlags { field } => write!(f, "two flags fill `{field}`"),
            Self::SameName { name } => write!(f, "two flags are typed `{name}`"),
            Self::BadName { name } => write!(f, "`{name}` cannot be typed as a flag"),
            Self::PositionalSwitch { field } => {
                write!(f, "`{field}` is a switch, and a switch is not filled by position")
            }
            Self::RequiredAfterOptional { field } => {
                write!(f, "`{field}` must be given, and is filled by position after a field that may be left out")
            }
            Self::Unflagged { field } => write!(f, "`{field}` must be given, and has no flag"),
        }
    }
}

impl Error for FlagsFault {}

/// Why a typed line is not a call of its tool, as a person reads it (ADR-0053).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    /// A quote is opened and not closed.
    Unclosed { quote: char },
    /// The line ends in a backslash, which escapes nothing.
    TrailingEscape,
    /// A word names no flag of the tool.
    NoFlag { word: String },
    /// A flag's text is not of its field's form; `expects` says what is.
    Unread { flag: String, expects: String },
    /// A bare word that no field takes.
    Unwanted { word: String },
    /// A field that must be given is not.
    Missing { flag: String },
    /// A tool with no flags is given more than one word.
    NotOneWord,
    /// The one word of a tool with no flags is not JSON.
    NotJson { error: JsonError },
    /// The input read is not of the tool's input schema.
    NotOfSchema { error: JsonError },
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unclosed { quote } => write!(f, "a `{quote}` is not closed"),
            Self::TrailingEscape => f.write_str("the line ends in a `\\`"),
            Self::NoFlag { word } => write!(f, "there is no flag `{word}`"),
            Self::Unread { flag, expects } => write!(f, "`{flag}` takes {expects}"),
            Self::Unwanted { word } => write!(f, "nothing takes `{word}`"),
            Self::Missing { flag } => write!(f, "missing `{flag}`"),
            Self::NotOneWord => f.write_str("this tool takes its input as one JSON word"),
            Self::NotJson { error } => write!(f, "the input is not JSON: {error}"),
            Self::NotOfSchema { error } => write!(f, "the input is not of the tool's input: {error}"),
        }
    }
}

impl Error for CommandError {}
