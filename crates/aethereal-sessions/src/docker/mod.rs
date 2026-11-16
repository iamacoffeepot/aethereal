//! The Docker client (ADR-0057): one root, [`Docker`], which asks Docker's Engine API over its Unix socket for the
//! containers the hub's sessions run in.
//!
//! An actor asks it at its typed path, `path::<Docker>(())`, to [`Create`] a container from a [`ContainerSpec`], to
//! [`Start`], [`Stop`], [`Kill`], or [`Remove`] one, to [`Inspect`] its state, or to [`List`] the containers that carry
//! a label, and is answered once Docker has. It opens an [`Attaching`] stream to a container's stdin and output, a
//! [`Logging`] stream of what a container wrote from a time on, or a [`Watching`] stream of the events of the
//! containers that carry a label. Each request goes on a connection of its own, which `Http` dials to the socket and
//! the client shuts once its reply is read, so a stream that runs for hours holds up nothing else.
//!
//! **Nothing is passed through.** Every request is one of these, written by the client from typed fields, and no
//! request, path, query, or JSON from its asker reaches Docker: only a [`ContainerSpec`] makes a container, and a
//! container is named only by its id, as Docker gives one, never by a name or a path. Access to Docker's socket is
//! access to this machine, so what a container is made of must come from the hub's own config, and the hub reaches the
//! socket only through this client (ADR-0057).
//!
//! Every path names the API version [`API_VERSION`], so a daemon answers in the shapes this client reads. A container
//! is made with no terminal, so its output comes on an attach and in its logs in Docker's frames, each chunk behind a
//! header naming its stream, which the client takes apart into stdout and stderr.
//!
//! The client times nothing. A request answered whole is asked of `Http`'s client within its asker's deadline, so an
//! asker that states one bounds the whole exchange (ADR-0041); a stream runs until its asker stops it.
//!
//! The root needs nothing of its host but `Http`, which it is listed after. Its settings, the section `docker`, name
//! the socket.

mod asks;
mod demux;
mod events;
mod json;
mod relay;
mod reply;
mod request;
mod settings;
mod spec;
mod streams;
mod whole;

pub use asks::{
    Acted, ContainerId, ContainerState, Create, Created, DockerError, Inspect, Inspected, Kill, Label, List, Listed,
    Listing, Phase, Remove, Start, Stop, Unwritable,
};
pub use request::API_VERSION;
pub use settings::{DockerConfig, DockerSettings, SocketUnaddressable};
pub use spec::{Access, Bind, ContainerSpec, Network, Stdin, Variable};
pub use streams::{
    Attach, Attached, Attaching, Detached, Event, Events, EventsDone, Logging, Logs, LogsDone, Output, Std, Until,
    Watching,
};

use aethereal_actor::{
    Actor, Allows, Answer, Answered, Asks, Beginning, Carries, ChildOf, Ended, Ending, Enough, Fault, Handles, More,
    Number, Open, Part, Path, Protocol, Serving, Streamed, Streams, ToAny, ToType, Top, Turn, path,
};
use aethereal_data::{Bound, Plain};
use aethereal_http::{Client, Connect, Connected, Fetch, Fetched, Finished, Http, Peer, Receiving, Unreached};
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};
use aethereal_tcp::{Shut, SocketPath};
use relay::Relay;
use std::collections::BTreeMap;
use std::num::NonZeroU32;
use whole::Whole;

/// What the client keeps for a connection it asked `Http` to dial: which request or stream it is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.docker.dialing")]
pub struct Dialing {
    number: u64,
}

/// What the client keeps for a request it asked of `Http`'s client: which request it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.docker.calling")]
pub struct Calling {
    number: u64,
}

/// What the client asks `Http`'s client for a request answered whole.
pub struct Fetches;

impl Protocol for Fetches {
    type Kinds = (Fetch,);
}

impl Carries<Fetch> for Fetches {
    type Answers = (Fetched,);
}

/// A dial asked of `Http`.
type Dial = Asks<Path<Http>, Connect, Dialing>;

/// A request asked of `Http`'s client, answered whole.
type Fetching = Asks<Fetches, Fetch, Calling>;

/// A hang-up of `Http`'s client, once what it was dialed for is done.
type HangUp = ToType<Client, Shut>;

/// A stream of a response opened on `Http`'s client.
type Following = Asks<Streamed<Receiving>, Open<Receiving>, Number>;

/// Credit granted to `Http`'s client for a stream of a response.
type Credit = ToAny<Streamed<Receiving>, More>;

/// A stream of a response stopped.
type Stopping = ToAny<Streamed<Receiving>, Enough>;

/// What the client sends as `Http` answers a dial: the request asked, or the stream opened, on the client it dialed,
/// and that client's hang-up when nothing waits on it.
type Connecting = (Fetching, HangUp, Following, Credit);

/// What the client sends as it relays a stream: parts to its asker, and to `Http`'s client credit, a stop, and a
/// hang-up.
type Relaying = (
    ToAny<(Part<Attaching>,), Part<Attaching>>,
    ToAny<(Part<Logging>,), Part<Logging>>,
    ToAny<(Part<Watching>,), Part<Watching>>,
    Credit,
    Stopping,
    HangUp,
);

/// The Docker client: asks Docker's Engine API, over its Unix socket, for what the hub's sessions need of their
/// containers, as typed requests and streams, and nothing else (ADR-0057).
///
/// It watches each actor that opens a stream, and when one ends, hangs up that stream's connection.
#[derive(Bound)]
#[kind(name = "aether.docker")]
pub struct Docker {
    socket: SocketPath,
    /// The requests answered whole, by their number, each until its reply is read.
    wholes: BTreeMap<u64, Whole>,
    /// The streams relayed, by their number, each until its asker is answered.
    relays: BTreeMap<u64, Relay>,
    /// The streams of responses opened on `Http`'s clients, each kept with the number of the stream it is relayed to.
    receiving: Streams<Receiving, u64>,
    attaching: Serving<Attaching>,
    logging: Serving<Logging>,
    watching: Serving<Watching>,
    next: u64,
}

/// It stands up on every host from its settings, and refuses to when its socket's path names no socket.
impl<H: Host> Bootstrap<H> for Docker {
    const SECTION: &'static str = "docker";
    type Settings = DockerSettings;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(settings: DockerSettings, (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
        let config = settings.config().map_err(|why| Unbuilt::Failed(Box::new(why)))?;

        cx.begin(&(), config)
    }
}

impl ChildOf for Docker {
    type Parent = Top;
}

impl Actor for Docker {
    type Config = DockerConfig;
    type Key = ();
    type Handles = (
        Create,
        Start,
        Stop,
        Kill,
        Remove,
        Inspect,
        List,
        Open<Attaching>,
        Open<Logging>,
        Open<Watching>,
        More,
        Enough,
        Ended,
        Answer<Connected, Dialing>,
        Answer<Fetched, Calling>,
        Part<Receiving>,
        Answer<Finished, Number>,
    );
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: DockerConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        let window = NonZeroU32::new(config.window).unwrap_or(NonZeroU32::MIN);

        Ok(Self {
            socket: config.socket,
            wholes: BTreeMap::new(),
            relays: BTreeMap::new(),
            receiving: Streams::new(window),
            attaching: Serving::new(),
            logging: Serving::new(),
            watching: Serving::new(),
            next: 0,
        })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Answer<Connected, Dialing>> for Docker {
    type Sends = Connecting;

    /// A connection `Http` dialed: the request it was for is asked on it, or the stream opened, and a connection
    /// nothing waits on any more is hung up. A dial that failed answers what it was for.
    fn handle(
        &mut self,
        (answered, Dialing { number }): (Answered<Connected>, Dialing),
        turn: &mut Turn<'_, Self, Connecting>,
    ) -> Result<(), Fault> {
        let reached = match answered {
            Answered::Yes(Connected::Open { client }) => Ok(client),
            Answered::Yes(Connected::Unreached(why)) => Err(DockerError::Unreachable(why)),
            Answered::Yes(Connected::Full) => Err(DockerError::Unreachable(Unreached::Full)),
            Answered::Yes(Connected::Unavailable) | Answered::No(_) => {
                Err(DockerError::Unreachable(Unreached::Unavailable))
            }
        };
        if let Some(whole) = self.wholes.remove(&number) {
            return self.fetch(turn, number, whole, reached);
        }
        if let Some(relay) = self.relays.remove(&number) {
            return self.follow(turn, number, relay, reached);
        }
        if let Ok(client) = reached {
            // What it was dialed for has ended: its asker stopped or ended first.
            turn.to(client).send(&Shut)?;
        }

        Ok(())
    }
}

impl Handles<Ended> for Docker {
    type Sends = (Stopping,);

    /// An actor that opened streams has ended: each of its streams' connections is hung up, as nothing will take what
    /// comes on it.
    fn handle(&mut self, ended: Ended, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let attaches = self.attaching.ended(&ended).into_iter().map(|stream| relay::Served::Attach(stream.key()));
        let logs = self.logging.ended(&ended).into_iter().map(|stream| relay::Served::Logs(stream.key()));
        let events = self.watching.ended(&ended).into_iter().map(|stream| relay::Served::Events(stream.key()));
        let gone: Vec<relay::Served> = attaches.chain(logs).chain(events).collect();
        for served in gone {
            self.drop_relay(turn, served)?;
        }

        Ok(())
    }
}

impl Docker {
    /// Asks `Http` to dial Docker's socket for a request or a stream, and answers the number it is kept by.
    fn dial<S, I>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<u64, Fault>
    where
        S: Allows<Dial, I>,
    {
        let number = self.next;
        self.next += 1;
        let connect = Connect { peer: Peer::Unix { path: self.socket.clone() } };
        turn.to(path::<Http>(())).ask::<_, _, I>(&connect, &Dialing { number })?;

        Ok(number)
    }
}
