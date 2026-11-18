//! One engine linked: the hub's actor for one connection `Engines` accepted (ADR-0038, ADR-0040).

use super::{CallEngine, DescribeEngine, Described, Engines, Shown, TakeToken, TokenFor};
use crate::frames::{self, Written};
use crate::{Down, REVISION, Refusal, Token, Trust, Up, Within};
use aethereal_actor::{
    ActorRef, Answered, Beginning, Due, Ended, Fault, Held, JobId, Later, Level, Number, Part, ProtocolRef, Sleep,
    Slept, Streams, Subscribers, Taken, Time, Turn, actor, path,
};
use aethereal_data::{Bound, Plain, to_bytes};
use aethereal_sessions::{Chat, Chatted, Joined, Numbered, ShowAsk, ShowEvents, ShowTabs};
use aethereal_tcp::{Accepted, HungUp, Read, Reading, Shut, Write, Writing, Wrote};
use aethereal_tools::Called;
use core::mem;
use std::collections::BTreeMap;
use std::num::NonZeroU32;

/// What an engine is made from: its id, which is its key, the connection accepted for it, and its limits.
#[derive(Debug, Bound)]
#[kind(name = "aether.link.engine_config")]
pub struct EngineConfig {
    pub id: u32,
    pub connection: ActorRef<Accepted>,
    pub most_calls: u32,
    pub most_bytes: u32,
}

/// What a trusted engine's chat is told to: the sessions' [`Joined`] as it is trusted, and [`Chatted`] for each chat
/// frame, from the engine's own actor (ADR-0057).
pub type Chatting = (Joined, Chatted);

/// Tells an engine where its chat goes: to `to`, which is told [`Joined`] once the hub trusts the engine, at once when
/// it does already, and each chat frame after as [`Chatted`], both from the engine's own actor, so the sessions take
/// them as that engine's and no other's. An untrusted engine holds `to` and tells it nothing.
#[derive(Debug, Bound)]
#[kind(name = "aether.link.chat_to")]
pub struct ChatTo {
    pub to: ProtocolRef<Chatting>,
}

/// What an engine keeps for a write: whether it closes once the write is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.sent")]
pub struct Sent {
    closes: bool,
}

/// What an engine keeps for the take of its token: nothing, as it asks one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.vouching")]
pub struct Vouching;

/// What an engine keeps for the sleep that bounds a call it wrote: the call's number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.lapsing")]
pub struct Lapsing {
    call: u64,
}

/// A call written to the engine and not yet answered: the request it answers, and what bounds it.
#[derive(Bound)]
struct Out {
    held: Held<Called>,
    deadline: Deadline,
}

/// What bounds a call out (ADR-0041).
#[derive(Debug, Bound)]
enum Deadline {
    /// Nothing: its asker waits until the engine answers.
    Unbounded,
    /// The sleep `sleep`, run to the call's deadline, whose answer frees its slot.
    Sleeping { sleep: JobId },
}

/// What an engine has said on its connection.
#[derive(Debug, Plain)]
enum Said {
    /// Nothing: its first frame must be `Hello`.
    Nothing,
    /// `Hello`, naming itself, whose token `Engines` is asked to take, and the set it has sent since, kept until it
    /// answers.
    Vouching { name: String, tools: Described },
    /// `Hello`, its token taken, the set it has sent last, and whether the hub trusts it, as `Engines` answered the
    /// take of its token (ADR-0057).
    Linked { name: String, tools: Described, trust: Trust },
    /// It was refused: the refusal is being written, and what it sends is passed over.
    Refused,
}

/// The frames of its connection an engine is sent ahead of reading them.
const READING_WINDOW: NonZeroU32 = NonZeroU32::MIN.saturating_add(3);

/// One engine linked, beneath `Engines`, keyed by its count, which is the engine's id for the agent (ADR-0038,
/// ADR-0040): reads its connection, whose first frame must be [`Up::Hello`] of this crate's revision, keeps the latest
/// [`Up::Sheets`], and asks each [`CallEngine`] of the engine as a [`Down::Call`] with a number of its own, answering
/// it when [`Up::Called`] comes back.
///
/// A call carries the time it has left before its request's deadline (ADR-0041), and the engine's `Link` stops waiting
/// for its tool once that much has passed. This is the one hop that waits on a frame, not an ask, so nothing answers it
/// `No` at the deadline: it runs a [`Sleep`] to the deadline itself, and frees the call's slot when it is answered. A
/// `Called` that comes after is passed over, as one of no call out is.
///
/// It asks `Engines` to take the token its `Hello` brings, and tells `Engines` what it shows only once the token is
/// taken, so it is listed only then. A token `Engines` does not hold is refused ([`Refusal::Token`]), as a revision it
/// does not speak is, and the connection shut.
///
/// Its end is its connection's: it closes when the connection hangs up, or sends a frame that does not read, or that
/// is out of turn, and each call it holds is then answered `No`. Asked to [`Shut`] by `Engines`, it shuts the
/// connection and closes.
///
/// It keeps whether the hub trusts the engine, as `Engines` answers the take of its token (ADR-0057). It depends on no
/// sessions: an actor that takes the chat, in a hub that runs sessions, hears of each trusted engine from `Engines`
/// and tells it [`ChatTo`] where its chat goes, and the engine keeps each place it is told. A trusted engine tells
/// each that it [`Joined`], as it links or as it is told the place, and tells each chat frame it sends to them as
/// [`Chatted`], with the engine's id added, so a frame names no engine and none can claim to be another; what the
/// sessions show it is written down as [`Down::Sessions`], [`Down::Show`], and [`Down::Ask`]. A chat frame from an
/// engine not trusted, or not yet, or with nowhere to go, is dropped and logged, and nothing of the chat is written to
/// one not trusted.
#[derive(Bound)]
#[kind(name = "aether.link.engine")]
pub struct Engine {
    id: u32,
    connection: ActorRef<Accepted>,
    most_calls: u32,
    most_bytes: u32,
    reading: Streams<Reading, ()>,
    said: Said,
    /// Each call written and not yet answered, nor its deadline come, by its number.
    calls: BTreeMap<u64, Out>,
    next: u64,
    /// Where its chat goes, as each actor that told it [`ChatTo`] named: told only once the hub trusts it.
    chat_to: Subscribers<Chatting>,
}

impl Engine {
    /// How many calls it has written that the engine has not answered, nor their deadlines come.
    #[must_use]
    pub fn calls_out(&self) -> usize {
        self.calls.len()
    }

    /// Whether the hub trusts the engine: none until its token is taken.
    #[must_use]
    pub const fn trust(&self) -> Option<Trust> {
        match &self.said {
            Said::Linked { trust, .. } => Some(*trust),
            Said::Nothing | Said::Vouching { .. } | Said::Refused => None,
        }
    }
}

#[actor(key = u64, parent = Engines, depends(Engines))]
impl Engine {
    /// Opens its connection's stream.
    fn begin(config: EngineConfig, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let mut reading = Streams::new(READING_WINDOW);
        reading.open(turn, config.connection.protocol(), Read, ())?;

        Ok(Self {
            id: config.id,
            connection: config.connection,
            most_calls: config.most_calls,
            most_bytes: config.most_bytes,
            reading,
            said: Said::Nothing,
            calls: BTreeMap::new(),
            next: 0,
            chat_to: Subscribers::new(),
        })
    }

    /// A frame from the engine, in the order the link has them: `Hello` first, then sets, answers, and chat. A set that
    /// comes while its token is being taken is kept, and told once it is.
    #[tell]
    fn part(&mut self, part: Part<Reading>, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Some(Taken { value, .. }) = self.reading.part(turn, part)? else {
            return Ok(());
        };
        if matches!(self.said, Said::Refused) {
            return Ok(());
        }
        let up = match frames::read::<Up>(turn.read(&value.bytes), self.most_bytes) {
            Ok(up) => up,
            Err(why) => {
                turn.log(Level::Warn, format_args!("a frame from an engine did not read: {why:?}"));
                return self.shut(turn);
            }
        };
        let up = match up.into_chat() {
            Ok(chat) => return self.chatted(turn, chat),
            Err(up) => up,
        };
        match (up, &mut self.said) {
            (Up::Hello { revision, name, token }, Said::Nothing) => self.greeted(turn, revision, name, token),
            (Up::Sheets { revision, tools }, Said::Vouching { tools: kept, .. }) => {
                *kept = Described::Tools { revision, tools };
                Ok(())
            }
            (Up::Sheets { revision, tools }, Said::Linked { name, tools: kept, .. }) => {
                *kept = Described::Tools { revision, tools };
                let shown = shown(name, kept);
                turn.to(turn.parent()).send(&shown)?;
                Ok(())
            }
            (Up::Called { call, called }, Said::Linked { .. }) => {
                let Some(Out { held, deadline }) = self.calls.remove(&call) else {
                    return Ok(());
                };
                if let Deadline::Sleeping { sleep } = deadline {
                    turn.cancel(sleep);
                }
                let called = called.into_called(turn);
                turn.answer(held, &called)?;
                Ok(())
            }
            (up, _) => {
                turn.log(Level::Warn, format_args!("an engine sent a frame out of turn: {up:?}"));
                self.shut(turn)
            }
        }
    }

    /// The connection hung up: the engine has gone.
    #[answer]
    fn hung_up(&mut self, (_, number): (Answered<HungUp>, Number), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.reading.answered(number);
        turn.close();

        Ok(())
    }

    /// A write that was not sent ends the connection, as one sent that closes it does.
    #[answer]
    fn wrote(&mut self, (answered, sent): (Answered<Wrote>, Sent), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let is_sent = answered == Answered::Yes(Wrote::Sent);
        let stays = is_sent && !sent.closes;
        if stays {
            return Ok(());
        }

        self.shut(turn)
    }

    /// Its token taken, the engine is linked, trusted as `Engines` says, and `Engines` told what it shows; a trusted
    /// engine joins wherever its chat goes, to be shown the sessions. A token `Engines` does not hold is refused, and
    /// the connection shut once the refusal is sent.
    #[answer]
    fn token_for(
        &mut self,
        (answered, Vouching): (Answered<TokenFor>, Vouching),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let said = mem::replace(&mut self.said, Said::Refused);
        let Said::Vouching { name, tools } = said else {
            self.said = said;
            return Ok(());
        };
        let Answered::Yes(TokenFor::External { trust } | TokenFor::Spawned { trust }) = answered else {
            turn.log(Level::Info, format_args!("an engine's token was refused: {answered:?}"));
            return self.write(turn, to_bytes(&Down::Refused(Refusal::Token))?, true);
        };
        let shown = shown(&name, &tools);
        self.said = Said::Linked { name, tools, trust };
        turn.to(turn.parent()).send(&shown)?;
        if trust.is_trusted() {
            turn.to(&self.chat_to).send(&Joined { engine: self.id })?;
        }

        Ok(())
    }

    /// A call's deadline has come before the engine answered it, or this would have been cancelled: its slot is freed.
    /// A sleep that came to nothing frees it too, as nothing else would.
    ///
    /// Its asker has been answered `Expired` already, or is as its deadline's step runs, and the engine drops an answer
    /// that comes after that (ADR-0041). The call is answered all the same, not let go of: should this come before the
    /// engine expires the request, the asker reads why it was not called, where a request let go of would read only
    /// that it went unanswered.
    #[answer]
    fn slept(
        &mut self,
        (_, Lapsing { call }): (Answered<Slept>, Lapsing),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Some(Out { held, .. }) = self.calls.remove(&call) else {
            return Ok(());
        };
        turn.answer(held, &Called::Refused(String::from("the engine did not answer in time")))?;

        Ok(())
    }

    #[request]
    fn describe_engine(&mut self, _: DescribeEngine) -> Result<Described, Fault> {
        let described = match &self.said {
            Said::Linked { tools, .. } => tools.clone(),
            Said::Nothing | Said::Vouching { .. } | Said::Refused => Described::NotYet,
        };

        Ok(described)
    }

    /// Holds the call and writes it with the time it has left, running a sleep to its deadline when it has one, or
    /// refuses it at once: before the engine has sent its tools, past the most calls out, or when it would not fit in
    /// a frame.
    #[request]
    fn call_engine(&mut self, asked: CallEngine, turn: &mut Turn<'_, Self>) -> Result<Later<Called>, Fault> {
        let (held, later) = turn.hold();
        let input = turn.read(&asked.input).to_vec();
        let fate = self.fate(asked, input, held.due(), turn.now())?;
        let call = match fate {
            Fate::Writes(call) => call,
            Fate::Refused(why) => {
                turn.answer(held, &Called::Refused(String::from(why)))?;
                return Ok(later);
            }
        };
        let deadline = match held.due() {
            Due::Never => Deadline::Unbounded,
            Due::Before(until) => {
                Deadline::Sleeping { sleep: turn.run(&Sleep { until }, &Lapsing { call: self.next })? }
            }
        };
        self.calls.insert(self.next, Out { held, deadline });
        self.next += 1;
        self.write(turn, call, false)?;

        Ok(later)
    }

    /// `Engines` asks it to end its link, as the agent terminates an engine started outside the hub: its connection is
    /// shut, and the engine at its other end ends by the rule that no engine outlives its link (ADR-0040). From any
    /// other actor, it is passed over.
    #[tell]
    fn on_shut(&mut self, _: Shut, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let is_from_parent = turn.sender().id() == turn.parent().id();
        if !is_from_parent {
            return Ok(());
        }

        self.shut(turn)
    }

    /// Every session, written down as `Sessions`, to a trusted engine alone.
    #[tell]
    fn show_tabs(&mut self, tabs: ShowTabs, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        if !self.is_trusted() {
            turn.log(Level::Warn, format_args!("the sessions were not shown to engine {}, not trusted", self.id));
            return Ok(());
        }

        self.write_down(turn, &Down::Sessions { list: tabs.tabs })
    }

    /// Events of a session's transcript, written down as `Show`, to a trusted engine alone: as many to a frame as fit
    /// in one, and an event that fits in none, which a long tool's output can be, left out and logged.
    #[tell]
    fn show_events(&mut self, shown: ShowEvents, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        if !self.is_trusted() {
            turn.log(Level::Warn, format_args!("a transcript was not shown to engine {}, not trusted", self.id));
            return Ok(());
        }
        let ShowEvents { session, events } = shown;
        let mut pending: Vec<Vec<Numbered>> = vec![events];
        while let Some(events) = pending.pop() {
            let frame = Down::Show { session, events };
            let bytes = match frames::written(&frame, self.most_bytes)? {
                Written::Fits(bytes) => bytes,
                Written::TooLarge => {
                    let Down::Show { events, .. } = frame else {
                        continue;
                    };
                    pending.extend(halves(events, self.id, turn));
                    continue;
                }
            };
            self.write(turn, bytes, false)?;
        }

        Ok(())
    }

    /// An ask that waits for the owner, written down as `Ask`, to a trusted engine alone.
    #[tell]
    fn show_ask(&mut self, ask: ShowAsk, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        if !self.is_trusted() {
            turn.log(Level::Warn, format_args!("an ask was not shown to engine {}, not trusted", self.id));
            return Ok(());
        }
        let ShowAsk { ask, session, what } = ask;

        self.write_down(turn, &Down::Ask { ask, session, what })
    }

    /// Keeps where its chat goes, and joins there at once when the hub trusts the engine already; a place it keeps
    /// already is not joined again. An engine not trusted, or not yet, keeps the place and tells it nothing: it joins
    /// as it is trusted, and an untrusted engine never does.
    #[tell]
    fn chat_to(&mut self, chat_to: ChatTo, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let ChatTo { to } = chat_to;
        let added = self.chat_to.subscribe(turn, to);
        let trusted = self.is_trusted();
        if added && trusted {
            turn.to(to).send(&Joined { engine: self.id })?;
        }

        Ok(())
    }

    /// A place its chat went to has ended, and is let go.
    #[ended]
    fn ended(&mut self, ended: Ended) -> Result<(), Fault> {
        self.chat_to.ended(&ended);

        Ok(())
    }
}

/// What becomes of a call asked: the frame written for it, or why it is refused.
enum Fate {
    Writes(Vec<u8>),
    Refused(&'static str),
}

impl Engine {
    /// What becomes of `asked`, whose input's bytes, read out of its blob, are `input`, numbered as the next call, due
    /// as `due` says, at `now`.
    fn fate(&self, asked: CallEngine, input: Vec<u8>, due: Due, now: Time) -> Result<Fate, Fault> {
        let Said::Linked { tools: Described::Tools { .. }, .. } = self.said else {
            return Ok(Fate::Refused("the engine has not sent its tools"));
        };
        let room = usize::try_from(self.most_calls).unwrap_or(usize::MAX);
        let is_full = self.calls.len() >= room;
        if is_full {
            return Ok(Fate::Refused("too many calls of the engine are out"));
        }
        let CallEngine { revision, tool, .. } = asked;
        let call = Down::Call { call: self.next, revision, tool, input, within: Within::left(due, now) };
        let fate = match frames::written(&call, self.most_bytes)? {
            Written::Fits(bytes) => Fate::Writes(bytes),
            Written::TooLarge => Fate::Refused("the call is too large for the link"),
        };

        Ok(fate)
    }

    /// Takes the engine's `Hello`: one of another revision is refused, and the connection shut once the refusal is
    /// sent; otherwise `Engines` is asked to take its token.
    fn greeted(&mut self, turn: &mut Turn<'_, Self>, revision: u32, name: String, token: Token) -> Result<(), Fault> {
        if revision != REVISION {
            turn.log(Level::Info, format_args!("an engine of revision {revision} was refused"));
            self.said = Said::Refused;
            let refused = Down::Refused(Refusal::Revision { revision: REVISION });
            return self.write(turn, to_bytes(&refused)?, true);
        }
        turn.to(path::<Engines>(())).ask(&TakeToken { token }, &Vouching)?;
        self.said = Said::Vouching { name, tools: Described::NotYet };

        Ok(())
    }

    /// Writes the frame `bytes`, put as a blob, noting whether it closes once they are sent. A frame the engine has no
    /// room to hold ends the connection, as a write not sent does: the engine closes, and its connection with it.
    ///
    /// A write is the connection's, not the call it may carry: one that is not sent ends the connection, so it states
    /// no deadline, where in a turn that handles a call it would inherit the call's and end the link when it expired
    /// (ADR-0041).
    fn write(&self, turn: &mut Turn<'_, Self>, bytes: Vec<u8>, closes: bool) -> Result<(), Fault> {
        let Ok(frame) = turn.put(bytes) else {
            turn.log(Level::Warn, "the hub had no room to hold a frame to an engine, and closes its connection");
            turn.close();
            return Ok(());
        };
        let connection = self.connection.protocol::<Writing>();
        turn.to(connection).within(Due::Never).ask(&Write { parts: vec![frame] }, &Sent { closes })?;

        Ok(())
    }

    /// Whether the hub trusts the engine, its token taken.
    fn is_trusted(&self) -> bool {
        self.trust().is_some_and(Trust::is_trusted)
    }

    /// What the owner did at the engine, as a chat frame says: told wherever its chat goes, with the engine's id, when
    /// the engine is trusted, and dropped and logged otherwise, so code an untrusted engine runs never acts on a session
    /// (ADR-0057). A chat frame before `Hello` is out of turn, as any frame is.
    fn chatted(&self, turn: &mut Turn<'_, Self>, chat: Chat) -> Result<(), Fault> {
        let id = self.id;
        match &self.said {
            Said::Linked { trust: Trust::Trusted, .. } => {
                if self.chat_to.references().is_empty() {
                    turn.log(Level::Info, format_args!("a chat frame from engine {id} was dropped: nothing takes it"));
                    return Ok(());
                }
                turn.to(&self.chat_to).send(&Chatted { engine: id, chat })?;
            }
            Said::Linked { trust: Trust::Untrusted { session }, .. } => {
                turn.log(Level::Warn, format_args!("a chat frame from engine {id}, session {session}'s, was dropped"));
            }
            Said::Vouching { .. } => {
                turn.log(Level::Warn, format_args!("a chat frame from engine {id}, not yet trusted, was dropped"));
            }
            Said::Refused => {}
            Said::Nothing => {
                turn.log(Level::Warn, format_args!("an engine sent a chat frame before its `Hello`: {chat:?}"));
                return self.shut(turn);
            }
        }

        Ok(())
    }

    /// Writes `frame` down the link, unless it is too large for one, which is logged and left out.
    fn write_down(&self, turn: &mut Turn<'_, Self>, frame: &Down) -> Result<(), Fault> {
        match frames::written(frame, self.most_bytes)? {
            Written::Fits(bytes) => self.write(turn, bytes, false),
            Written::TooLarge => {
                turn.log(Level::Warn, format_args!("a frame too large for the link was not written to {}", self.id));
                Ok(())
            }
        }
    }

    /// Shuts the connection, and closes.
    fn shut(&self, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        turn.to(self.connection).send(&Shut)?;
        turn.close();

        Ok(())
    }
}

/// `events`, too many for one frame, as the halves to write in their place, the later first, as they are taken from
/// the end: none for one event alone, which no frame holds, and which is logged as left out.
fn halves(mut events: Vec<Numbered>, engine: u32, turn: &mut Turn<'_, Engine>) -> Vec<Vec<Numbered>> {
    if events.len() < 2 {
        let numbers: Vec<u64> = events.iter().map(|numbered| numbered.number).collect();
        turn.log(Level::Warn, format_args!("events {numbers:?} were too large to show engine {engine}, and left out"));
        return Vec::new();
    }
    let later = events.split_off(events.len() / 2);

    vec![later, events]
}

/// What an engine named `name`, holding `tools`, shows the agent.
fn shown(name: &str, tools: &Described) -> Shown {
    let count = match tools {
        Described::Tools { tools, .. } => u32::try_from(tools.len()).unwrap_or(u32::MAX),
        Described::NotYet => 0,
    };

    Shown { name: String::from(name), tools: count }
}
