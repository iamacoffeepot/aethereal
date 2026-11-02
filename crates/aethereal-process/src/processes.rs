//! The process capability's root (ADR-0040).

use crate::jobs::{Launch, Launched, Launching, Raise, Raised, Raising};
use crate::{Exits, Process, ProcessConfig, Signal};
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Beginning, Carries, ChildOf, Ending, Fault, Handles, Held, Level, ProtocolRef,
    ToSender, Top, Turn,
};
use aethereal_data::{Bound, Plain};
use std::collections::BTreeMap;

/// What an actor asks the root to start: a program, a path on the host's machine, its arguments, and the actor told
/// how it exited, usually the asker itself (`turn.me().protocol()`), whose ending stops it.
#[derive(Debug, Bound)]
#[kind(name = "aether.process.start")]
pub struct Start {
    pub program: String,
    pub args: Vec<String>,
    pub starter: ProtocolRef<Exits>,
}

/// What a start came to.
#[derive(Debug, Bound)]
#[kind(name = "aether.process.started")]
pub enum Started {
    /// Running: the process beneath the root that owns it.
    Running { process: ActorRef<Process> },
    /// The host would not start it: nothing runnable is at its path, or the host starts no programs.
    Refused,
}

/// What the root is made from: how long a program asked to stop is given (ADR-0040).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.process.config")]
pub struct ProcessesConfig {
    pub stop_within_millis: u32,
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

impl ChildOf for Processes {
    type Parent = Top;
}

/// Asked at its typed path, it answers a start once its program is running, or refused.
impl Carries<Start> for Processes {
    type Answers = (Started,);
}

impl Actor for Processes {
    type Config = ProcessesConfig;
    type Key = ();
    type Handles = (Start, Answer<Launched, Launching>, Answer<Raised, Raising>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: ProcessesConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { config, waiting: BTreeMap::new(), next: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Start> for Processes {
    type Sends = (ToSender<Started>,);

    fn handle(&mut self, start: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let held = turn.hold()?;
        let process = self.next;
        self.next += 1;
        turn.run(&Launch { process, program: start.program, args: start.args }, &Launching { process })?;
        self.waiting.insert(process, Waiting { held, starter: start.starter });

        Ok(())
    }
}

impl Handles<Answer<Launched, Launching>> for Processes {
    type Sends = ();

    /// Stands the process up over a program that started, and answers its start.
    fn handle(
        &mut self,
        (answered, Launching { process }): (Answered<Launched>, Launching),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Some(Waiting { held, starter }) = self.waiting.remove(&process) else {
            return Ok(());
        };
        let started = match answered {
            Answered::Yes(Launched::Started) => {
                let config =
                    ProcessConfig { number: process, starter, stop_within_millis: self.config.stop_within_millis };
                match turn.spawn::<Process>(&process, config) {
                    Ok(process) => Started::Running { process },
                    Err(refused) => {
                        // No actor owns the program, so it is ended here.
                        turn.log(Level::Error, format_args!("process {process} was not stood up: {refused:?}"));
                        turn.run(&Raise { process, signal: Signal::Kill }, &Raising)?;
                        Started::Refused
                    }
                }
            }
            Answered::Yes(Launched::Refused) | Answered::No(_) => Started::Refused,
        };
        turn.answer(held, &started)?;

        Ok(())
    }
}

impl Handles<Answer<Raised, Raising>> for Processes {
    type Sends = ();

    /// The program of a process not stood up was sent its kill: its exit is the launcher's to keep.
    fn handle(&mut self, _: (Answered<Raised>, Raising), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}
