//! The streams the Docker client relays (ADR-0017, ADR-0057): each a response `Http`'s client streams on a connection
//! of its own, taken apart into typed parts and passed on to the stream's asker as its credit allows.
//!
//! The client takes each part of the response holding its credit back, and grants it once everything taken has been
//! passed on, so an asker that falls behind holds Docker back, a window of reads at most behind it.

use super::demux::{Demux, Piece};
use super::events::Lines;
use super::request::Call;
use super::whole::outgoing;
use super::{
    Attached, Attaching, Connecting, Detached, Dial, Docker, DockerError, Event, EventsDone, Logging, LogsDone, Output,
    Relaying, Stopping, Unwritable, Watching, reply,
};
use aethereal_actor::{
    ActorRef, Allows, Answer, Answered, Enough, Fault, Handles, More, Number, Open, Part, ProtocolRef, Serving, Stream,
    StreamKey, Taken, ToAny, ToSender, Turn, Unsent,
};
use aethereal_data::{Bound, Plain};
use aethereal_http::{Client, Failure, Finished, Follow, Received, Receiving, Switch};
use aethereal_tcp::{HungUp, Shut, Writing};
use core::mem;
use std::collections::VecDeque;
use std::num::NonZeroU32;

/// The most bytes of an error's body kept as it is read: past what Docker writes for one.
const MOST_ERROR_BYTES: usize = 64 * 1024;

/// A stream relayed: the stream served to its asker, where its response is, and the credit held back.
#[derive(Bound)]
pub(super) struct Relay {
    downstream: Downstream,
    flow: Flow,
    /// The parts taken from `Http`'s client whose credit is not yet granted back: granted once everything taken from
    /// them has been passed on.
    withheld: u32,
}

/// The stream served to the asker, what reads the response into its parts, and the parts waiting for its credit.
#[derive(Bound)]
enum Downstream {
    Attach { key: StreamKey, demux: Demux, queue: VecDeque<Passing> },
    Logs { key: StreamKey, demux: Demux, queue: VecDeque<Piece> },
    Events { key: StreamKey, lines: Lines, queue: VecDeque<Event> },
}

/// Which stream a relay serves: its kind and its key, as its asker numbered it among its streams of that kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub(super) enum Served {
    Attach(StreamKey),
    Logs(StreamKey),
    Events(StreamKey),
}

/// A part of an attach waiting for its asker's credit.
#[derive(Bound)]
enum Passing {
    Stdin(ProtocolRef<Writing>),
    Piece(Piece),
}

/// Where a relayed stream's response is.
#[derive(Bound)]
enum Flow {
    /// `Http` dials the socket, and the request waits to be written.
    Dialing(Call),
    /// Its response is streamed by `client` as the stream `upstream`.
    Following { client: ActorRef<Client>, upstream: Number },
    /// Docker refused it with `status`: the body is gathered, as far as it is kept, for the error it says.
    Refused { client: ActorRef<Client>, upstream: Number, status: u16, body: Vec<u8> },
    /// Its response has ended, as `End` says: its asker is answered once every part waiting is passed on.
    Ending(End),
}

/// How a relayed response ended.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
enum End {
    /// Its body ended where its framing said.
    Whole,
    /// Its connection hung up, as `HungUp` says.
    HungUp(HungUp),
    Failed(DockerError),
}

/// Whether the parts waiting were passed on.
enum Passed {
    /// Every one.
    Drained,
    /// Not all: the asker has no credit.
    Waiting,
    /// The asker's stream is no longer served.
    Unserved,
    /// A part could not be made.
    Broke(DockerError),
}

impl Handles<Open<Attaching>> for Docker {
    type Sends = (ToSender<Detached>, Dial);

    fn handle(&mut self, open: Open<Attaching>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let (key, attach) = self.attaching.open(turn, open)?;
        let downstream = Downstream::Attach { key, demux: Demux::new(), queue: VecDeque::new() };

        self.relay(turn, downstream, attach.call())
    }
}

impl Handles<Open<Logging>> for Docker {
    type Sends = (ToSender<LogsDone>, Dial);

    fn handle(&mut self, open: Open<Logging>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let (key, logs) = self.logging.open(turn, open)?;
        let downstream = Downstream::Logs { key, demux: Demux::new(), queue: VecDeque::new() };

        self.relay(turn, downstream, logs.call())
    }
}

impl Handles<Open<Watching>> for Docker {
    type Sends = (ToSender<EventsDone>, Dial);

    fn handle(&mut self, open: Open<Watching>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let (key, events) = self.watching.open(turn, open)?;
        let downstream = Downstream::Events { key, lines: Lines::new(), queue: VecDeque::new() };

        self.relay(turn, downstream, events.call())
    }
}

impl Handles<More> for Docker {
    type Sends = Relaying;

    /// An asker grants a stream more credit: what waits for it is passed on.
    fn handle(&mut self, more: More, turn: &mut Turn<'_, Self, Relaying>) -> Result<(), Fault> {
        let attach = self.attaching.more(turn, &more).map(Served::Attach);
        let logs = self.logging.more(turn, &more).map(Served::Logs);
        let events = self.watching.more(turn, &more).map(Served::Events);
        let Some(served) = attach.or(logs).or(events) else {
            return Ok(());
        };
        let Some(number) = self.relay_of(served) else {
            return Ok(());
        };

        self.flush(turn, number)
    }
}

impl Handles<Enough> for Docker {
    type Sends = (Stopping,);

    /// An asker stops a stream: it is answered at once, and the response's connection hung up.
    fn handle(&mut self, enough: Enough, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let served = if let Some(stream) = self.attaching.stop(turn, &enough) {
            let served = Served::Attach(stream.key());
            stream.finish(turn, &Detached::HungUp(HungUp::Shut))?;
            served
        } else if let Some(stream) = self.logging.stop(turn, &enough) {
            let served = Served::Logs(stream.key());
            stream.finish(turn, &LogsDone::Stopped)?;
            served
        } else if let Some(stream) = self.watching.stop(turn, &enough) {
            let served = Served::Events(stream.key());
            stream.finish(turn, &EventsDone::Stopped)?;
            served
        } else {
            return Ok(());
        };

        self.drop_relay(turn, served)
    }
}

impl Handles<Part<Receiving>> for Docker {
    type Sends = Relaying;

    /// A part of a response `Http`'s client streams: taken apart into the parts of the stream it is relayed to, its
    /// credit held back until they are passed on.
    fn handle(&mut self, part: Part<Receiving>, turn: &mut Turn<'_, Self, Relaying>) -> Result<(), Fault> {
        let Some(Taken { value, kept, .. }) = self.receiving.part_held(turn, part) else {
            return Ok(());
        };
        let number = *kept;
        let Some(relay) = self.relays.get_mut(&number) else {
            return Ok(());
        };
        relay.withheld += 1;
        if let Err(error) = relay.take(turn, value) {
            self.break_off(turn, number, error)?;
        }

        self.flush(turn, number)
    }
}

impl Handles<Answer<Finished, Number>> for Docker {
    type Sends = Relaying;

    /// A response `Http`'s client streamed has ended: its client is hung up, and the stream it was relayed to is
    /// answered once every part waiting is passed on.
    fn handle(
        &mut self,
        (answered, upstream): (Answered<Finished>, Number),
        turn: &mut Turn<'_, Self, Relaying>,
    ) -> Result<(), Fault> {
        let Some(number) = self.receiving.answered(upstream) else {
            return Ok(());
        };
        let Some(relay) = self.relays.get_mut(&number) else {
            return Ok(());
        };
        let end = match mem::replace(&mut relay.flow, Flow::Ending(End::Whole)) {
            Flow::Following { client, .. } => {
                turn.to(client).send(&Shut)?;
                match answered {
                    Answered::Yes(Finished::Whole) => End::Whole,
                    Answered::Yes(Finished::Closed(how)) => End::HungUp(how),
                    Answered::Yes(Finished::Failed(failure)) => End::Failed(DockerError::Broken(failure)),
                    Answered::No(why) => End::Failed(DockerError::Unanswered(why)),
                }
            }
            Flow::Refused { client, status, body, .. } => {
                turn.to(client).send(&Shut)?;
                End::Failed(reply::error(status, &body))
            }
            Flow::Ending(end) => end,
            Flow::Dialing(_) => End::Failed(DockerError::Malformed),
        };
        relay.flow = Flow::Ending(end);

        self.flush(turn, number)
    }
}

impl Docker {
    /// Asks `Http` to dial the socket for the stream `downstream` serves, whose request is `call`, or answers the
    /// stream why it could not be written.
    fn relay<S, IS>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        downstream: Downstream,
        call: Result<Call, Unwritable>,
    ) -> Result<(), Fault>
    where
        S: Allows<Dial, IS>,
    {
        match call {
            Ok(call) => {
                let number = self.dial::<_, IS>(turn)?;
                self.relays.insert(number, Relay { downstream, flow: Flow::Dialing(call), withheld: 0 });
            }
            Err(why) => self.conclude(turn, downstream.served(), End::Failed(DockerError::Unwritable(why)))?,
        }

        Ok(())
    }

    /// Opens the stream of the response `relay` waits on, on the client `Http` dialed for it, or answers its asker why
    /// `Http` could not dial.
    pub(super) fn follow(
        &mut self,
        turn: &mut Turn<'_, Self, Connecting>,
        number: u64,
        mut relay: Relay,
        reached: Result<ActorRef<Client>, DockerError>,
    ) -> Result<(), Fault> {
        let served = relay.downstream.served();
        let client = match reached {
            Ok(client) => client,
            Err(error) => return self.conclude(turn, served, End::Failed(error)),
        };
        let Flow::Dialing(call) = mem::replace(&mut relay.flow, Flow::Ending(End::Whole)) else {
            turn.to(client).send(&Shut)?;
            return Ok(());
        };
        let Ok(request) = outgoing(turn, call) else {
            turn.to(client).send(&Shut)?;
            return self.conclude(turn, served, End::Failed(DockerError::Broken(Failure::NoRoom)));
        };
        let switch = match served {
            Served::Attach(_) => Switch::To { protocol: String::from("tcp") },
            Served::Logs(_) | Served::Events(_) => Switch::Stay,
        };
        let upstream = self.receiving.open(turn, client.protocol(), Follow { request, switch }, number)?;
        relay.flow = Flow::Following { client, upstream };
        self.relays.insert(number, relay);

        Ok(())
    }

    /// The number of the relay that serves `served`: none when no relay does.
    fn relay_of(&self, served: Served) -> Option<u64> {
        self.relays.iter().find(|(_, relay)| relay.downstream.served() == served).map(|(&number, _)| number)
    }

    /// Drops the relay that serves `served`, whose asker has stopped it or ended, hanging up its response's
    /// connection.
    pub(super) fn drop_relay<S, I>(&mut self, turn: &mut Turn<'_, Self, S>, served: Served) -> Result<(), Fault>
    where
        S: Allows<Stopping, I>,
    {
        let Some(number) = self.relay_of(served) else {
            return Ok(());
        };
        let Some(relay) = self.relays.remove(&number) else {
            return Ok(());
        };
        if let Flow::Following { upstream, .. } | Flow::Refused { upstream, .. } = relay.flow {
            self.receiving.stop::<_, _, I>(turn, upstream)?;
        }

        Ok(())
    }

    /// Ends the relay `number` early with `error`: its response's connection is hung up, and its asker answered once
    /// the parts before it are passed on.
    fn break_off(&mut self, turn: &mut Turn<'_, Self, Relaying>, number: u64, error: DockerError) -> Result<(), Fault> {
        let Some(relay) = self.relays.get_mut(&number) else {
            return Ok(());
        };
        if let Flow::Following { upstream, .. } | Flow::Refused { upstream, .. } = relay.flow {
            self.receiving.stop(turn, upstream)?;
        }
        relay.flow = Flow::Ending(End::Failed(error));

        Ok(())
    }

    /// Passes on what waits in the relay `number` while its asker has credit. Once nothing waits, the credit held back
    /// is granted to `Http`'s client, or, when the response has ended, the asker is answered.
    fn flush(&mut self, turn: &mut Turn<'_, Self, Relaying>, number: u64) -> Result<(), Fault> {
        let Some(relay) = self.relays.get_mut(&number) else {
            return Ok(());
        };
        let served = relay.downstream.served();
        let passed = match &mut relay.downstream {
            Downstream::Attach { key, queue, .. } => pass(&mut self.attaching, turn, *key, queue)?,
            Downstream::Logs { key, queue, .. } => pass(&mut self.logging, turn, *key, queue)?,
            Downstream::Events { key, queue, .. } => pass(&mut self.watching, turn, *key, queue)?,
        };
        match passed {
            Passed::Waiting => Ok(()),
            Passed::Unserved => self.drop_relay(turn, served),
            Passed::Broke(error) => {
                // The engine has no room for the parts' bytes, so none that wait can be passed on.
                if let Some(relay) = self.relays.get_mut(&number) {
                    relay.downstream.clear();
                }
                self.break_off(turn, number, error)?;
                self.drained(turn, number)
            }
            Passed::Drained => self.drained(turn, number),
        }
    }

    /// Nothing waits in the relay `number` any more: the credit held back is granted, or its asker answered.
    fn drained(&mut self, turn: &mut Turn<'_, Self, Relaying>, number: u64) -> Result<(), Fault> {
        let Some(relay) = self.relays.get_mut(&number) else {
            return Ok(());
        };
        match &relay.flow {
            Flow::Following { upstream, .. } | Flow::Refused { upstream, .. } => {
                if let Some(withheld) = NonZeroU32::new(relay.withheld) {
                    self.receiving.grant(turn, *upstream, withheld)?;
                }
                relay.withheld = 0;
            }
            Flow::Ending(end) => {
                let end = end.clone();
                let served = relay.downstream.served();
                self.relays.remove(&number);
                self.conclude(turn, served, end)?;
            }
            Flow::Dialing(_) => {}
        }

        Ok(())
    }

    /// Answers the stream `served` with how its response ended, as that stream's kind says it.
    fn conclude<S>(&mut self, turn: &mut Turn<'_, Self, S>, served: Served, end: End) -> Result<(), Fault> {
        match served {
            Served::Attach(key) => {
                if let Some(stream) = self.attaching.take(key) {
                    stream.finish(turn, &end.detached())?;
                }
            }
            Served::Logs(key) => {
                if let Some(stream) = self.logging.take(key) {
                    stream.finish(turn, &end.logs())?;
                }
            }
            Served::Events(key) => {
                if let Some(stream) = self.watching.take(key) {
                    stream.finish(turn, &end.events())?;
                }
            }
        }

        Ok(())
    }
}

impl Relay {
    /// Takes `received`, the next part of the response: a head goes on to its body, or to the error it says, and bytes
    /// are taken apart into parts that wait for the asker's credit.
    ///
    /// # Errors
    ///
    /// Why the response can be read no further.
    fn take<S>(&mut self, turn: &Turn<'_, Docker, S>, received: Received) -> Result<(), DockerError> {
        match received {
            Received::Head(head) => self.headed(head.status),
            Received::Switched { writing, .. } => self.switched(writing),
            Received::Bytes(bytes) => self.body(turn.read(&bytes)),
        }
    }

    /// Goes on from a head of `status`: to its body, or, for an error, to gathering what it says.
    fn headed(&mut self, status: u16) -> Result<(), DockerError> {
        let Flow::Following { client, upstream } = self.flow else {
            return Ok(());
        };
        let is_success = (200..300).contains(&status);
        if !is_success {
            self.flow = Flow::Refused { client, upstream, status, body: Vec::new() };
            return Ok(());
        }
        // An attach that Docker answers with a body has not taken the connection.
        match self.downstream {
            Downstream::Attach { .. } => Err(DockerError::Malformed),
            Downstream::Logs { .. } | Downstream::Events { .. } => Ok(()),
        }
    }

    /// Docker took the connection, which `writing` writes: an attach's first part.
    fn switched(&mut self, writing: ProtocolRef<Writing>) -> Result<(), DockerError> {
        let is_following = matches!(self.flow, Flow::Following { .. });
        if !is_following {
            return Ok(());
        }
        match &mut self.downstream {
            Downstream::Attach { queue, .. } => {
                queue.push_back(Passing::Stdin(writing));
                Ok(())
            }
            Downstream::Logs { .. } | Downstream::Events { .. } => Err(DockerError::Malformed),
        }
    }

    /// Takes `bytes` of the response's body: read into parts, or gathered into the error it says.
    fn body(&mut self, bytes: &[u8]) -> Result<(), DockerError> {
        match &mut self.flow {
            Flow::Following { .. } => self.downstream.read(bytes),
            Flow::Refused { body, .. } => {
                let room = MOST_ERROR_BYTES.saturating_sub(body.len());
                body.extend_from_slice(bytes.get(..room).unwrap_or(bytes));
                Ok(())
            }
            Flow::Dialing(_) | Flow::Ending(_) => Ok(()),
        }
    }
}

impl Downstream {
    /// Which stream it is.
    const fn served(&self) -> Served {
        match self {
            Self::Attach { key, .. } => Served::Attach(*key),
            Self::Logs { key, .. } => Served::Logs(*key),
            Self::Events { key, .. } => Served::Events(*key),
        }
    }

    /// Lets go of every part that waits.
    fn clear(&mut self) {
        match self {
            Self::Attach { queue, .. } => queue.clear(),
            Self::Logs { queue, .. } => queue.clear(),
            Self::Events { queue, .. } => queue.clear(),
        }
    }

    /// Reads `bytes` of the response's body into the parts that wait for the asker's credit.
    fn read(&mut self, bytes: &[u8]) -> Result<(), DockerError> {
        match self {
            Self::Attach { demux, queue, .. } => {
                let mut pieces = Vec::new();
                let read = demux.push(bytes, &mut pieces);
                queue.extend(pieces.into_iter().map(Passing::Piece));
                read
            }
            Self::Logs { demux, queue, .. } => {
                let mut pieces = Vec::new();
                let read = demux.push(bytes, &mut pieces);
                queue.extend(pieces);
                read
            }
            Self::Events { lines, queue, .. } => {
                let mut events = Vec::new();
                let read = lines.push(bytes, &mut events);
                queue.extend(events);
                read
            }
        }
    }
}

impl End {
    /// How an attach ended. A response that ends where its framing says never took the connection.
    fn detached(self) -> Detached {
        match self {
            Self::Whole => Detached::Failed(DockerError::Malformed),
            Self::HungUp(how) => Detached::HungUp(how),
            Self::Failed(error) => Detached::Failed(error),
        }
    }

    /// How a container's logs ended: a body that runs until Docker hangs up is read when it does.
    fn logs(self) -> LogsDone {
        match self {
            Self::Whole | Self::HungUp(HungUp::Peer) => LogsDone::Read,
            Self::HungUp(HungUp::Shut) => LogsDone::Stopped,
            Self::HungUp(how) => LogsDone::Failed(DockerError::Broken(Failure::HungUp(how))),
            Self::Failed(error) => LogsDone::Failed(error),
        }
    }

    /// How the events ended.
    fn events(self) -> EventsDone {
        match self {
            Self::Whole | Self::HungUp(HungUp::Peer) => EventsDone::Closed,
            Self::HungUp(HungUp::Shut) => EventsDone::Stopped,
            Self::HungUp(how) => EventsDone::Failed(DockerError::Broken(Failure::HungUp(how))),
            Self::Failed(error) => EventsDone::Failed(error),
        }
    }
}

/// A part waiting for a stream's credit, made into the part the stream sends.
trait Pass<S: Stream> {
    /// The part, its bytes put as a blob.
    ///
    /// # Errors
    ///
    /// The engine had no room for its bytes.
    fn part<T>(self, turn: &mut Turn<'_, Docker, T>) -> Result<S::Part, DockerError>;
}

impl Pass<Attaching> for Passing {
    fn part<T>(self, turn: &mut Turn<'_, Docker, T>) -> Result<Attached, DockerError> {
        match self {
            Self::Stdin(writing) => Ok(Attached::Stdin(writing)),
            Self::Piece(piece) => Ok(Attached::Output(<Piece as Pass<Logging>>::part(piece, turn)?)),
        }
    }
}

impl Pass<Logging> for Piece {
    fn part<T>(self, turn: &mut Turn<'_, Docker, T>) -> Result<Output, DockerError> {
        let Self { stream, bytes } = self;
        let bytes = turn.put(bytes).map_err(|_| DockerError::Broken(Failure::NoRoom))?;

        Ok(Output { stream, bytes })
    }
}

impl Pass<Watching> for Event {
    fn part<T>(self, _: &mut Turn<'_, Docker, T>) -> Result<Self, DockerError> {
        Ok(self)
    }
}

/// Passes what waits in `queue` on to the stream `key` of those `serving` holds, while it has credit.
fn pass<S, Q, I>(
    serving: &mut Serving<S>,
    turn: &mut Turn<'_, Docker, Relaying>,
    key: StreamKey,
    queue: &mut VecDeque<Q>,
) -> Result<Passed, Fault>
where
    S: Stream,
    Q: Pass<S>,
    Relaying: Allows<ToAny<(Part<S>,), Part<S>>, I>,
{
    loop {
        let Some(stream) = serving.get_mut(key) else {
            return Ok(Passed::Unserved);
        };
        let has_credit = stream.credit() > 0;
        if !has_credit {
            let waits = !queue.is_empty();
            let passed = if waits {
                Passed::Waiting
            } else {
                Passed::Drained
            };
            return Ok(passed);
        }
        let Some(next) = queue.pop_front() else {
            return Ok(Passed::Drained);
        };
        let part = match next.part(turn) {
            Ok(part) => part,
            Err(error) => return Ok(Passed::Broke(error)),
        };
        match stream.send::<_, _, I>(turn, part) {
            Ok(()) => {}
            Err(Unsent::Unwritable(error)) => return Err(error.into()),
            Err(Unsent::NoCredit(_)) => return Ok(Passed::Waiting),
        }
    }
}
