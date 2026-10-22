//! The native host's layers: the command line, the environment, and a file, read into a program (ADR-0028).
//!
//! A program's flags are its sections' suggestions as it changed them ([`Program::flags`]). The command line reads
//! them with `clap`, with `--set path=value` for any field, `--config path` for a JSON file, `--help` from each
//! field's doc comment, and `--describe` for the configuration's schema and defaults. The environment reads each
//! flag under the program's prefix. The layers go beneath what the program sets in code, the first winning: set in
//! code, the command line, the environment, the file, the defaults.
//!
//! This is the native host's alone, so the desktop host, which builds on this crate, reads them the same way. A
//! page has no command line or environment; the browser reads its own layer.

mod command_line;
mod environment;
mod error;
mod name;
#[cfg(test)]
mod tests;

pub use error::NativeError;
pub use name::ProgramName;

use aethereal_actor::Ordered;
use aethereal_data::Budget;
use aethereal_substrate::{Bootstraps, Host, Program};
use command_line::Asked;
use std::env;
use std::ffi::OsString;

/// The name of the command line's layer.
pub const COMMAND_LINE: &str = "command line";

/// The name of the environment's layer.
pub const ENVIRONMENT: &str = "environment";

/// The name of the layer of the file `--config` names.
pub const FILE: &str = "file";

/// What describing the configuration may hold at once, in bytes.
const DESCRIBE_BYTES: usize = 1 << 24;

/// What a program is invoked with: its arguments, its own path first, and its environment's variables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    args: Vec<OsString>,
    variables: Vec<(OsString, OsString)>,
}

impl Invocation {
    /// The process's own arguments and environment.
    #[must_use]
    pub fn of_process() -> Self {
        Self { args: env::args_os().collect(), variables: env::vars_os().collect() }
    }

    /// The arguments `args`, the program's own path first, and the variables `variables`, each a name and a value.
    pub fn new<A, N, V>(args: impl IntoIterator<Item = A>, variables: impl IntoIterator<Item = (N, V)>) -> Self
    where
        A: Into<OsString>,
        N: Into<OsString>,
        V: Into<OsString>,
    {
        let args = args.into_iter().map(Into::into).collect();
        let variables = variables.into_iter().map(|(name, value)| (name.into(), value.into())).collect();

        Self { args, variables }
    }
}

/// What an invocation asks of a program.
pub enum Invoked<P> {
    /// The program, with the layers the invocation set beneath what it sets in code, to run or build.
    Run(P),
    /// The help: the flags, each with its field's doc comment and its variable.
    Help(String),
    /// The configuration's schema and defaults, as JSON.
    Describe(String),
}

/// What launching a program came to.
#[derive(Debug)]
pub enum Launched<E> {
    /// It ran to its end.
    Ran(E),
    /// It was asked for its help or its description, which was printed, and ran nothing.
    Printed,
}

/// A program read by the native host: its command line, its environment, and a file (ADR-0028).
///
/// ```
/// use aethereal_chassis_headless::native::{Invocation, Invoked, NativeLayers};
/// use aethereal_chassis_headless::Headless;
/// use aethereal_substrate::{EngineSettings, Host};
///
/// let invocation = Invocation::new(["bench", "--workers", "2"], [("BENCH_HOPS", "3")]);
/// let program = Headless::program().set_engine(EngineSettings::PARTS.to(4_u32.try_into()?));
/// let Invoked::Run(program) = program.read("bench", invocation)? else {
///     return Err("the invocation asks for the program to run".into());
/// };
/// let headless = program.build()?;
/// assert!(headless.engine().is_idle());
/// # Ok::<(), Box<dyn core::error::Error>>(())
/// ```
pub trait NativeLayers<H: Host, R>: Sized {
    /// What `invocation` asks of the program named `name`: to run, with the command line's, the environment's, and
    /// the file's layers beneath those it has, or its help or description.
    ///
    /// # Errors
    ///
    /// The first refusal, as [`NativeError`] says: each names its layer, and a setting's names its path.
    fn read<I>(self, name: &str, invocation: Invocation) -> Result<Invoked<Self>, NativeError>
    where
        R: Bootstraps<H, I>;

    /// Reads the process's own invocation, as [`read`](Self::read) does, then runs the program, or prints what it
    /// asked for.
    ///
    /// # Errors
    ///
    /// The first refusal, as [`NativeError`] says.
    fn launch<I, O>(self, name: &str) -> Result<Launched<H::Exit>, NativeError>
    where
        R: Bootstraps<H, I> + Ordered<O>;
}

impl<H: Host, R> NativeLayers<H, R> for Program<H, R> {
    fn read<I>(self, name: &str, invocation: Invocation) -> Result<Invoked<Self>, NativeError>
    where
        R: Bootstraps<H, I>,
    {
        let name = ProgramName::new(name)?;
        let parts = self.parts()?;
        let table = self.flag_table()?;

        let asked = command_line::read(&name, &table, &parts, &invocation.args)?;
        let (command_line, file) = match asked {
            Asked::Help(text) => return Ok(Invoked::Help(text)),
            Asked::Describe => return Ok(Invoked::Describe(parts.describe(&mut Budget::new(DESCRIBE_BYTES))?)),
            Asked::Run { command_line, file } => (command_line, file),
        };
        let environment = environment::read(&name, &table, &invocation.variables)?;

        Ok(Invoked::Run(self.layer(command_line).layer(environment).layer(file)))
    }

    #[allow(clippy::print_stdout)] // What a person asked of the command line is printed for them to read.
    fn launch<I, O>(self, name: &str) -> Result<Launched<H::Exit>, NativeError>
    where
        R: Bootstraps<H, I> + Ordered<O>,
    {
        let launched = match self.read(name, Invocation::of_process())? {
            Invoked::Run(program) => Launched::Ran(program.run()?),
            Invoked::Help(text) | Invoked::Describe(text) => {
                println!("{text}");
                Launched::Printed
            }
        };

        Ok(launched)
    }
}
