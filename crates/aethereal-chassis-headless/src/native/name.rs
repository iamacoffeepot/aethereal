//! A program's name, and the prefix its environment variables carry (ADR-0028).

use super::NativeError;
use aethereal_data::config::Flag;
use std::env;
use std::ffi::{OsStr, OsString};
use std::process::Command;

/// A program's name, as its command line shows it, and the prefix of its environment variables: the name in
/// capitals, with `-` written `_`. A program named `my-game` reads `MY_GAME_WORKERS` for `--workers` (ADR-0028).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramName {
    name: String,
    prefix: String,
}

impl ProgramName {
    /// The program named `name`: ASCII letters, digits, `-` and `_`, a letter first, so its prefix is a variable's
    /// name on every shell.
    ///
    /// # Errors
    ///
    /// [`NativeError::Name`] for any other name.
    pub fn new(name: &str) -> Result<Self, NativeError> {
        let lettered = name.starts_with(|first: char| first.is_ascii_alphabetic());
        let worded = name.chars().all(|character| character.is_ascii_alphanumeric() || "-_".contains(character));
        let valid = lettered && worded;
        if !valid {
            return Err(NativeError::Name { name: String::from(name) });
        }

        Ok(Self { name: String::from(name), prefix: name.to_ascii_uppercase().replace('-', "_") })
    }

    /// The program's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The prefix of its environment variables, without the `_` that joins it to a flag's name: `MY_GAME`.
    #[must_use]
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// The environment variable that reads `flag`: `MY_GAME_WORKERS` for `--workers`.
    #[must_use]
    pub fn variable(&self, flag: &Flag) -> String {
        let long = flag.long().to_ascii_uppercase().replace('-', "_");
        format!("{}_{long}", self.prefix)
    }

    /// Whether the variable `variable` carries the program's prefix.
    pub(super) fn carries(&self, variable: &str) -> bool {
        variable.strip_prefix(&self.prefix).is_some_and(|rest| rest.starts_with('_'))
    }

    /// A command to start `program` with this program's environment, but for the variables its prefix carries: a
    /// process the host starts is not passed the program's configuration, which is given on its command line
    /// instead (ADR-0028).
    #[must_use]
    pub fn command(&self, program: impl AsRef<OsStr>) -> Command {
        let mut command = Command::new(program);
        self.withhold(&mut command, env::vars_os());
        command
    }

    /// Removes from `command`'s environment each of `variables` the prefix carries.
    pub(super) fn withhold(&self, command: &mut Command, variables: impl IntoIterator<Item = (OsString, OsString)>) {
        for (variable, _) in variables {
            let carried = self.carries(&variable.to_string_lossy());
            if carried {
                command.env_remove(variable);
            }
        }
    }
}
