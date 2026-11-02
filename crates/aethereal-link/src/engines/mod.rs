//! The hub's end of the link: the root that listens for engines, mints the tokens they link with, keeps one actor
//! for each linked, and spawns engines from the store (ADR-0038, ADR-0040).

mod asks;
mod engine;
mod randomness;
mod settings;
mod spawned;
mod spawning;
mod tokens;

pub use asks::{
    CallEngine, DescribeEngine, Described, EngineList, EngineListing, ExitedEngine, ListEngines, Origin, Shown,
};
pub use engine::{Engine, EngineConfig, Sent, Vouching};
pub use randomness::{Randomness, Unfilled};
pub use settings::EnginesSettings;
pub use spawned::{Booting, Fetching, LinkedAs, Patience, Runs, Spawned, SpawnedConfig};
pub use spawning::{Spawn, SpawnEngine, TerminateEngine, Terminated};
pub use tokens::{MintToken, Minted, Minting, TakeToken, TokenFor};

use crate::Token;
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Asks, Beginning, Carries, ChildOf, Ended, Ending, Fault, Handles, Held, Level,
    More, Number, Open, Part, Path, Streamed, Streams, Taken, ToAny, ToSender, ToType, Top, Turn, path,
};
use aethereal_data::{Bound, Plain};
use aethereal_process::Exited;
use aethereal_store::{Binary, StoreHash};
use aethereal_tcp::{Accept, Accepted, Accepting, Framing, Listen, Listening, Shut, Tcp, Unbound};
use core::mem;
use core::time::Duration;
use randomness::{Draw, Drawn};
use spawning::Spawning;
use std::collections::BTreeMap;
use std::num::NonZeroU32;
use tokens::{Purpose, Tokens};

/// What the root keeps for its listen: nothing, since it asks one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.binding")]
pub struct Binding;

/// Why the root failed: `Tcp` gave it no port, or its listener stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.unlistened")]
pub struct Unlistened;

/// Where the root is in listening.
#[derive(Debug, Bound)]
enum Port {
    /// `Tcp` has not answered: each token minted meanwhile waits, with its request, for the port.
    Asked { minted: Vec<Waiting> },
    /// `Tcp` bound this port.
    Bound(u16),
}

/// A token minted before the root listens, and the request it is for, which waits until it does.
#[derive(Debug, Bound)]
enum Waiting {
    /// A [`MintToken`]'s, answered with the token and the port.
    Minted { held: Held<Minted>, token: Token },
    /// A [`SpawnEngine`]'s, whose engine is spawned to link with the token at the port.
    Spawn { held: Held<Spawn>, spawn: SpawnEngine, token: Token },
}

/// What the agent is shown of one engine whose token was taken.
#[derive(Debug, Plain)]
enum Shows {
    /// Nothing: it has not told what it shows.
    Nothing,
    /// The name it gave, and how many tools it holds.
    Named { name: String, tools: u32 },
}

/// Where an engine whose token was taken came from.
#[derive(Bound)]
enum Came {
    /// It was started outside the hub.
    External,
    /// The hub spawned it: its spawn, the binary the spawn was asked for, and the hash of the build it runs.
    Spawned { spawned: ActorRef<Spawned>, binary: Binary, hash: StoreHash },
}

impl Came {
    /// What its token was for, as the engine that brought it is answered.
    const fn token_for(&self) -> TokenFor {
        match self {
            Self::External => TokenFor::External,
            Self::Spawned { .. } => TokenFor::Spawned,
        }
    }

    /// Where it came from, as the agent is shown.
    fn origin(&self) -> Origin {
        match self {
            Self::External => Origin::External,
            Self::Spawned { binary, hash, .. } => Origin::Spawned { binary: binary.clone(), hash: *hash },
        }
    }
}

/// Whether an engine is linked.
#[derive(Bound)]
enum Standing {
    /// Its token has not been taken: it has not said `Hello`, or `Engines` has not answered it.
    Unproven,
    /// Its token was taken: where it came from, and what it shows.
    Linked { came: Came, shows: Shows },
}

/// An engine connected: its actor, and whether it is linked.
#[derive(Bound)]
struct Linked {
    engine: ActorRef<Engine>,
    standing: Standing,
}

/// The connections the root's listener may accept ahead of the root spawning their engines.
const ACCEPTING_WINDOW: NonZeroU32 = NonZeroU32::MIN.saturating_add(3);

/// The hub's end of the link (ADR-0038, ADR-0040): listens on its port through `Tcp`, and for each connection accepted,
/// spawns an [`Engine`] beneath itself, keyed by a count it keeps, which is the engine's id for the agent. An id is a
/// `u32`, whose JSON form is a number an agent writes as it reads it (ADR-0007, ADR-0038), and an engine's key is that
/// id. An actor asks it to [`ListEngines`] or to [`MintToken`] at its typed path, `path::<Engines>(())`, and asks an
/// engine itself at `path::<Engine>(id)`.
///
/// Every engine links with a token it minted, good for one `Hello`: an engine reading a `Hello` asks it to
/// [`TakeToken`], and is listed only once it has. A token is 16 bytes of the host's [`Randomness`], drawn by a job. One
/// for an engine started outside the hub lasts `token_within_millis`, measured on the engine's clock; at most
/// `most_tokens` are kept, the oldest dropped as another is minted. A token asked for before the hub listens is
/// answered once it does, with the port bound.
///
/// Asked to [`SpawnEngine`], it holds the request, mints a token, and spawns a [`Spawned`] beneath itself, keyed by a
/// count of its own, with the binary and the agent's arguments followed by the link's own, built as [`Minted`]'s are;
/// a spawn asked before the hub listens waits until it does. It takes the spawn's token once the spawn tells it the
/// build it [`Runs`], until the spawn ends, and when the `Hello` that brings it is taken, tells the spawn its engine's
/// id ([`LinkedAs`]). The spawn tells it what the request comes to, which it answers, and how its program exited,
/// which it keeps among the last `most_exited`. Asked to [`TerminateEngine`], it asks a spawned engine's spawn to
/// [`Stop`], and an engine started outside the hub to [`Shut`] its link.
///
/// Each engine tells it the name it gave and how many tools it holds as they change, so its list is answered at once,
/// and it watches each engine and spawn, dropping one from its list when it ends.
///
/// It stands up on a host that serves it randomness: each native host's crate bootstraps it, serving the system's,
/// and checks its settings with [`EnginesSettings::check_read_limit`].
#[derive(Bound)]
#[kind(name = "aether.link.engines")]
pub struct Engines {
    settings: EnginesSettings,
    port: Port,
    accepting: Streams<Accepting, ()>,
    /// Each engine connected, by its id.
    linked: BTreeMap<u32, Linked>,
    next: u32,
    tokens: Tokens,
    /// Each spawn living, by its number.
    spawns: BTreeMap<u64, Spawning>,
    next_spawn: u64,
    /// The last `most_exited` spawned engines that ended, the latest last.
    exited: Vec<ExitedEngine>,
}

impl Engines {
    /// The port listened on: none until `Tcp` has bound it.
    #[must_use]
    pub const fn port(&self) -> Option<u16> {
        match self.port {
            Port::Asked { .. } => None,
            Port::Bound(port) => Some(port),
        }
    }

    /// The engine of id `engine`: none when no such engine is linked.
    #[must_use]
    pub fn engine(&self, engine: u32) -> Option<ActorRef<Engine>> {
        self.linked.get(&engine).map(|linked| linked.engine)
    }
}

impl ChildOf for Engines {
    type Parent = Top;
}

/// Asked at its typed path, it answers its list at once.
impl Carries<ListEngines> for Engines {
    type Answers = (EngineList,);
}

/// Asked at its typed path, it answers once a token is drawn and it listens.
impl Carries<MintToken> for Engines {
    type Answers = (Minted,);
}

/// Asked at its typed path, it answers at once.
impl Carries<TakeToken> for Engines {
    type Answers = (TokenFor,);
}

/// Asked at its typed path, it answers once the engine has linked, or what came of it before.
impl Carries<SpawnEngine> for Engines {
    type Answers = (Spawn,);
}

/// Asked at its typed path, it answers at once.
impl Carries<TerminateEngine> for Engines {
    type Answers = (Terminated,);
}

impl Actor for Engines {
    type Config = EnginesSettings;
    type Key = ();
    type Handles = (
        Answer<Listening, Binding>,
        Part<Accepting>,
        Answer<Unbound, Number>,
        Ended,
        Shown,
        ListEngines,
        MintToken,
        Answer<Drawn, Minting>,
        TakeToken,
        SpawnEngine,
        Runs,
        Spawn,
        Exited,
        TerminateEngine,
    );
    type BeginSends = (Asks<Path<Tcp>, Listen, Binding>,);
    type EndSends = ();

    /// Asks `Tcp` to listen on its port.
    fn begin(settings: EnginesSettings, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let listen =
            Listen { port: settings.port, framing: Framing::LengthPrefixed { most_bytes: settings.most_bytes } };
        turn.to(path::<Tcp>(())).ask(&listen, &Binding)?;

        Ok(Self {
            settings,
            port: Port::Asked { minted: Vec::new() },
            accepting: Streams::new(ACCEPTING_WINDOW),
            linked: BTreeMap::new(),
            next: 0,
            tokens: Tokens::default(),
            spawns: BTreeMap::new(),
            next_spawn: 0,
            exited: Vec::new(),
        })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Answer<Listening, Binding>> for Engines {
    type Sends = (Asks<Streamed<Accepting>, Open<Accepting>, Number>, ToAny<Streamed<Accepting>, More>);

    /// Bound, it accepts on its listener, answers each token minted meanwhile, and spawns each engine asked for
    /// meanwhile; otherwise it fails, and each request is answered that it went unanswered.
    fn handle(
        &mut self,
        (answered, Binding): (Answered<Listening>, Binding),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Listening::Bound { listener, port }) = answered else {
            turn.log(Level::Error, format_args!("no port to listen for engines on: {answered:?}"));
            return Err(Fault::new(&Unlistened)?);
        };
        self.accepting.open(turn, listener.protocol(), Accept, ())?;
        let Port::Asked { minted } = mem::replace(&mut self.port, Port::Bound(port)) else {
            return Ok(());
        };
        for waiting in minted {
            match waiting {
                Waiting::Minted { held, token } => turn.answer(held, &self.minted(token, port))?,
                Waiting::Spawn { held, spawn, token } => self.spawn(turn, held, spawn, token, port)?,
            }
        }

        Ok(())
    }
}

impl Handles<Part<Accepting>> for Engines {
    type Sends = (ToAny<Streamed<Accepting>, More>, ToType<Accepted, Shut>);

    /// A connection accepted: an engine is spawned for it, unless the root holds its most, when it is shut.
    fn handle(&mut self, part: Part<Accepting>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Some(Taken { value, .. }) = self.accepting.part(turn, part)? else {
            return Ok(());
        };
        let connection = value.connection;
        let room = usize::try_from(self.settings.most_engines).unwrap_or(usize::MAX);
        let is_full = self.linked.len() >= room;
        if is_full {
            turn.to(connection).send(&Shut)?;
            return Ok(());
        }
        let config =
            EngineConfig { connection, most_calls: self.settings.most_calls, most_bytes: self.settings.most_bytes };
        let id = self.next;
        let Some(following) = id.checked_add(1) else {
            // Every id has been given: no engine links again.
            turn.to(connection).send(&Shut)?;
            return Ok(());
        };
        self.next = following;
        let Ok(engine) = turn.spawn::<Engine>(&u64::from(id), config) else {
            turn.to(connection).send(&Shut)?;
            return Ok(());
        };
        turn.watch(engine);
        self.linked.insert(id, Linked { engine, standing: Standing::Unproven });

        Ok(())
    }
}

impl Handles<Answer<Unbound, Number>> for Engines {
    type Sends = ();

    /// Its listener stopped: no engine can link again, and it fails.
    fn handle(
        &mut self,
        (answered, number): (Answered<Unbound>, Number),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        self.accepting.answered(number);
        turn.log(Level::Error, format_args!("the listener for engines stopped: {answered:?}"));

        Err(Fault::new(&Unlistened)?)
    }
}

impl Handles<Ended> for Engines {
    type Sends = ();

    /// An engine or a spawn has ended: an engine leaves its list, and a spawn's token goes with it.
    fn handle(&mut self, ended: Ended, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let gone = ended.actor().id();
        self.linked.retain(|_, linked| linked.engine.id() != gone);
        self.spawn_ended(gone);

        Ok(())
    }
}

impl Handles<Shown> for Engines {
    type Sends = ();

    /// What one of its own engines shows, once its token is taken: from any other actor, it is passed over.
    fn handle(&mut self, shown: Shown, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let sender = turn.sender().id();
        let linked = self.linked.values_mut().find(|linked| linked.engine.id() == sender);
        if let Some(Linked { standing: Standing::Linked { shows, .. }, .. }) = linked {
            *shows = Shows::Named { name: shown.name, tools: shown.tools };
        }

        Ok(())
    }
}

impl Handles<ListEngines> for Engines {
    type Sends = (ToSender<EngineList>,);

    fn handle(&mut self, _: ListEngines, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let engines = self.linked.iter().filter_map(|(&engine, linked)| linked.listing(engine)).collect();
        turn.reply(&EngineList { engines, exited: self.exited.clone() })?;

        Ok(())
    }
}

impl Handles<MintToken> for Engines {
    type Sends = (ToSender<Minted>,);

    /// Holds the request, and draws a token for it from the host's randomness.
    fn handle(&mut self, _: MintToken, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let held = turn.hold()?;
        turn.run(&Draw, &Minting::External { held })?;

        Ok(())
    }
}

impl Handles<Answer<Drawn, Minting>> for Engines {
    type Sends = ();

    /// A token drawn: for an engine started outside the hub, kept in the table, and answered with the arguments to
    /// start it with, or kept waiting for the port; for a spawn, its engine spawned, or kept waiting for the port. A
    /// draw that came to nothing answers a spawn `Refused`, and a token's request nothing, and the engine tells its
    /// asker it went unanswered.
    fn handle(
        &mut self,
        (answered, minting): (Answered<Drawn>, Minting),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Drawn::Token(token)) = answered else {
            turn.log(Level::Error, format_args!("no token was drawn: {answered:?}"));
            if let Minting::Spawn { held, .. } = minting {
                turn.answer(held, &Spawn::Refused)?;
            }
            return Ok(());
        };
        let held = match minting {
            Minting::External { held } => held,
            Minting::Spawn { held, spawn } => return self.spawn_drawn(turn, held, spawn, token),
        };
        let within = Duration::from_millis(self.settings.token_within_millis);
        let most = usize::try_from(self.settings.most_tokens).unwrap_or(usize::MAX);
        self.tokens.mint_external(token, turn.now(), within, most);
        let port = match &mut self.port {
            Port::Bound(port) => *port,
            Port::Asked { minted } => {
                minted.push(Waiting::Minted { held, token });
                return Ok(());
            }
        };
        turn.answer(held, &self.minted(token, port))?;

        Ok(())
    }
}

impl Handles<TakeToken> for Engines {
    type Sends = (ToSender<TokenFor>, ToType<Spawned, LinkedAs>);

    /// Takes the token out of the table: what it was for, or that it is unknown, once used, expired, or its spawn
    /// ended too. The engine that asked is linked from then on, with where it came from, and a spawn's told its id.
    fn handle(&mut self, take: TakeToken, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let sender = turn.sender().id();
        let Some(purpose) = self.tokens.take(take.token, turn.now()) else {
            turn.reply(&TokenFor::Unknown)?;
            return Ok(());
        };
        let came = Came::from(purpose);
        let token_for = came.token_for();
        let linked = self.linked.iter_mut().find(|(_, linked)| linked.engine.id() == sender);
        if let Some((&engine, linked)) = linked {
            if let Came::Spawned { spawned, .. } = &came {
                turn.to(*spawned).send(&LinkedAs { engine })?;
            }
            linked.standing = Standing::Linked { came, shows: Shows::Nothing };
        }
        turn.reply(&token_for)?;

        Ok(())
    }
}

/// Where an engine came from is what its token was minted for.
impl From<Purpose> for Came {
    fn from(purpose: Purpose) -> Self {
        match purpose {
            Purpose::External { .. } => Self::External,
            Purpose::Spawn { spawned, binary, hash } => Self::Spawned { spawned, binary, hash },
        }
    }
}

impl Engines {
    /// The token `token` minted, with the arguments that start an engine linking to this hub at `port` with it.
    fn minted(&self, token: Token, port: u16) -> Minted {
        Minted { token, args: self.link_args(token, port) }
    }

    /// The arguments that link an engine to this hub at `port` with `token`: `--hub`, the host engines dial,
    /// `--port`, and `--hub-token`. A token minted for an engine started outside the hub is answered with them, and a
    /// spawned engine is started with them after the agent's own.
    fn link_args(&self, token: Token, port: u16) -> Vec<String> {
        vec![
            String::from("--hub"),
            self.settings.host.clone(),
            String::from("--port"),
            port.to_string(),
            String::from("--hub-token"),
            token.to_string(),
        ]
    }
}

impl Linked {
    /// What the agent is shown of it, as engine `engine`: nothing until it is linked and has told what it shows.
    fn listing(&self, engine: u32) -> Option<EngineListing> {
        let Standing::Linked { came, shows: Shows::Named { name, tools } } = &self.standing else {
            return None;
        };

        Some(EngineListing { engine, name: name.clone(), tools: *tools, origin: came.origin() })
    }
}
