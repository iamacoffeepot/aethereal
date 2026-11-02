//! One engine the hub spawned: the actor that holds its build, owns its process, and says how its spawn went
//! (ADR-0040).

use super::{Engines, Spawn};
use aethereal_actor::{
    Actor, ActorRef, Allows, Answer, Answered, Asks, Beginning, ChildOf, Ending, Fault, Handles, JobId, Level, Path,
    Sleep, Slept, ToType, Turn, path,
};
use aethereal_data::{Bound, Plain};
use aethereal_process::{Exited, Process, Processes, Start, Started, Stop};
use aethereal_store::{Binary, Launch, Launched, Store, StoreHash};
use core::mem;
use core::time::Duration;

/// What a spawn is made from: the binary to launch, the arguments to start it with, the agent's then the link's own
/// (`--hub`, `--port`, `--hub-token`), and how long its program is given to link.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.spawned_config")]
pub struct SpawnedConfig {
    pub binary: Binary,
    pub args: Vec<String>,
    pub link_within_millis: u64,
}

/// What `Engines` tells a spawn once the `Hello` that brought its token has been taken: the id its engine linked as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.linked_as")]
pub struct LinkedAs {
    pub engine: u32,
}

/// What a spawn tells `Engines` once its launch copy is made, before it starts it: the hash of the build it runs.
/// `Engines` takes the token it spawned it with only from then on, so the engine that brings it is listed with its
/// build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.runs")]
pub struct Runs {
    pub hash: StoreHash,
}

/// What a spawn keeps for its launch: nothing, as it asks one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.fetching")]
pub struct Fetching;

/// What a spawn keeps for the start of its program: nothing, as it asks one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.booting")]
pub struct Booting;

/// What a spawn keeps for the sleep that bounds its program's time to link: nothing, as it runs one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.patience")]
pub struct Patience;

/// Where a spawn is.
#[derive(Debug, Bound)]
enum Stage {
    /// The store is asked for a launch copy, started with `args` once it is made, and given `link_within_millis` to
    /// link once it starts.
    Fetching { args: Vec<String>, link_within_millis: u64 },
    /// `Processes` is asked to start the launch copy, which is given `link_within_millis` to link once it starts.
    Booting { link_within_millis: u64 },
    /// Its program runs and has not linked: the sleep `patience` is answered once `link_within_millis` has passed.
    Linking { process: ActorRef<Process>, patience: JobId },
    /// Its program linked, and the spawn was answered `Linked`.
    Linked { process: ActorRef<Process> },
    /// Its process was asked to stop, as its program did not link in time or the agent terminated it, and the spawn
    /// was answered: its exit is awaited.
    Stopping { process: ActorRef<Process> },
}

/// One engine the hub spawned, beneath `Engines`, keyed by `Engines`' count of spawns (ADR-0040). As it begins it asks
/// the store to [`Launch`] its binary, so the build is held for as long as it lives, then tells `Engines` the build
/// it [`Runs`], and asks `Processes` to [`Start`] the launch copy with its arguments, naming itself to be told how the
/// program exited.
///
/// It tells `Engines` the [`Spawn`] once, as soon as it is known: `Linked` when `Engines` tells it the engine's id,
/// `Exited` when the program ends before that, `Unlinked` when it has not linked within `link_within_millis`, its
/// process then stopped, and `NotStored` or `Refused` at once, when there is nothing to start or the host would not
/// start it. Asked to [`Stop`] by `Engines`, it asks its process to.
///
/// It lives as long as its process: told [`Exited`], it tells `Engines` how, and ends, so the store's hold and the
/// launch copy go with it. A program whose spawn ends is stopped by its process, which watches its starter.
#[derive(Bound)]
#[kind(name = "aether.link.spawned")]
pub struct Spawned {
    stage: Stage,
}

impl ChildOf for Spawned {
    type Parent = Engines;
}

impl Actor for Spawned {
    type Config = SpawnedConfig;
    type Key = u64;
    type Handles =
        (Answer<Launched, Fetching>, Answer<Started, Booting>, Answer<Slept, Patience>, Exited, LinkedAs, Stop);
    type BeginSends = (Asks<Path<Store>, Launch, Fetching>,);
    type EndSends = ();

    /// Asks the store for a launch copy of its binary.
    fn begin(config: SpawnedConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let SpawnedConfig { binary, args, link_within_millis } = config;
        turn.to(path::<Store>(())).ask(&Launch { binary }, &Fetching)?;

        Ok(Self { stage: Stage::Fetching { args, link_within_millis } })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Answer<Launched, Fetching>> for Spawned {
    type Sends = (ToType<Engines, Runs>, Asks<Path<Processes>, Start, Booting>, ToType<Engines, Spawn>);

    /// A launch copy made: `Engines` is told the build it runs, and `Processes` asked to start it. Nothing stored, or
    /// no copy made, answers the spawn, and it ends.
    fn handle(
        &mut self,
        (answered, Fetching): (Answered<Launched>, Fetching),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Stage::Fetching { args, link_within_millis } = &mut self.stage else {
            return Ok(());
        };
        let args = mem::take(args);
        self.stage = Stage::Booting { link_within_millis: *link_within_millis };
        let (program, hash) = match answered {
            Answered::Yes(Launched::Ready { program, hash }) => (program, hash),
            Answered::Yes(Launched::NotStored) => return answer_and_end(turn, &Spawn::NotStored),
            Answered::Yes(Launched::Failed) | Answered::No(_) => {
                turn.log(Level::Warn, format_args!("no launch copy was made: {answered:?}"));
                return answer_and_end(turn, &Spawn::Refused);
            }
        };
        turn.to(turn.parent()).send(&Runs { hash })?;
        let start = Start { program, args, starter: turn.me().protocol() };
        turn.to(path::<Processes>(())).ask(&start, &Booting)?;

        Ok(())
    }
}

impl Handles<Answer<Started, Booting>> for Spawned {
    type Sends = (ToType<Engines, Spawn>,);

    /// Its program running, it waits for it to link, for `link_within_millis` at most; refused, it answers the spawn,
    /// and ends.
    fn handle(
        &mut self,
        (answered, Booting): (Answered<Started>, Booting),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Stage::Booting { link_within_millis } = self.stage else {
            return Ok(());
        };
        let Answered::Yes(Started::Running { process }) = answered else {
            turn.log(Level::Warn, format_args!("the engine's program was not started: {answered:?}"));
            return answer_and_end(turn, &Spawn::Refused);
        };
        let until = turn.now() + Duration::from_millis(link_within_millis);
        let patience = turn.run(&Sleep { until }, &Patience)?;
        self.stage = Stage::Linking { process, patience };

        Ok(())
    }
}

impl Handles<Answer<Slept, Patience>> for Spawned {
    type Sends = (ToType<Engines, Spawn>, ToType<Process, Stop>);

    /// Its program has not linked in its time, or this would have been cancelled: the spawn is answered `Unlinked`,
    /// and the process asked to stop.
    fn handle(&mut self, _: (Answered<Slept>, Patience), turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Stage::Linking { process, .. } = self.stage else {
            return Ok(());
        };
        turn.to(turn.parent()).send(&Spawn::Unlinked)?;
        turn.to(process).send(&Stop)?;
        self.stage = Stage::Stopping { process };

        Ok(())
    }
}

impl Handles<Exited> for Spawned {
    type Sends = (ToType<Engines, Spawn>, ToType<Engines, Exited>);

    /// Its program has exited, as its own process tells it: a spawn not yet answered is answered `Exited`, `Engines`
    /// is told how it exited, and it ends. From any other actor, it is passed over.
    fn handle(&mut self, exited: Exited, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let (process, unanswered) = match self.stage {
            Stage::Linking { process, patience } => (process, Unanswered::Waiting(patience)),
            Stage::Linked { process } | Stage::Stopping { process } => (process, Unanswered::No),
            Stage::Fetching { .. } | Stage::Booting { .. } => return Ok(()),
        };
        let is_its_process = turn.sender().id() == process.id();
        if !is_its_process {
            return Ok(());
        }
        if let Unanswered::Waiting(patience) = unanswered {
            turn.cancel(patience);
            let spawn = Spawn::Exited { status: exited.status, stderr: exited.stderr.clone() };
            turn.to(turn.parent()).send(&spawn)?;
        }
        turn.to(turn.parent()).send(&exited)?;
        turn.close();

        Ok(())
    }
}

impl Handles<LinkedAs> for Spawned {
    type Sends = (ToType<Engines, Spawn>,);

    /// Its engine linked, as `Engines` tells it: the spawn is answered `Linked`, unless it was answered already. From
    /// any other actor, it is passed over.
    fn handle(&mut self, linked: LinkedAs, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let is_from_parent = turn.sender().id() == turn.parent().id();
        let Stage::Linking { process, patience } = self.stage else {
            return Ok(());
        };
        if !is_from_parent {
            return Ok(());
        }
        turn.cancel(patience);
        turn.to(turn.parent()).send(&Spawn::Linked { engine: linked.engine })?;
        self.stage = Stage::Linked { process };

        Ok(())
    }
}

impl Handles<Stop> for Spawned {
    type Sends = (ToType<Process, Stop>,);

    /// `Engines` asks it to stop its engine: its process is asked to. A spawn not yet answered stays so, and is
    /// answered `Exited` once the program has. From any other actor, it is passed over.
    fn handle(&mut self, _: Stop, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let is_from_parent = turn.sender().id() == turn.parent().id();
        if !is_from_parent {
            return Ok(());
        }
        match self.stage {
            Stage::Linking { process, .. } => {
                turn.to(process).send(&Stop)?;
            }
            Stage::Linked { process } => {
                turn.to(process).send(&Stop)?;
                self.stage = Stage::Stopping { process };
            }
            Stage::Fetching { .. } | Stage::Booting { .. } | Stage::Stopping { .. } => {}
        }

        Ok(())
    }
}

/// Whether a spawn whose program exited is still to be answered.
enum Unanswered {
    /// It is: its program had not linked, and the sleep that bounds its time to link still runs.
    Waiting(JobId),
    /// It is not: it was answered `Linked` or `Unlinked`.
    No,
}

/// Tells `Engines` the spawn came to `spawn`, and ends: nothing of it is running.
fn answer_and_end<S, I>(turn: &mut Turn<'_, Spawned, S>, spawn: &Spawn) -> Result<(), Fault>
where
    S: Allows<ToType<Engines, Spawn>, I>,
{
    turn.to(turn.parent()).send::<_, I>(spawn)?;
    turn.close();

    Ok(())
}
