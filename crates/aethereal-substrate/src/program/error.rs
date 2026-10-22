//! Why a program did not start: a refusal as it is put together, a setting, or a root's bootstrap (ADR-0029).

use crate::CodeRefused;
use aethereal_actor::{Refused, Unordered};
use aethereal_data::KindName;
use aethereal_data::config::{ConfigError, FlagError};
use core::error::Error;
use core::fmt;

/// Why a root's bootstrap failed, which stops the program as it is set up (ADR-0029).
#[derive(Debug)]
pub enum Unbuilt {
    /// The root was refused as it began.
    Refused(Refused),
    /// Code it put was refused.
    Code(CodeRefused),
    /// Something the host opens did not open: a window, a GPU, a canvas.
    Unopened(Box<dyn Error + Send + Sync>),
    /// The bootstrap's own failure, such as a module file that could not be read.
    Failed(Box<dyn Error + Send + Sync>),
}

impl From<Refused> for Unbuilt {
    fn from(refused: Refused) -> Self {
        Self::Refused(refused)
    }
}

impl From<CodeRefused> for Unbuilt {
    fn from(refused: CodeRefused) -> Self {
        Self::Code(refused)
    }
}

impl fmt::Display for Unbuilt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(refused) => write!(f, "refused as it began: {refused}"),
            Self::Code(refused) => write!(f, "its code was refused: {refused:?}"),
            Self::Unopened(error) => write!(f, "what the host opens did not open: {error}"),
            Self::Failed(error) => write!(f, "{error}"),
        }
    }
}

impl Error for Unbuilt {}

/// What reads a section of the configuration: the host, or a root (ADR-0029).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reader {
    /// The host: the engine's section and its own.
    Host,
    /// The root of this kind.
    Root(KindName),
}

impl fmt::Display for Reader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Host => f.write_str("the host"),
            Self::Root(name) => write!(f, "{name}"),
        }
    }
}

/// Why a program did not start (ADR-0029): each is found before the program runs, and the engine and any roots
/// begun before it are dropped.
#[derive(Debug)]
pub enum ProgramError {
    /// Two sections of one name, refused before anything is made.
    Repeated {
        /// The name.
        name: &'static str,
        /// What reads each section given it, in the order they were listed: the first two.
        readers: Vec<Reader>,
    },
    /// A layer set what does not exist or the wrong shape, or a section's value broke its type's rule, found before
    /// anything is made (ADR-0028).
    Configuration(ConfigError),
    /// The program's flags were refused as it was put together: a change to a flag that does not exist, or two flags
    /// left of one name (ADR-0028).
    Flags(FlagError),
    /// The engine's cycle was refused as it was made.
    Cycle(Unordered),
    /// A root's bootstrap failed.
    Root {
        /// The root whose bootstrap failed.
        root: KindName,
        /// Why.
        why: Unbuilt,
    },
}

impl From<ConfigError> for ProgramError {
    fn from(error: ConfigError) -> Self {
        Self::Configuration(error)
    }
}

impl From<FlagError> for ProgramError {
    fn from(error: FlagError) -> Self {
        Self::Flags(error)
    }
}

impl fmt::Display for ProgramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repeated { name, readers } => {
                write!(f, "program: section {name} is read by")?;
                for reader in readers {
                    write!(f, " {reader}")?;
                }
                Ok(())
            }
            Self::Configuration(error) => write!(f, "program: {error}"),
            Self::Flags(error) => write!(f, "program: {error}"),
            Self::Cycle(unordered) => write!(f, "program: the cycle was refused: {unordered:?}"),
            Self::Root { root, why } => write!(f, "program: {root} did not bootstrap: {why}"),
        }
    }
}

impl Error for ProgramError {}
