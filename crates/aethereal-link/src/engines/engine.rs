//! One engine linked: the hub's actor for one connection `Engines` accepted (ADR-0038, ADR-0040).

use super::{CallEngine, DescribeEngine, Described, Engines, Shown, TakeToken, TokenFor};
use crate::frames::{self, Written};
use crate::{Down, REVISION, Refusal, Token, Up, Within};
use aethereal_actor::{
    Actor, ActorRef, Allows, Answer, Answered, Asks, Beginning, Carries, ChildOf, Due, Ending, Fault, Handles, Held,
    JobId, Level, More, Number, Open, Part, Path, Sleep, Slept, Streamed, Streams, Taken, Time, ToAny, ToSender,
    ToType, Turn, path,
};
use aethereal_data::{Bound, Plain, to_bytes};
use aethereal_tcp::{Accepted, HungUp, Read, Reading, Shut, Write, Writing, Wrote};
use aethereal_tools::Called;
use core::mem;
use std::collections::BTreeMap;
use std::num::NonZeroU32;

/// What an engine is made from: the connection accepted for it, and its limits.
#[derive(Debug, Bound)]
#[kind(name = "aether.link.engine_config")]
pub struct EngineConfig {
    pub connection: ActorRef<Accepted>,
    pub most_calls: u32,
    pub most_bytes: u32,
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
    /// `Hello`, its token taken, and the set it has sent last.
    Linked { name: String, tools: Described },
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
#[derive(Bound)]
#[kind(name = "aether.link.engine")]
pub struct Engine {
    connection: ActorRef<Accepted>,
    most_calls: u32,
    most_bytes: u32,
    reading: Streams<Reading, ()>,
    said: Said,
    /// Each call written and not yet answered, nor its deadline come, by its number.
    calls: BTreeMap<u64, Out>,
    next: u64,
}

impl Engine {
    /// How many calls it has written that the engine has not answered, nor their deadlines come.
    #[must_use]
    pub fn calls_out(&self) -> usize {
        self.calls.len()
    }
}

impl ChildOf for Engine {
    type Parent = Engines;
}

/// Asked at its typed path, it answers with its tools at once.
impl Carries<DescribeEngine> for Engine {
    type Answers = (Described,);
}

/// Asked at its typed path, it answers once the engine has.
impl Carries<CallEngine> for Engine {
    type Answers = (Called,);
}

impl Actor for Engine {
    type Config = EngineConfig;
    type Key = u64;
    type Handles = (
        Part<Reading>,
        Answer<HungUp, Number>,
        Answer<Wrote, Sent>,
        Answer<TokenFor, Vouching>,
        Answer<Slept, Lapsing>,
        DescribeEngine,
        CallEngine,
        Shut,
    );
    type BeginSends = (Asks<Streamed<Reading>, Open<Reading>, Number>, ToAny<Streamed<Reading>, More>);
    type EndSends = ();

    /// Opens its connection's stream.
    fn begin(config: EngineConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let mut reading = Streams::new(READING_WINDOW);
        reading.open(turn, config.connection.protocol(), Read, ())?;

        Ok(Self {
            connection: config.connection,
            most_calls: config.most_calls,
            most_bytes: config.most_bytes,
            reading,
            said: Said::Nothing,
            calls: BTreeMap::new(),
            next: 0,
        })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Part<Reading>> for Engine {
    type Sends = (
        ToAny<Streamed<Reading>, More>,
        Asks<Writing, Write, Sent>,
        ToType<Accepted, Shut>,
        ToType<Engines, Shown>,
        Asks<Path<Engines>, TakeToken, Vouching>,
    );

    /// A frame from the engine, in the order the link has them: `Hello` first, then sets and answers. A set that comes
    /// while its token is being taken is kept, and told once it is.
    fn handle(&mut self, part: Part<Reading>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
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
        match (up, &mut self.said) {
            (Up::Hello { revision, name, token }, Said::Nothing) => self.greeted(turn, revision, name, token),
            (Up::Sheets { revision, tools }, Said::Vouching { tools: kept, .. }) => {
                *kept = Described::Tools { revision, tools };
                Ok(())
            }
            (Up::Sheets { revision, tools }, Said::Linked { name, tools: kept }) => {
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
}

impl Handles<Answer<HungUp, Number>> for Engine {
    type Sends = ();

    /// The connection hung up: the engine has gone.
    fn handle(&mut self, (_, number): (Answered<HungUp>, Number), turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.reading.answered(number);
        turn.close();

        Ok(())
    }
}

impl Handles<Answer<Wrote, Sent>> for Engine {
    type Sends = (ToType<Accepted, Shut>,);

    /// A write that was not sent ends the connection, as one sent that closes it does.
    fn handle(
        &mut self,
        (answered, sent): (Answered<Wrote>, Sent),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let is_sent = answered == Answered::Yes(Wrote::Sent);
        let stays = is_sent && !sent.closes;
        if stays {
            return Ok(());
        }

        self.shut(turn)
    }
}

impl Handles<Answer<TokenFor, Vouching>> for Engine {
    type Sends = (Asks<Writing, Write, Sent>, ToType<Engines, Shown>);

    /// Its token taken, the engine is linked, and `Engines` told what it shows; a token `Engines` does not hold is
    /// refused, and the connection shut once the refusal is sent.
    fn handle(
        &mut self,
        (answered, Vouching): (Answered<TokenFor>, Vouching),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let said = mem::replace(&mut self.said, Said::Refused);
        let Said::Vouching { name, tools } = said else {
            self.said = said;
            return Ok(());
        };
        let Answered::Yes(TokenFor::External | TokenFor::Spawned) = answered else {
            turn.log(Level::Info, format_args!("an engine's token was refused: {answered:?}"));
            return self.write(turn, to_bytes(&Down::Refused(Refusal::Token))?, true);
        };
        let shown = shown(&name, &tools);
        self.said = Said::Linked { name, tools };
        turn.to(turn.parent()).send(&shown)?;

        Ok(())
    }
}

impl Handles<Answer<Slept, Lapsing>> for Engine {
    type Sends = ();

    /// A call's deadline has come before the engine answered it, or this would have been cancelled: its slot is freed.
    /// A sleep that came to nothing frees it too, as nothing else would.
    ///
    /// Its asker has been answered `Expired` already, or is as its deadline's step runs, and the engine drops an answer
    /// that comes after that (ADR-0041). The call is answered all the same, not let go of: should this come before the
    /// engine expires the request, the asker reads why it was not called, where a request let go of would read only
    /// that it went unanswered.
    fn handle(
        &mut self,
        (_, Lapsing { call }): (Answered<Slept>, Lapsing),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Some(Out { held, .. }) = self.calls.remove(&call) else {
            return Ok(());
        };
        turn.answer(held, &Called::Refused(String::from("the engine did not answer in time")))?;

        Ok(())
    }
}

impl Handles<DescribeEngine> for Engine {
    type Sends = (ToSender<Described>,);

    fn handle(&mut self, _: DescribeEngine, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let described = match &self.said {
            Said::Linked { tools, .. } => tools.clone(),
            Said::Nothing | Said::Vouching { .. } | Said::Refused => Described::NotYet,
        };
        turn.reply(&described)?;

        Ok(())
    }
}

impl Handles<CallEngine> for Engine {
    type Sends = (ToSender<Called>, Asks<Writing, Write, Sent>);

    /// Holds the call and writes it with the time it has left, running a sleep to its deadline when it has one, or
    /// refuses it at once: before the engine has sent its tools, past the most calls out, or when it would not fit in
    /// a frame.
    fn handle(&mut self, asked: CallEngine, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let held = turn.hold()?;
        let input = turn.read(&asked.input).to_vec();
        let fate = self.fate(asked, input, held.due(), turn.now())?;
        let call = match fate {
            Fate::Writes(call) => call,
            Fate::Refused(why) => {
                turn.answer(held, &Called::Refused(String::from(why)))?;
                return Ok(());
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

        self.write(turn, call, false)
    }
}

impl Handles<Shut> for Engine {
    type Sends = (ToType<Accepted, Shut>,);

    /// `Engines` asks it to end its link, as the agent terminates an engine started outside the hub: its connection is
    /// shut, and the engine at its other end ends by the rule that no engine outlives its link (ADR-0040). From any
    /// other actor, it is passed over.
    fn handle(&mut self, _: Shut, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let is_from_parent = turn.sender().id() == turn.parent().id();
        if !is_from_parent {
            return Ok(());
        }

        self.shut(turn)
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
    fn greeted<S, IW, IT>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        revision: u32,
        name: String,
        token: Token,
    ) -> Result<(), Fault>
    where
        S: Allows<Asks<Writing, Write, Sent>, IW> + Allows<Asks<Path<Engines>, TakeToken, Vouching>, IT>,
    {
        if revision != REVISION {
            turn.log(Level::Info, format_args!("an engine of revision {revision} was refused"));
            self.said = Said::Refused;
            let refused = Down::Refused(Refusal::Revision { revision: REVISION });
            return self.write::<_, IW>(turn, to_bytes(&refused)?, true);
        }
        turn.to(path::<Engines>(())).ask::<_, _, IT>(&TakeToken { token }, &Vouching)?;
        self.said = Said::Vouching { name, tools: Described::NotYet };

        Ok(())
    }

    /// Writes the frame `bytes`, put as a blob, noting whether it closes once they are sent. A frame the engine has no
    /// room to hold ends the connection, as a write not sent does: the engine closes, and its connection with it.
    ///
    /// A write is the connection's, not the call it may carry: one that is not sent ends the connection, so it states
    /// no deadline, where in a turn that handles a call it would inherit the call's and end the link when it expired
    /// (ADR-0041).
    fn write<S, I>(&self, turn: &mut Turn<'_, Self, S>, bytes: Vec<u8>, closes: bool) -> Result<(), Fault>
    where
        S: Allows<Asks<Writing, Write, Sent>, I>,
    {
        let Ok(frame) = turn.put(bytes) else {
            turn.log(Level::Warn, "the hub had no room to hold a frame to an engine, and closes its connection");
            turn.close();
            return Ok(());
        };
        let connection = self.connection.protocol::<Writing, _>();
        turn.to(connection).within(Due::Never).ask::<_, _, I>(&Write { parts: vec![frame] }, &Sent { closes })?;

        Ok(())
    }

    /// Shuts the connection, and closes.
    fn shut<S, I>(&self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<ToType<Accepted, Shut>, I>,
    {
        turn.to(self.connection).send::<_, I>(&Shut)?;
        turn.close();

        Ok(())
    }
}

/// What an engine named `name`, holding `tools`, shows the agent.
fn shown(name: &str, tools: &Described) -> Shown {
    let count = match tools {
        Described::Tools { tools, .. } => u32::try_from(tools.len()).unwrap_or(u32::MAX),
        Described::NotYet => 0,
    };

    Shown { name: String::from(name), tools: count }
}
