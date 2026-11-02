//! The hub's end of the link: the root that listens for engines, and keeps one actor for each linked (ADR-0038).

mod asks;
mod engine;
mod settings;

pub use asks::{CallEngine, DescribeEngine, Described, EngineList, EngineListing, ListEngines, Shown};
pub use engine::{Engine, EngineConfig, Sent};
pub use settings::EnginesSettings;

use crate::read_limit;
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Asks, Beginning, Carries, ChildOf, Ended, Ending, Fault, Handles, Level, More,
    Number, Open, Part, Path, Streamed, Streams, Taken, ToAny, ToSender, ToType, Top, Turn, path,
};
use aethereal_data::{Bound, Plain};
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};
use aethereal_tcp::{Accept, Accepted, Accepting, Framing, Listen, Listening, Shut, Tcp, Unbound};
use std::collections::BTreeMap;
use std::num::NonZeroU32;

/// What the root keeps for its listen: nothing, since it asks one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.binding")]
pub struct Binding;

/// Why the root failed: `Tcp` gave it no port, or its listener stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.unlistened")]
pub struct Unlistened;

/// Where the root is in listening.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Port {
    /// `Tcp` has not answered.
    Asked,
    /// `Tcp` bound this port.
    Bound(u16),
}

/// What the agent is shown of one engine.
#[derive(Debug, Plain)]
enum Shows {
    /// Nothing: it has not said `Hello`.
    Nothing,
    /// The name it gave, and how many tools it holds.
    Named { name: String, tools: u32 },
}

/// An engine linked: its actor, and what it shows.
#[derive(Bound)]
struct Linked {
    engine: ActorRef<Engine>,
    shows: Shows,
}

/// The connections the root's listener may accept ahead of the root spawning their engines.
const ACCEPTING_WINDOW: NonZeroU32 = NonZeroU32::MIN.saturating_add(3);

/// The hub's end of the link (ADR-0038): listens on its port through `Tcp`, and for each connection accepted, spawns an
/// [`Engine`] beneath itself, keyed by a count it keeps, which is the engine's id for the agent. An id is a `u32`,
/// whose JSON form is a number an agent writes as it reads it (ADR-0007, ADR-0038), and an engine's key is that id. An actor asks it to
/// [`ListEngines`] at its typed path, `path::<Engines>(())`, and asks an engine itself at `path::<Engine>(id)`.
///
/// Each engine tells it the name it gave and how many tools it holds as they change, so its list is answered at once,
/// and it watches each, dropping one from its list when it ends.
#[derive(Bound)]
#[kind(name = "aether.link.engines")]
pub struct Engines {
    settings: EnginesSettings,
    port: Port,
    accepting: Streams<Accepting, ()>,
    /// Each engine linked, by its id.
    linked: BTreeMap<u32, Linked>,
    next: u32,
}

impl Engines {
    /// The port listened on: none until `Tcp` has bound it.
    #[must_use]
    pub const fn port(&self) -> Option<u16> {
        match self.port {
            Port::Asked => None,
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

/// It stands up on every host, and refuses to where a frame its engines would read could not be read.
impl<H: Host> Bootstrap<H> for Engines {
    const SECTION: &'static str = "engines";
    type Settings = EnginesSettings;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(settings: EnginesSettings, (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
        let checked = read_limit::check("engines", cx.read_limit(), settings.most_bytes);
        checked.map_err(|why| Unbuilt::Failed(Box::new(why)))?;

        cx.begin(&(), settings)
    }
}

impl Actor for Engines {
    type Config = EnginesSettings;
    type Key = ();
    type Handles = (Answer<Listening, Binding>, Part<Accepting>, Answer<Unbound, Number>, Ended, Shown, ListEngines);
    type BeginSends = (Asks<Path<Tcp>, Listen, Binding>,);
    type EndSends = ();

    /// Asks `Tcp` to listen on its port.
    fn begin(settings: EnginesSettings, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let listen =
            Listen { port: settings.port, framing: Framing::LengthPrefixed { most_bytes: settings.most_bytes } };
        turn.to(path::<Tcp>(())).ask(&listen, &Binding)?;

        Ok(Self {
            settings,
            port: Port::Asked,
            accepting: Streams::new(ACCEPTING_WINDOW),
            linked: BTreeMap::new(),
            next: 0,
        })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Answer<Listening, Binding>> for Engines {
    type Sends = (Asks<Streamed<Accepting>, Open<Accepting>, Number>, ToAny<Streamed<Accepting>, More>);

    /// Bound, it accepts on its listener; otherwise it fails.
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
        self.port = Port::Bound(port);

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
        self.linked.insert(id, Linked { engine, shows: Shows::Nothing });

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

    fn handle(&mut self, ended: Ended, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let gone = ended.actor().id();
        self.linked.retain(|_, linked| linked.engine.id() != gone);

        Ok(())
    }
}

impl Handles<Shown> for Engines {
    type Sends = ();

    /// What one of its own engines shows: from any other actor, it is passed over.
    fn handle(&mut self, shown: Shown, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let sender = turn.sender().id();
        let linked = self.linked.values_mut().find(|linked| linked.engine.id() == sender);
        if let Some(linked) = linked {
            linked.shows = Shows::Named { name: shown.name, tools: shown.tools };
        }

        Ok(())
    }
}

impl Handles<ListEngines> for Engines {
    type Sends = (ToSender<EngineList>,);

    fn handle(&mut self, _: ListEngines, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let engines = self
            .linked
            .iter()
            .filter_map(|(&engine, linked)| match &linked.shows {
                Shows::Named { name, tools } => Some(EngineListing { engine, name: name.clone(), tools: *tools }),
                Shows::Nothing => None,
            })
            .collect();
        turn.reply(&EngineList { engines })?;

        Ok(())
    }
}
