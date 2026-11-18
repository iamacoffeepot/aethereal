//! The hub's end of the link: the root that listens for engines, mints the tokens they link with, keeps one actor
//! for each linked, and spawns engines from the store (ADR-0038, ADR-0040).

mod asks;
mod engine;
mod hearing;
mod randomness;
mod settings;
mod spawned;
mod spawning;
mod tokens;

pub use asks::{
    CallEngine, DescribeEngine, Described, EngineList, EngineListing, EngineTrust, ExitedEngine, ListEngines, Origin,
    Shown, TrustOf,
};
pub use engine::{ChatTo, Chatting, Engine, EngineConfig, Lapsing, Sent, Vouching};
pub use hearing::{HearTrusted, Hearing, TrustedEngine};

pub use settings::EnginesSettings;
pub use spawned::{Booting, Fetching, LinkedAs, Patience, Runs, Spawned, SpawnedConfig};
pub use spawning::{Spawn, SpawnEngine, SpawnFor, TerminateEngine, Terminated};
pub use tokens::{MintFor, MintToken, Minted, Minting, TakeToken, TokenFor};

use crate::{Token, Trust};
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anyone, Beginning, Carries, ChildOf, Ended, Ending, Fault, Handles, Held, Level,
    Number, Part, Receives, Streams, Subscribers, Taken, Top, Turn, path,
};
use aethereal_data::{Bound, Plain};
use aethereal_process::Exited;
use aethereal_store::{Binary, StoreHash};
use aethereal_tcp::{Accept, Accepting, Framing, Interface, Listen, Listening, Shut, Tcp, Unbound};
use aethereal_tools::Caller;
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
    /// A [`MintFor`]'s, answered with the token and the port.
    Minted { held: Held<Minted>, token: Token },
    /// A [`SpawnFor`]'s, whose engine is spawned for `by` to link with the token at the port.
    Spawn { held: Held<Spawn>, spawn: SpawnEngine, token: Token, by: Caller },
}

/// What the agent is shown of one engine whose token was taken.
#[derive(Debug, Plain)]
enum Shows {
    /// Nothing: it has not told what it shows.
    Nothing,
    /// The name it gave, and how many tools it holds.
    Named { name: String, tools: u32 },
}

/// Where an engine whose token was taken came from, and who asked for it.
#[derive(Bound)]
enum Came {
    /// It was started outside the hub, with a token minted for `by`.
    External { by: Caller },
    /// The hub spawned it for `by`: its spawn, the binary the spawn was asked for, and the hash of the build it runs.
    Spawned { spawned: ActorRef<Spawned>, binary: Binary, hash: StoreHash, by: Caller },
}

impl Came {
    /// Whether the hub trusts it: only when the owner asked for it (ADR-0057).
    fn trust(&self) -> Trust {
        match self {
            Self::External { by } | Self::Spawned { by, .. } => Trust::from(*by),
        }
    }

    /// What its token was for, as the engine that brought it is answered.
    fn token_for(&self) -> TokenFor {
        let trust = self.trust();
        match self {
            Self::External { .. } => TokenFor::External { trust },
            Self::Spawned { .. } => TokenFor::Spawned { trust },
        }
    }

    /// Where it came from, as the agent is shown.
    fn origin(&self) -> Origin {
        match self {
            Self::External { .. } => Origin::External,
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
/// [`TakeToken`], and is listed only once it has. A token is 16 bytes of the host's randomness, drawn by a job. One
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
/// An actor that takes what trusted engines say asks it to [`HearTrusted`], and is told each engine the hub trusts as
/// a [`TrustedEngine`]: those linked already as it asks, and each after as its token is taken (ADR-0057). Neither it nor
/// its engines name what hears them.
///
/// It stands up on a host that serves it randomness: each native host's crate bootstraps it, serving the system's,
/// and checks its settings with [`EnginesSettings::check`].
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
    /// The actors told each engine the hub trusts.
    hearers: Subscribers<Hearing>,
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
impl Carries<MintFor> for Engines {
    type Answers = (Minted,);
}

/// Asked at its typed path, it answers at once.
impl Carries<TakeToken> for Engines {
    type Answers = (TokenFor,);
}

/// Asked at its typed path, it answers at once.
impl Carries<TrustOf> for Engines {
    type Answers = (EngineTrust,);
}

/// Asked at its typed path, it answers once the engine has linked, or what came of it before.
impl Carries<SpawnFor> for Engines {
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
        TrustOf,
        MintFor,
        Answer<Drawn, Minting>,
        TakeToken,
        SpawnFor,
        Runs,
        Spawn,
        Exited,
        TerminateEngine,
        HearTrusted,
    );
    type Depends = (Tcp,);

    /// Asks `Tcp` to listen on its port.
    fn begin(settings: EnginesSettings, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let framing = Framing::LengthPrefixed { most_bytes: settings.most_bytes };
        let listen = Listen { port: settings.port, interface: Interface::Settings, framing };
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
            hearers: Subscribers::new(),
        })
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Receives<Answer<Listening, Binding>> for Engines {
    /// Bound, it accepts on its listener, answers each token minted meanwhile, and spawns each engine asked for
    /// meanwhile; otherwise it fails, and each request is answered that it went unanswered.
    fn handle(
        &mut self,
        (answered, Binding): (Answered<Listening>, Binding),
        turn: &mut Turn<'_, Self>,
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
                Waiting::Spawn { held, spawn, token, by } => self.spawn(turn, held, spawn, token, port, by)?,
            }
        }

        Ok(())
    }
}

impl Handles<Part<Accepting>> for Engines {
    type From = Anyone;

    /// A connection accepted: an engine is spawned for it, unless the root holds its most, when it is shut.
    fn handle(&mut self, part: Part<Accepting>, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
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
        let id = self.next;
        let (most_calls, most_bytes) = (self.settings.most_calls, self.settings.most_bytes);
        let config = EngineConfig { id, connection, most_calls, most_bytes };
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

impl Receives<Answer<Unbound, Number>> for Engines {
    /// Its listener stopped: no engine can link again, and it fails.
    fn handle(
        &mut self,
        (answered, number): (Answered<Unbound>, Number),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        self.accepting.answered(number);
        turn.log(Level::Error, format_args!("the listener for engines stopped: {answered:?}"));

        Err(Fault::new(&Unlistened)?)
    }
}

impl Receives<Ended> for Engines {
    /// An engine or a spawn has ended: an engine leaves its list, and a spawn's token goes with it. An actor that
    /// heard of trusted engines has ended, and is let go.
    fn handle(&mut self, ended: Ended, _: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let gone = ended.actor().id();
        self.linked.retain(|_, linked| linked.engine.id() != gone);
        self.spawn_ended(gone);
        self.hearers.ended(&ended);

        Ok(())
    }
}

impl Handles<Shown> for Engines {
    type From = Anyone;

    /// What one of its own engines shows, once its token is taken: from any other actor, it is passed over.
    fn handle(&mut self, shown: Shown, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let sender = turn.sender();
        let linked = self.linked.values_mut().find(|linked| linked.engine.id() == sender.id());
        if let Some(Linked { standing: Standing::Linked { shows, .. }, .. }) = linked {
            *shows = Shows::Named { name: shown.name, tools: shown.tools };
        }

        Ok(())
    }
}

impl Handles<ListEngines> for Engines {
    type From = Anyone;

    fn handle(&mut self, _: ListEngines, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let engines = self.linked.iter().filter_map(|(&engine, linked)| linked.listing(engine)).collect();
        turn.reply(&EngineList { engines, exited: self.exited.clone() })?;

        Ok(())
    }
}

impl Handles<TrustOf> for Engines {
    type From = Anyone;

    /// The trust of the engine linked under the id asked, decided as its token was taken: an engine whose token has
    /// not been taken is not linked.
    fn handle(&mut self, asked: TrustOf, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let trust = self.linked.get(&asked.engine).map_or(EngineTrust::NotLinked, Linked::trust);
        turn.reply(&trust)?;

        Ok(())
    }
}

impl Handles<MintFor> for Engines {
    type From = Anyone;

    /// Holds the request, and draws a token for it from the host's randomness.
    fn handle(&mut self, mint: MintFor, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let held = turn.hold()?;
        turn.run(&Draw, &Minting::External { held, by: mint.by })?;

        Ok(())
    }
}

impl Receives<Answer<Drawn, Minting>> for Engines {
    /// A token drawn: for an engine started outside the hub, kept in the table, and answered with the arguments to
    /// start it with, or kept waiting for the port; for a spawn, its engine spawned, or kept waiting for the port. A
    /// draw that came to nothing answers a spawn `Refused`, and a token's request nothing, and the engine tells its
    /// asker it went unanswered.
    fn handle(
        &mut self,
        (answered, minting): (Answered<Drawn>, Minting),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Drawn::Token(token)) = answered else {
            turn.log(Level::Error, format_args!("no token was drawn: {answered:?}"));
            if let Minting::Spawn { held, .. } = minting {
                turn.answer(held, &Spawn::Refused)?;
            }
            return Ok(());
        };
        let (held, by) = match minting {
            Minting::External { held, by } => (held, by),
            Minting::Spawn { held, spawn, by } => return self.spawn_drawn(turn, held, spawn, token, by),
        };
        let within = Duration::from_millis(self.settings.token_within_millis);
        let most = usize::try_from(self.settings.most_tokens).unwrap_or(usize::MAX);
        self.tokens.mint_external(token, turn.now(), within, most, by);
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
    type From = Anyone;

    /// Takes the token out of the table: what it was for and whether its engine is trusted, or that it is unknown,
    /// once used, expired, or its spawn ended too. The engine that asked is linked from then on, with where it came
    /// from, and a spawn's told its id; one the hub trusts is told, after its answer, to each actor that hears of
    /// trusted engines. Only an engine connected here takes a token: any other sender is answered that it is unknown,
    /// and the token stays in the table for its engine.
    fn handle(&mut self, take: TakeToken, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let sender = turn.sender();
        let linked = self.linked.iter_mut().find(|(_, linked)| linked.engine.id() == sender.id());
        let Some((&engine, linked)) = linked else {
            turn.log(Level::Warn, format_args!("{sender:?}, which is no engine connected here, asked to take a token"));
            turn.reply(&TokenFor::Unknown)?;
            return Ok(());
        };
        let Some(purpose) = self.tokens.take(take.token, turn.now()) else {
            turn.reply(&TokenFor::Unknown)?;
            return Ok(());
        };
        let came = Came::from(purpose);
        let token_for = came.token_for();
        if let Came::Spawned { spawned, .. } = &came {
            turn.to(*spawned).send(&LinkedAs { engine })?;
        }
        let trusted = came.trust().is_trusted();
        linked.standing = Standing::Linked { came, shows: Shows::Nothing };
        turn.reply(&token_for)?;
        if trusted {
            turn.to(&self.hearers).send(&TrustedEngine { engine: linked.engine })?;
        }

        Ok(())
    }
}

/// Where an engine came from is what its token was minted for.
impl From<Purpose> for Came {
    fn from(purpose: Purpose) -> Self {
        match purpose {
            Purpose::External { by, .. } => Self::External { by },
            Purpose::Spawn { spawned, binary, hash, by } => Self::Spawned { spawned, binary, hash, by },
        }
    }
}

impl Engines {
    /// Each engine linked that the hub trusts, in the order of their ids.
    fn trusted(&self) -> impl Iterator<Item = ActorRef<Engine>> + '_ {
        self.linked.values().filter(|linked| linked.is_trusted()).map(|linked| linked.engine)
    }

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

        Some(EngineListing { engine, name: name.clone(), tools: *tools, origin: came.origin(), trust: came.trust() })
    }

    /// Whether the hub trusts it: never before its token is taken.
    fn is_trusted(&self) -> bool {
        match &self.standing {
            Standing::Linked { came, .. } => came.trust().is_trusted(),
            Standing::Unproven => false,
        }
    }

    /// Its trust, once its token is taken.
    fn trust(&self) -> EngineTrust {
        match &self.standing {
            Standing::Linked { came, .. } => EngineTrust::Linked { trust: came.trust() },
            Standing::Unproven => EngineTrust::NotLinked,
        }
    }
}
