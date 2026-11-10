//! The engine's end of the link: the root that dials its hub once, hands the hub the whole set of the engine's tools
//! after each change, asks the engine's tools for each call the hub sends, and ends its program when the link ends
//! (ADR-0038, ADR-0040, ADR-0053).

mod settings;

pub use settings::{LinkConfig, LinkSettings, NoToken};

use crate::frames::{self, Written};
use crate::limits;
use crate::{Down, Outcome, REVISION, Refusal, Up};
use aethereal_actor::{
    Actor, ActorRef, Allows, Answer, Answered, Asks, Beginning, ChildOf, Due, Ending, Fault, Handles, Level, More,
    Number, Open, Part, Path, Streamed, Streams, Taken, Tells, ToAny, ToType, Top, Turn, Unanswered, path,
};
use aethereal_data::{Bound, Plain, to_bytes};
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};
use aethereal_tcp::{Connect, Connected, Dialed, Framing, HungUp, Read, Reading, Shut, Tcp, Write, Writing, Wrote};
use aethereal_tools::{CallTool, Called, Sheets, Subscribe, Tools};
use std::collections::BTreeMap;
use std::num::NonZeroU32;

/// What the root keeps for its dial: nothing, as it has one out at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.connecting")]
pub struct Connecting;

/// What the root keeps for a write: the stream of the connection it was written to, so the answer of a write once the
/// link has ended is passed over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.sending")]
pub struct Sending {
    reader: Number,
}

/// What the root keeps for a call it asked of the engine's tools: which it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.calling")]
pub struct Calling {
    number: u64,
}

/// Where the root is in reaching its hub.
#[derive(Debug, Bound)]
enum Line {
    /// Its one dial is out.
    Dialing,
    /// Connected: the connection, and the number of the stream it is read through.
    Open { connection: ActorRef<Dialed>, reader: Number },
    /// The dial came to nothing, or the connection ended: the root has asked its program to stop, and writes nothing.
    Ended,
}

impl Set {
    /// Whether the set last heard is the set of `revision`.
    const fn is_of(&self, revision: u64) -> bool {
        match self {
            Self::Unheard => false,
            Self::Heard(sheets) => sheets.revision == revision,
        }
    }
}

impl Line {
    /// Whether the connection open is the one read through the stream `number`.
    fn is_read_by(&self, number: Number) -> bool {
        match self {
            Self::Open { reader, .. } => *reader == number,
            Self::Dialing | Self::Ended => false,
        }
    }
}

/// What the root has heard of the engine's tools.
#[derive(Debug, Plain)]
enum Set {
    /// `Tools` has not yet sent the set: none is written, and every call is refused.
    Unheard,
    /// The set `Tools` last sent.
    Heard(Sheets),
}

/// The frames of its connection the root is sent ahead of reading them.
const READING_WINDOW: NonZeroU32 = NonZeroU32::MIN.saturating_add(3);

/// The engine's end of the link (ADR-0038, ADR-0040, ADR-0053). It holds no tools: it [`Subscribe`]s to the engine's
/// [`Tools`], listed before it, as it begins. It dials its hub once, as it begins, writes [`Up::Hello`] with the token
/// its hub minted and then [`Up::Sheets`], the set `Tools` last sent with `Tools`'s revision, and writes the whole set
/// again each time `Tools` sends it another. Each [`Down::Call`] of the set it last wrote is asked of `Tools` as a
/// [`CallTool`] within the time the call has left, from when its frame is read, and its answer written back as
/// [`Up::Called`]: a call not answered by then is written back refused (ADR-0041). A tool's offer never waits on the
/// hub.
///
/// No engine outlives its link: when the dial comes to nothing, the connection ends, a frame from the hub does not
/// read, or the hub refuses the engine, the root asks its program to stop (`turn.quit`, ADR-0020). Its token is spent,
/// and a hub that restarted holds none of its own to take, so it could never link again.
///
/// Mail it cannot read never fails it (ADR-0017): a tool's answer past the engine's `read_limit` is written back
/// refused. It will not start where a frame could be such mail, which would be dropped unread, and a program sets
/// `read_limit` above what its tools answer, so that each is served.
#[derive(Bound)]
#[kind(name = "aether.link")]
pub struct Link {
    config: LinkConfig,
    set: Set,
    line: Line,
    reading: Streams<Reading, ()>,
    /// Each call of the open connection asked of `Tools`, by the root's own number, with the hub's number for it.
    calls: BTreeMap<u64, u64>,
    next: u64,
}

impl ChildOf for Link {
    type Parent = Top;
}

/// It stands up on every host, and refuses to where a frame it would read could not be read, or where its settings
/// hold no token.
impl<H: Host> Bootstrap<H> for Link {
    const SECTION: &'static str = "link";
    type Settings = LinkSettings;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(settings: LinkSettings, (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
        limits::check("link", cx.read_limit(), cx.blob_bytes(), settings.most_bytes)?;
        let config = settings.config().map_err(|why| Unbuilt::Failed(Box::new(why)))?;

        cx.begin(&(), config)
    }
}

impl Actor for Link {
    type Config = LinkConfig;
    type Key = ();
    type Handles = (
        Sheets,
        Answer<Connected, Connecting>,
        Part<Reading>,
        Answer<HungUp, Number>,
        Answer<Wrote, Sending>,
        Answer<Called, Calling>,
    );
    type BeginSends = (Tells<Path<Tools>, Subscribe>, Asks<Path<Tcp>, Connect, Connecting>);
    type EndSends = ();

    /// Subscribes to the engine's tools, and dials its hub.
    fn begin(config: LinkConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.to(path::<Tools>(())).send(&Subscribe)?;
        turn.to(path::<Tcp>(())).ask(&connect(&config), &Connecting)?;

        Ok(Self {
            config,
            set: Set::Unheard,
            line: Line::Dialing,
            reading: Streams::new(READING_WINDOW),
            calls: BTreeMap::new(),
            next: 0,
        })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Sheets> for Link {
    type Sends = (Asks<Writing, Write, Sending>,);

    /// The engine's tools as `Tools` now holds them: kept, and written to the hub when connected.
    fn handle(&mut self, sheets: Sheets, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.set = Set::Heard(sheets);

        self.write_sheets(turn)
    }
}

impl Handles<Answer<Connected, Connecting>> for Link {
    type Sends =
        (Asks<Streamed<Reading>, Open<Reading>, Number>, ToAny<Streamed<Reading>, More>, Asks<Writing, Write, Sending>);

    /// Connected, it reads the connection and writes `Hello`, with its token, and the set; otherwise the link has
    /// ended.
    fn handle(
        &mut self,
        (answered, Connecting): (Answered<Connected>, Connecting),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Connected::Open { connection, .. }) = answered else {
            turn.log(Level::Warn, format_args!("the hub was not reached: {answered:?}"));
            self.end_link(turn);
            return Ok(());
        };
        let reader = self.reading.open(turn, connection.protocol(), Read, ())?;
        self.line = Line::Open { connection, reader };
        let hello = Up::Hello { revision: REVISION, name: self.config.name.clone(), token: self.config.token };
        self.write(turn, to_bytes(&hello)?)?;

        self.write_sheets(turn)
    }
}

impl Handles<Part<Reading>> for Link {
    type Sends = (
        ToAny<Streamed<Reading>, More>,
        Asks<Writing, Write, Sending>,
        Asks<Path<Tools>, CallTool, Calling>,
        ToType<Dialed, Shut>,
    );

    /// A frame from the hub: a call, asked of the engine's tools, or a refusal, which ends the link, as a frame that does not
    /// read does.
    fn handle(&mut self, part: Part<Reading>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Some(Taken { number, value, .. }) = self.reading.part(turn, part)? else {
            return Ok(());
        };
        if !self.line.is_read_by(number) {
            return Ok(());
        }
        let down = match frames::read::<Down>(turn.read(&value.bytes), self.config.most_bytes) {
            Ok(down) => down,
            Err(why) => {
                turn.log(Level::Warn, format_args!("a frame from the hub did not read: {why:?}"));
                return self.hang_up(turn);
            }
        };
        match down {
            Down::Refused(Refusal::Revision { revision }) => {
                turn.log(Level::Warn, format_args!("the hub speaks revision {revision}, and this engine {REVISION}"));
                self.hang_up(turn)
            }
            Down::Refused(Refusal::Token) => {
                turn.log(Level::Warn, "the hub refused this engine's token: it holds no such token");
                self.hang_up(turn)
            }
            Down::Call { call, revision, tool, input, within } => {
                let Ok(input) = turn.put(input) else {
                    let unheld = Called::Refused(String::from("the engine had no room to hold the input"));
                    return self.answer(turn, call, unheld);
                };
                let due = within.due(turn.now());
                self.call(turn, call, revision, due, &CallTool { tool, input })
            }
        }
    }
}

impl Handles<Answer<HungUp, Number>> for Link {
    type Sends = ();

    /// The connection's stream has ended: when it is the open connection's, the link has ended.
    fn handle(
        &mut self,
        (answered, number): (Answered<HungUp>, Number),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        self.reading.answered(number);
        if !self.line.is_read_by(number) {
            return Ok(());
        }
        turn.log(Level::Info, format_args!("the hub's connection ended: {answered:?}"));
        self.end_link(turn);

        Ok(())
    }
}

impl Handles<Answer<Wrote, Sending>> for Link {
    type Sends = (ToType<Dialed, Shut>,);

    /// A write to the open connection that was not sent ends the link.
    fn handle(
        &mut self,
        (answered, sending): (Answered<Wrote>, Sending),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let is_sent = answered == Answered::Yes(Wrote::Sent);
        let is_current = self.line.is_read_by(sending.reader);
        let ends = is_current && !is_sent;
        if !ends {
            return Ok(());
        }
        turn.log(Level::Info, format_args!("a frame to the hub was not sent: {answered:?}"));

        self.hang_up(turn)
    }
}

impl Handles<Answer<Called, Calling>> for Link {
    type Sends = (Asks<Writing, Write, Sending>,);

    /// What `Tools` answered, written to the hub: one that comes once the link has ended is dropped.
    fn handle(
        &mut self,
        (answered, Calling { number }): (Answered<Called>, Calling),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Some(call) = self.calls.remove(&number) else {
            return Ok(());
        };
        let called = match answered {
            Answered::Yes(called) => called,
            Answered::No(Unanswered::Expired) => Called::Refused(String::from("the tool did not answer in time")),
            Answered::No(why) => Called::Refused(format!("the tool did not answer: {why:?}")),
        };

        self.answer(turn, call, called)
    }
}

impl Link {
    /// The hub's call `call` of `revision`: refused when it is not the set last heard, and asked of `Tools`
    /// otherwise, due as `due` says, so a tool that never answers is refused at the time the hub stops waiting
    /// (ADR-0041). `Tools` refuses a tool it does not hold.
    fn call<S, IW, IC>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        call: u64,
        revision: u64,
        due: Due,
        asked: &CallTool,
    ) -> Result<(), Fault>
    where
        S: Allows<Asks<Writing, Write, Sending>, IW> + Allows<Asks<Path<Tools>, CallTool, Calling>, IC>,
    {
        let is_current = self.set.is_of(revision);
        if !is_current {
            let changed = Called::Refused(String::from("the engine's tools have changed: describe it again"));
            return self.answer::<_, IW>(turn, call, changed);
        }
        let number = self.next;
        self.next += 1;
        turn.to(path::<Tools>(())).within(due).ask::<_, _, IC>(asked, &Calling { number })?;
        self.calls.insert(number, call);

        Ok(())
    }

    /// Writes what the call `call` came to, its output read into the frame, or a refusal in its place when it would not
    /// fit in one.
    fn answer<S, I>(&mut self, turn: &mut Turn<'_, Self, S>, call: u64, called: Called) -> Result<(), Fault>
    where
        S: Allows<Asks<Writing, Write, Sending>, I>,
    {
        let called = Outcome::of(called, turn);
        let written = frames::written(&Up::Called { call, called }, self.config.most_bytes)?;
        let bytes = match written {
            Written::Fits(bytes) => bytes,
            Written::TooLarge => {
                let refused = Outcome::Refused(String::from("the tool's answer is too large for the link"));
                to_bytes(&Up::Called { call, called: refused })?
            }
        };

        self.write::<_, I>(turn, bytes)
    }

    /// Writes the set `Tools` last sent, when it has sent one and the root is connected. A set too large for a frame
    /// is not written, and the hub keeps the set before it, whose calls are then refused as changed.
    fn write_sheets<S, I>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<Asks<Writing, Write, Sending>, I>,
    {
        let Set::Heard(Sheets { revision, tools }) = &self.set else {
            return Ok(());
        };
        let up = Up::Sheets { revision: *revision, tools: tools.clone() };
        let written = frames::written(&up, self.config.most_bytes)?;
        let Written::Fits(bytes) = written else {
            turn.log(Level::Error, "the engine's tools do not fit in a frame, and were not written");
            return Ok(());
        };

        self.write::<_, I>(turn, bytes)
    }

    /// Writes the frame `bytes`, put as a blob, to the open connection: with none open, nothing is written. Before the
    /// dial is answered, the connection is written the whole set once it opens; after the link has ended, nothing is. A
    /// frame the engine has no room to hold ends the link, as a write not sent does.
    fn write<S, I>(&mut self, turn: &mut Turn<'_, Self, S>, bytes: Vec<u8>) -> Result<(), Fault>
    where
        S: Allows<Asks<Writing, Write, Sending>, I>,
    {
        let Line::Open { connection, reader } = self.line else {
            return Ok(());
        };
        let Ok(frame) = turn.put(bytes) else {
            turn.log(Level::Warn, "the engine had no room to hold a frame to the hub, and the link ends");
            self.end_link(turn);
            return Ok(());
        };
        turn.to(connection.protocol::<Writing, _>())
            .ask::<_, _, I>(&Write { parts: vec![frame] }, &Sending { reader })?;

        Ok(())
    }

    /// Shuts the open connection, and ends the link.
    fn hang_up<S, I>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<ToType<Dialed, Shut>, I>,
    {
        if let Line::Open { connection, .. } = self.line {
            turn.to(connection).send::<_, I>(&Shut)?;
        }
        self.end_link(turn);

        Ok(())
    }

    /// Ends the link: asks the program to stop, and forgets the calls of the connection, whose answers are then
    /// dropped.
    fn end_link<S>(&mut self, turn: &mut Turn<'_, Self, S>) {
        turn.quit();
        self.line = Line::Ended;
        self.calls.clear();
    }
}

/// What dials the hub `config` names.
fn connect(config: &LinkConfig) -> Connect {
    Connect {
        host: config.hub.clone(),
        port: config.port,
        framing: Framing::LengthPrefixed { most_bytes: config.most_bytes },
    }
}
