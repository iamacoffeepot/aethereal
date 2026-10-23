//! Why a command stopped, and the code it exits with.

use std::fmt::Display;
use std::io;
use std::process::ExitCode;

/// A command that stopped short: what is printed for it, and the code the process exits with.
pub struct Failure {
    code: u8,
    message: String,
}

impl Failure {
    /// A command line that asks for nothing this tool does, answered with how to ask: exits 2.
    pub fn usage(message: impl Display) -> Self {
        Self { code: 2, message: message.to_string() }
    }

    /// A step that failed: exits 1.
    pub fn new(message: impl Display) -> Self {
        Self { code: 1, message: message.to_string() }
    }

    /// Prints why the command stopped, and hands back the code to exit with.
    pub fn report(self) -> ExitCode {
        eprintln!("{}", self.message);
        ExitCode::from(self.code)
    }
}

impl From<io::Error> for Failure {
    fn from(error: io::Error) -> Self {
        Self::new(error)
    }
}
