//! A process: one program running, owned by an actor that waits for it to exit and stops it when asked (ADR-0040).

use crate::jobs::{Raise, Raised, Raising, Wait, Waited, Waiting};
use crate::{Processes, Signal, Unwaited};
use aethereal_actor::{
    Actor, Answer, Answered, Beginning, Carries, ChildOf, Ended, Ending, Fault, Handles, JobId, Level, Protocol,
    ProtocolRef, Sleep, Slept, ToAny, Turn,
};
use aethereal_data::{Bound, Plain};
use core::time::Duration;

/// How a program exited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.process.exit_status")]
pub enum ExitStatus {
    /// It exited with this code.
    Code(i32),
    /// This signal ended it.
    Signal(i32),
}

/// Told to the actor a program was started for, as its process ends: how it exited, and its stderr as text, its head
/// and its tail each bounded with a line saying what was left out between them, every over-long line cut, and a
/// character the program's own bytes cut short replaced (ADR-0040).
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.process.exited")]
pub struct Exited {
    pub status: ExitStatus,
    pub stderr: String,
}

/// Asks a process to stop its program: its group is sent SIGTERM, then SIGKILL if it still runs once the setting
/// `kill_after_millis` has passed. [`Exited`] follows either way. Asking again changes nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.process.stop")]
pub struct Stop;

/// What the actor a program is started for takes: the [`Exited`] its process tells it (ADR-0040). It is named in
/// [`Start`](crate::Start) by a reference to it of this protocol.
pub struct Exits;

impl Protocol for Exits {
    type Kinds = (Exited,);
}

impl Carries<Exited> for Exits {
    type Answers = ();
}

/// What a process is made from: its number, which names its program to the launcher, the actor it tells how the
/// program exited, and how long a program asked to stop is given.
#[derive(Debug, Bound)]
#[kind(name = "aether.process.process_config")]
pub struct ProcessConfig {
    pub number: u64,
    pub starter: ProtocolRef<Exits>,
    pub kill_after_millis: u32,
}

/// What a process keeps for the sleep that gives its program its time to end: nothing, as it runs one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.process.grace")]
pub struct Grace;

/// Whether a process has asked its program to end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Bound)]
enum Stopping {
    /// It has not: the program runs until it exits.
    Unasked,
    /// Its group was sent SIGTERM, and this sleep runs until it is to be killed.
    Asked(JobId),
}

/// One program running, beneath the root, keyed by the root's count (ADR-0040). It owns nothing the engine keeps:
/// its program is the host's launcher's, named by its number.
///
/// As it begins it watches its starter and waits for its program to exit; once it has, it tells the starter
/// [`Exited`] and ends. Asked to [`Stop`], or once its starter ends, it signals its program's group to end, and kills
/// the group if it still runs once `kill_after_millis` has passed.
#[derive(Bound)]
#[kind(name = "aether.process.process")]
pub struct Process {
    number: u64,
    starter: ProtocolRef<Exits>,
    kill_after_millis: u32,
    stopping: Stopping,
}

impl ChildOf for Process {
    type Parent = Processes;
}

impl Actor for Process {
    type Config = ProcessConfig;
    type Key = u64;
    type Handles = (Stop, Ended, Answer<Waited, Waiting>, Answer<Slept, Grace>, Answer<Raised, Raising>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: ProcessConfig, turn: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        turn.watch(config.starter);
        turn.run(&Wait { process: config.number }, &Waiting)?;

        Ok(Self {
            number: config.number,
            starter: config.starter,
            kill_after_millis: config.kill_after_millis,
            stopping: Stopping::Unasked,
        })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Stop> for Process {
    type Sends = ();

    fn handle(&mut self, _: Stop, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.stop(turn)
    }
}

impl Handles<Ended> for Process {
    type Sends = ();

    /// Its starter has ended, which is the one actor it watches: no one wants the program any more.
    fn handle(&mut self, ended: Ended, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let is_starter = ended.actor().id() == self.starter.id();
        if is_starter {
            self.stop(turn)?;
        }

        Ok(())
    }
}

impl Handles<Answer<Waited, Waiting>> for Process {
    type Sends = (ToAny<Exits, Exited>,);

    /// The program has exited: its starter is told how, and the process ends.
    fn handle(
        &mut self,
        (answered, Waiting): (Answered<Waited>, Waiting),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Waited::Exited { status, stderr }) = answered else {
            turn.log(Level::Error, format_args!("the exit of program {} was lost: {answered:?}", self.number));
            return Err(Fault::new(&Unwaited)?);
        };
        if let Stopping::Asked(grace) = self.stopping {
            turn.cancel(grace);
        }
        turn.to(self.starter).send(&Exited { status, stderr })?;
        turn.close();

        Ok(())
    }
}

impl Handles<Answer<Slept, Grace>> for Process {
    type Sends = ();

    /// The program's time to end has passed, and it has not exited, or this would have been cancelled: its group is
    /// killed.
    fn handle(&mut self, _: (Answered<Slept>, Grace), turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        turn.run(&Raise { process: self.number, signal: Signal::Kill }, &Raising)?;

        Ok(())
    }
}

impl Handles<Answer<Raised, Raising>> for Process {
    type Sends = ();

    /// A signal was sent: what it came to is told by the program's exit.
    fn handle(&mut self, _: (Answered<Raised>, Raising), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Process {
    /// Sends the program's group SIGTERM and sleeps for the time it is given to end, unless it was asked already.
    fn stop<S>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault> {
        let Stopping::Unasked = self.stopping else {
            return Ok(());
        };
        turn.run(&Raise { process: self.number, signal: Signal::Terminate }, &Raising)?;
        let until = turn.now() + Duration::from_millis(u64::from(self.kill_after_millis));
        self.stopping = Stopping::Asked(turn.run(&Sleep { until }, &Grace)?);

        Ok(())
    }
}
