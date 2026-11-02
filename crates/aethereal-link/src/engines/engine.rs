//! One engine linked: the hub's actor for one connection `Engines` accepted (ADR-0038, ADR-0040).

use super::{CallEngine, DescribeEngine, Described, Engines, Shown, TakeToken, TokenFor};
use crate::frames::{self, Written};
use crate::{Down, REVISION, Refusal, Token, Up};
use aethereal_actor::{
    Actor, ActorRef, Allows, Answer, Answered, Asks, Beginning, Carries, ChildOf, Ending, Fault, Handles, Held, Level,
    More, Number, Open, Part, Path, Streamed, Streams, Taken, ToAny, ToSender, ToType, Turn, path,
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
    /// Each call written and not yet answered, by its number.
    calls: BTreeMap<u64, Held<Called>>,
    next: u64,
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
        let up = match frames::read::<Up>(&value.bytes, self.most_bytes) {
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
                if let Some(held) = self.calls.remove(&call) {
                    turn.answer(held, &called)?;
                }
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

    /// Holds the call and writes it, or refuses it at once: before the engine has sent its tools, past the most calls
    /// out, or when it would not fit in a frame.
    fn handle(&mut self, asked: CallEngine, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let fate = self.fate(asked)?;
        let call = match fate {
            Fate::Writes(call) => call,
            Fate::Refused(why) => {
                turn.reply(&Called::Refused(String::from(why)))?;
                return Ok(());
            }
        };
        let held = turn.hold()?;
        self.calls.insert(self.next, held);
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
    /// What becomes of `asked`, numbered as the next call.
    fn fate(&self, asked: CallEngine) -> Result<Fate, Fault> {
        let Said::Linked { tools: Described::Tools { .. }, .. } = self.said else {
            return Ok(Fate::Refused("the engine has not sent its tools"));
        };
        let room = usize::try_from(self.most_calls).unwrap_or(usize::MAX);
        let is_full = self.calls.len() >= room;
        if is_full {
            return Ok(Fate::Refused("too many calls of the engine are out"));
        }
        let CallEngine { revision, tool, input } = asked;
        let call = Down::Call { call: self.next, revision, tool, input };
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

    /// Writes the frame `bytes`, noting whether it closes once they are sent.
    fn write<S, I>(&self, turn: &mut Turn<'_, Self, S>, bytes: Vec<u8>, closes: bool) -> Result<(), Fault>
    where
        S: Allows<Asks<Writing, Write, Sent>, I>,
    {
        turn.to(self.connection.protocol::<Writing, _>()).ask::<_, _, I>(&Write { bytes }, &Sent { closes })?;

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
