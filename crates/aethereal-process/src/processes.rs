//! The process capability's root (ADR-0040).

use crate::jobs::{Launch, Launched, Launching, Raise, Raised, Raising};
use crate::{Environment, Exits, Invocation, Pipe, PipeConfig, Process, ProcessConfig, Signal, Stdio};
use aethereal_actor::{ActorRef, Answered, Fault, Held, Later, Level, ProtocolRef, Spawn, Turn, actor};
use aethereal_data::{Bound, Plain};
use aethereal_tcp::Shut;
use std::collections::BTreeMap;
use std::fmt::Debug;

/// What an actor asks the root to start: a program, a path on the host's machine, its arguments, its environment,
/// whether its stdin and its stdout are connected to a pipe, and the actor told how it exited, usually the asker itself
/// (`turn.me().protocol()`), whose ending stops it and hangs its pipe up.
#[derive(Debug, Bound)]
#[kind(name = "aether.process.start")]
pub struct Start {
    pub program: String,
    pub args: Vec<String>,
    pub environment: Environment,
    pub stdin: Stdio,
    pub stdout: Stdio,
    pub starter: ProtocolRef<Exits>,
}

/// What a start came to.
#[derive(Debug, Bound)]
#[kind(name = "aether.process.started")]
pub enum Started {
    /// Running: the process beneath the root that owns it, and its pipe.
    Running { process: ActorRef<Process>, pipe: Piped },
    /// The host would not start it: nothing runnable is at its path, or the host starts no programs.
    Refused,
}

/// A running program's pipe, as its start answers it.
#[derive(Debug, Bound)]
#[kind(name = "aether.process.piped")]
pub enum Piped {
    /// Its start connected neither its stdin nor its stdout.
    Closed,
    /// The pipe beneath the root that serves the program's stdin and stdout, as many of them as its start connected.
    Connected(ActorRef<Pipe>),
}

/// What the root is made from: how long a program asked to stop is given (ADR-0040).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.process.config")]
pub struct ProcessesConfig {
    pub kill_after_millis: u32,
}

/// A start the root holds until its launch is answered, and the actor its process is to tell.
#[derive(Bound)]
struct Waiting {
    held: Held<Started>,
    starter: ProtocolRef<Exits>,
}

/// The process capability: starts programs for every other actor (ADR-0040). An actor asks it a [`Start`] at its
/// typed path, `path::<Processes>(())`.
///
/// It starts any program it is asked to: what a program lets be started is the rule of whoever asks. Each program
/// running is a [`Process`] beneath it, keyed by its count, which owns the program from then on.
#[derive(Bound)]
#[kind(name = "aether.process")]
pub struct Processes {
    config: ProcessesConfig,
    /// Each start whose launch runs, by the number of the process it is for.
    waiting: BTreeMap<u64, Waiting>,
    next: u64,
}

#[actor(spawns(Pipe, Process))]
impl Processes {
    fn begin(config: ProcessesConfig) -> Result<Self, Fault> {
        Ok(Self { config, waiting: BTreeMap::new(), next: 0 })
    }

    #[request]
    fn start(&mut self, start: Start, turn: &mut Turn<'_, Self>) -> Result<Later<Started>, Fault> {
        let (held, later) = turn.hold();
        let process = self.next;
        self.next += 1;
        let invocation = Invocation {
            program: start.program,
            args: start.args,
            environment: start.environment,
            stdin: start.stdin,
            stdout: start.stdout,
        };
        turn.run(&Launch { process, invocation }, &Launching { process })?;
        self.waiting.insert(process, Waiting { held, starter: start.starter });

        Ok(later)
    }

    /// Stands the pipe up over the launcher's end of a program's pipe, then the process over the program, and answers
    /// its start.
    #[answer]
    fn launched(
        &mut self,
        (answered, Launching { process }): (Answered<Launched>, Launching),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Some(Waiting { held, starter }) = self.waiting.remove(&process) else {
            return Ok(());
        };
        let started = match answered {
            Answered::Yes(Launched::Started) => self.stand_up(turn, process, starter, Piped::Closed)?,
            Answered::Yes(Launched::Piped { socket }) => {
                let socket = turn.take(socket)?;
                let config = PipeConfig { socket: turn.give(socket)?, starter };
                match turn.spawn::<Pipe>(&process, config) {
                    Ok(pipe) => self.stand_up(turn, process, starter, Piped::Connected(pipe))?,
                    Err(refused) => {
                        abandon(turn, process, "pipe", &refused)?;
                        Started::Refused
                    }
                }
            }
            Answered::Yes(Launched::Refused) | Answered::No(_) => Started::Refused,
        };
        turn.answer(held, &started)?;

        Ok(())
    }

    /// The program of a process not stood up was sent its kill: its exit is the launcher's to keep.
    #[answer]
    fn raised(&mut self, _: (Answered<Raised>, Raising)) -> Result<(), Fault> {
        Ok(())
    }
}

impl Processes {
    /// Stands the process up over the running program numbered `process`, whose pipe is `pipe`: running, or refused,
    /// the program killed and its pipe hung up, when the process cannot be.
    fn stand_up(
        &self,
        turn: &mut Turn<'_, Self>,
        process: u64,
        starter: ProtocolRef<Exits>,
        pipe: Piped,
    ) -> Result<Started, Fault> {
        let config = ProcessConfig { number: process, starter, kill_after_millis: self.config.kill_after_millis };
        let refused = match turn.spawn::<Process>(&process, config) {
            Ok(running) => return Ok(Started::Running { process: running, pipe }),
            Err(refused) => refused,
        };
        abandon(turn, process, "process", &refused)?;
        if let Piped::Connected(pipe) = pipe {
            turn.to(pipe).send(&Shut)?;
        }

        Ok(Started::Refused)
    }
}

/// Kills the program numbered `process`, whose `actor` was not stood up, as `refused` says: no actor owns the program,
/// so it is ended here, and its exit is the launcher's to keep.
fn abandon(turn: &mut Turn<'_, Processes>, process: u64, actor: &str, refused: &impl Debug) -> Result<(), Fault> {
    turn.log(Level::Error, format_args!("program {process}'s {actor} was not stood up: {refused:?}"));
    turn.run(&Raise { process, signal: Signal::Kill }, &Raising)?;

    Ok(())
}
