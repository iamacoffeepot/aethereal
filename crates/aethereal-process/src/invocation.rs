//! What a program is started with: its path, its arguments, its environment, and whether its stdin and its stdout are
//! connected to a pipe its starter reads and writes (ADR-0040).

use aethereal_data::Plain;

/// A program to start, as the root hands it to the host's launcher.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.process.invocation")]
pub struct Invocation {
    /// Where the program is on the host's machine.
    pub program: String,
    pub args: Vec<String>,
    pub environment: Environment,
    pub stdin: Stdio,
    pub stdout: Stdio,
}

/// The environment a program is started with.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.process.environment")]
pub enum Environment {
    /// The host's own, as the engine's process has it.
    Inherited,
    /// Exactly these variables: nothing of the host's is passed on.
    Only(Vec<Variable>),
}

/// One variable of an environment.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.process.variable")]
pub struct Variable {
    pub name: String,
    pub value: String,
}

/// What a program's stdin or its stdout is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.process.stdio")]
pub enum Stdio {
    /// Nothing: its stdin reads to its end at once, and what it writes to its stdout is discarded.
    Closed,
    /// The program's end of its pipe, whose other end a [`Pipe`](crate::Pipe) serves to its starter.
    Connected,
}
