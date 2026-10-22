//! Why the native host's layers were refused, or the program they set up did not start or run (ADR-0028).

use aethereal_data::JsonPath;
use aethereal_data::config::{ConfigError, FlagName};
use aethereal_substrate::ProgramError;
use core::error::Error;
use core::fmt;
use std::io;
use std::path::PathBuf;

/// Why a program read by the native host did not start: each stops it as it is set up, before anything is made.
#[derive(Debug)]
pub enum NativeError {
    /// The program's name is not one its environment variables can carry: ASCII letters, digits, `-` and `_`, a
    /// letter first.
    Name {
        /// The name.
        name: String,
    },
    /// A flag of the program's takes a name the command line keeps for itself: `--set`, `--config`, `--describe`,
    /// `--help`, or `-h`.
    Reserved {
        /// The name.
        name: FlagName,
        /// The field the flag sets.
        field: JsonPath,
    },
    /// The command line was refused: a flag that does not exist, with the nearest one suggested, or a flag given no
    /// value.
    CommandLine(clap::Error),
    /// A variable with the program's prefix that names no flag, as an unknown field is refused.
    UnknownVariable {
        /// The variable.
        variable: String,
    },
    /// A variable with the program's prefix whose value is not Unicode.
    NotUnicode {
        /// The variable.
        variable: String,
    },
    /// The file `--config` names could not be read.
    File {
        /// The path given.
        path: PathBuf,
        /// Why.
        error: io::Error,
    },
    /// A layer set a field that does not exist, or a value its form does not read, naming the layer and the path.
    Configuration(ConfigError),
    /// The program was refused as it was put together, settled, or bootstrapped.
    Program(ProgramError),
}

impl From<ConfigError> for NativeError {
    fn from(error: ConfigError) -> Self {
        Self::Configuration(error)
    }
}

impl From<ProgramError> for NativeError {
    fn from(error: ProgramError) -> Self {
        Self::Program(error)
    }
}

impl fmt::Display for NativeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name { name } => write!(f, "program: {name:?} is not a name a variable can carry"),
            Self::Reserved { name, field } => {
                write!(f, "command line: {name}, the flag of {field}, is kept for itself")
            }
            Self::CommandLine(error) => write!(f, "command line: {error}"),
            Self::UnknownVariable { variable } => write!(f, "environment: {variable} names no flag"),
            Self::NotUnicode { variable } => write!(f, "environment: {variable} is not Unicode"),
            Self::File { path, error } => write!(f, "file: {}: {error}", path.display()),
            Self::Configuration(error) => write!(f, "{error}"),
            Self::Program(error) => write!(f, "{error}"),
        }
    }
}

impl Error for NativeError {}

/// Why a launched program did not run to its end: its invocation was refused, or the host, running it, answered its
/// own error, `E`, which is the headless host's [`ProgramError`] and the desktop's own.
#[derive(Debug)]
pub enum LaunchError<E> {
    /// The invocation was refused, as this says, before the program was run.
    Read(NativeError),
    /// The program was run and did not come to its end, as the host says.
    Ran(E),
}

impl<E> From<NativeError> for LaunchError<E> {
    fn from(error: NativeError) -> Self {
        Self::Read(error)
    }
}

impl<E: fmt::Display> fmt::Display for LaunchError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(error) => write!(f, "{error}"),
            Self::Ran(error) => write!(f, "{error}"),
        }
    }
}

impl<E: Error> Error for LaunchError<E> {}
