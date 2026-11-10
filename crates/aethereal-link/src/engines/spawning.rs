//! What `Engines` is asked of the engines it spawns and terminates, and how it keeps each spawn (ADR-0040).

use super::{Came, Engine, Engines, Linked, Minting, Port, Runs, Spawned, SpawnedConfig, Standing, Waiting};
use crate::Token;
use crate::engines::ExitedEngine;
use crate::engines::randomness::Draw;
use aethereal_actor::{ActorRef, Fault, Handles, Held, Level, ToSender, ToType, Turn};
use aethereal_data::{ActorId, Bound, Plain};
use aethereal_process::{ExitStatus, Exited, Stop};
use aethereal_store::Binary;
use aethereal_tcp::Shut;
use core::mem;

/// Asks `Engines`, at `path::<Engines>(())`, to spawn an engine from its store: `binary`, started with `args`, then the
/// link's own arguments. Answered with [`Spawn`] once the engine has linked, or what came of it before.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.spawn_engine")]
pub struct SpawnEngine {
    /// The stored binary: a name, or the 64-digit hash.
    #[flag(position)]
    pub binary: Binary,
    /// The arguments the engine is started with, before those that link it: none for no arguments.
    #[flag]
    pub args: Option<Vec<String>>,
}

/// What came of a [`SpawnEngine`].
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.spawn")]
pub enum Spawn {
    /// Started, and linked as engine `engine`.
    Linked { engine: u32 },
    /// The store holds no binary of that name or hash.
    NotStored,
    /// The host would not start it.
    Refused,
    /// It ended before it linked: how, and the head and tail of its stderr.
    Exited { status: ExitStatus, stderr: String },
    /// It ran, and did not link within the hub's `link_within_millis`: its process was then stopped.
    Unlinked,
}

/// Asks `Engines`, at `path::<Engines>(())`, to terminate the engine linked as `engine`, answered [`Terminated`] at
/// once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.terminate_engine")]
pub struct TerminateEngine {
    /// The engine to terminate, by its id.
    #[flag(position)]
    pub engine: u32,
}

/// What came of a [`TerminateEngine`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.terminated")]
pub enum Terminated {
    /// The engine is being stopped: a spawned engine's process is asked to stop, and an engine started outside the
    /// hub has its link shut, and ends by the rule that no engine outlives its link.
    Stopping,
    /// No engine of that id is linked.
    NotLinked,
}

/// Whether a spawn's request has been answered.
#[derive(Debug, Bound)]
enum Request {
    /// Not yet: the request, answered with what the spawn tells.
    Held(Held<Spawn>),
    /// It has been, as the spawn told the first time.
    Answered,
}

/// One spawn `Engines` keeps while its [`Spawned`] lives: the actor, the binary it was asked for, the token its engine
/// links with, which `Engines` takes once the spawn says the build it runs, and lets go of as it ends, and its
/// request.
#[derive(Bound)]
pub(super) struct Spawning {
    pub(super) spawned: ActorRef<Spawned>,
    pub(super) binary: Binary,
    token: Token,
    request: Request,
}

/// What terminating an engine asks, and of whom.
enum Terminating {
    /// Its spawn, to stop its process.
    Spawned(ActorRef<Spawned>),
    /// The engine, to shut its link.
    External(ActorRef<Engine>),
    /// Nothing: its token has not been taken, so it is not linked.
    Unproven,
}

impl Handles<SpawnEngine> for Engines {
    type Sends = (ToSender<Spawn>,);

    /// Holds the request, and draws its engine's token from the host's randomness.
    fn handle(&mut self, spawn: SpawnEngine, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let held = turn.hold()?;
        turn.run(&Draw, &Minting::Spawn { held, spawn })?;

        Ok(())
    }
}

impl Handles<Runs> for Engines {
    type Sends = ();

    /// One of its spawns runs a build: the token its engine links with is taken from now on. From any other actor, it
    /// is passed over.
    fn handle(&mut self, runs: Runs, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let Some((_, spawning)) = self.spawn_of(turn.sender().id()) else {
            return Ok(());
        };
        let (token, spawned, binary) = (spawning.token, spawning.spawned, spawning.binary.clone());
        self.tokens.mint_spawn(token, spawned, binary, runs.hash);

        Ok(())
    }
}

impl Handles<Spawn> for Engines {
    type Sends = ();

    /// What one of its spawns came to: the request for it is answered, the first time. From any other actor, it is
    /// passed over.
    fn handle(&mut self, spawn: Spawn, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let sender = turn.sender().id();
        let Some(spawning) = self.spawns.values_mut().find(|spawning| spawning.spawned.id() == sender) else {
            return Ok(());
        };
        let request = mem::replace(&mut spawning.request, Request::Answered);
        if let Request::Held(held) = request {
            turn.answer(held, &spawn)?;
        }

        Ok(())
    }
}

impl Handles<Exited> for Engines {
    type Sends = ();

    /// How one of its spawns' programs exited, as the spawn ends: kept, with its binary, among the last `most_exited`,
    /// and the spawn let go of at once, its token with it. From any other actor, it is passed over.
    fn handle(&mut self, exited: Exited, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let Some(binary) = self.spawn_ended(turn.sender().id()) else {
            return Ok(());
        };
        self.exited.push(ExitedEngine { binary, status: exited.status, stderr: exited.stderr });
        let most = usize::try_from(self.settings.most_exited).unwrap_or(usize::MAX);
        let over = self.exited.len().saturating_sub(most);
        self.exited.drain(..over);

        Ok(())
    }
}

impl Handles<TerminateEngine> for Engines {
    type Sends = (ToSender<Terminated>, ToType<Spawned, Stop>, ToType<Engine, Shut>);

    /// A spawned engine's spawn is asked to stop its process; an engine started outside the hub is asked to shut its
    /// link, and ends as its link does. An engine whose token has not been taken is not linked.
    fn handle(&mut self, terminate: TerminateEngine, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let linked = self.linked.get(&terminate.engine).map(Linked::terminating);
        let terminated = match linked {
            Some(Terminating::Spawned(spawned)) => {
                turn.to(spawned).send(&Stop)?;
                Terminated::Stopping
            }
            Some(Terminating::External(engine)) => {
                turn.to(engine).send(&Shut)?;
                Terminated::Stopping
            }
            Some(Terminating::Unproven) | None => Terminated::NotLinked,
        };
        turn.reply(&terminated)?;

        Ok(())
    }
}

impl Linked {
    /// What terminating it asks, and of whom.
    fn terminating(&self) -> Terminating {
        match &self.standing {
            Standing::Linked { came: Came::Spawned { spawned, .. }, .. } => Terminating::Spawned(*spawned),
            Standing::Linked { came: Came::External, .. } => Terminating::External(self.engine),
            Standing::Unproven => Terminating::Unproven,
        }
    }
}

impl Engines {
    /// The spawn whose actor is `actor`, with its number: none when it is no spawn of this root's.
    fn spawn_of(&self, actor: ActorId) -> Option<(u64, &Spawning)> {
        self.spawns
            .iter()
            .find(|(_, spawning)| spawning.spawned.id() == actor)
            .map(|(&spawn, spawning)| (spawn, spawning))
    }

    /// Spawns, for the request `held`, the engine `spawn` asks for, which links to this hub at `port` with `token`; a
    /// spawn the engine will not stand up is answered `Refused` at once.
    pub(super) fn spawn<S>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        held: Held<Spawn>,
        spawn: SpawnEngine,
        token: Token,
        port: u16,
    ) -> Result<(), Fault> {
        let SpawnEngine { binary, args } = spawn;
        let args = args.into_iter().flatten().chain(self.link_args(token, port)).collect();
        let config =
            SpawnedConfig { binary: binary.clone(), args, link_within_millis: self.settings.link_within_millis };
        let number = self.next_spawn;
        self.next_spawn += 1;
        let spawned = match turn.spawn::<Spawned>(&number, config) {
            Ok(spawned) => spawned,
            Err(refused) => {
                turn.log(Level::Error, format_args!("spawn {number} was not stood up: {refused:?}"));
                turn.answer(held, &Spawn::Refused)?;
                return Ok(());
            }
        };
        turn.watch(spawned);
        self.spawns.insert(number, Spawning { spawned, binary, token, request: Request::Held(held) });

        Ok(())
    }

    /// Lets go of the spawn whose actor is `gone`, if it is one, as it ends: its token is let go of, and a request for
    /// it still held is dropped, which the engine answers as unanswered. The binary it was asked for, or none when
    /// `gone` is no spawn, or one let go of already.
    pub(super) fn spawn_ended(&mut self, gone: ActorId) -> Option<Binary> {
        let (spawn, _) = self.spawn_of(gone)?;
        let Spawning { binary, token, request, .. } = self.spawns.remove(&spawn)?;
        self.tokens.forget(token);
        drop(request);

        Some(binary)
    }

    /// A token drawn for a spawn: spawned at once when the root listens, or kept waiting for its port.
    pub(super) fn spawn_drawn<S>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        held: Held<Spawn>,
        spawn: SpawnEngine,
        token: Token,
    ) -> Result<(), Fault> {
        let port = match &mut self.port {
            Port::Bound(port) => *port,
            Port::Asked { minted } => {
                minted.push(Waiting::Spawn { held, spawn, token });
                return Ok(());
            }
        };

        self.spawn(turn, held, spawn, token, port)
    }
}
