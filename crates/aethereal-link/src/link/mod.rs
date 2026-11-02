//! The engine's end of the link: the root that holds the engine's tools, dials its hub, and hands the hub the whole set
//! after each change (ADR-0038).

mod settings;

pub use settings::LinkSettings;

use crate::frames::{self, Written};
use crate::read_limit;
use crate::{Down, REVISION, Up};
use aethereal_actor::{
    Actor, ActorRef, Allows, Answer, Answered, Asks, Beginning, Carries, ChildOf, Ended, Ending, Fault, Handles, Level,
    More, Number, Open, Part, Path, ProtocolRef, Sleep, Slept, Streamed, Streams, Taken, ToAny, ToSender, ToType, Top,
    Turn, path,
};
use aethereal_data::{Bound, Plain, to_bytes};
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};
use aethereal_tcp::{Connect, Connected, Dialed, Framing, HungUp, Read, Reading, Shut, Tcp, Write, Writing, Wrote};
use aethereal_tools::{Call, Called, Holder, Offer, Offered, Table, ToolSheet, Tooling, Withdraw};
use core::time::Duration;
use std::collections::BTreeMap;
use std::num::NonZeroU32;

/// What the root keeps for its dial: nothing, as it has one out at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.connecting")]
pub struct Connecting;

/// What the root keeps for a write: the stream of the connection it was written to, so the answer of a write to a
/// connection that has gone is passed over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.sending")]
pub struct Sending {
    reader: Number,
}

/// What the root keeps for a call it asked of a worker: which it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.calling")]
pub struct Calling {
    number: u64,
}

/// What a sleep the root runs is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.woken")]
pub enum Woken {
    /// The wait before dialing again is over.
    Redial,
    /// A withdrawal changed what it holds in a turn that could send nothing: the set is written now.
    Resend,
}

/// Where the root is in reaching its hub.
#[derive(Debug, Bound)]
enum Line {
    /// A dial is out.
    Dialing,
    /// Connected: the connection, and the number of the stream it is read through.
    Open { connection: ActorRef<Dialed>, reader: Number },
    /// A dial came to nothing, or the connection ended: a sleep runs until the next dial.
    Waiting,
}

impl Line {
    /// Whether the connection open is the one read through the stream `number`.
    fn is_read_by(&self, number: Number) -> bool {
        match self {
            Self::Open { reader, .. } => *reader == number,
            Self::Dialing | Self::Waiting => false,
        }
    }
}

/// The frames of its connection the root is sent ahead of reading them.
const READING_WINDOW: NonZeroU32 = NonZeroU32::MIN.saturating_add(3);

/// The engine's end of the link (ADR-0038): a tools [`Holder`], which any actor may [`Offer`] tools at its typed path,
/// `path::<Link>(())`, naming a worker that speaks [`Tooling`]. It dials its hub as it begins, writes [`Up::Hello`]
/// and then [`Up::Sheets`] on each connection, and writes the whole set again after every change to what it holds.
/// Each [`Down::Call`] is asked of the tool's worker and its answer written back as [`Up::Called`].
///
/// A tool's offer never waits on the hub: the root holds its tools through every connection, and nothing in the
/// engine sees the hub go. When a dial comes to nothing, or a connection ends, it waits and dials again, each wait
/// twice the last, from the first of its setting `redial_millis` to the longest. The wait goes back to the first when
/// the hub sends its first call on a connection, which shows the hub took the engine's `Hello`: a hub that refuses
/// the engine, or hangs up at once, is dialed less and less often.
///
/// Mail it cannot read fails its turn, which ends it: it will not start where a frame could be such mail, and a
/// program sets the engine's `read_limit` above what its tools answer too.
#[derive(Bound)]
#[kind(name = "aether.link")]
pub struct Link {
    settings: LinkSettings,
    table: Table,
    /// The revision of what it holds: raised with every change, and never reset, across connections too.
    revision: u64,
    line: Line,
    reading: Streams<Reading, ()>,
    /// Each call of the open connection asked of a worker, by the root's own number, with the hub's number for it.
    calls: BTreeMap<u64, u64>,
    next: u64,
    /// The milliseconds of the next wait before dialing again.
    wait_millis: u64,
}

impl ChildOf for Link {
    type Parent = Top;
}

/// It holds the tools an engine's own program offers.
impl Holder for Link {}

/// Asked at its typed path, it answers an offer in the same turn.
impl Carries<Offer> for Link {
    type Answers = (Offered,);
}

/// It stands up on every host, and refuses to where a frame it would read could not be read.
impl<H: Host> Bootstrap<H> for Link {
    const SECTION: &'static str = "link";
    type Settings = LinkSettings;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(settings: LinkSettings, (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
        let checked = read_limit::check("link", cx.read_limit(), settings.most_bytes);
        checked.map_err(|why| Unbuilt::Failed(Box::new(why)))?;

        cx.begin(&(), settings)
    }
}

impl Actor for Link {
    type Config = LinkSettings;
    type Key = ();
    type Handles = (
        Offer,
        Withdraw,
        Ended,
        Answer<Connected, Connecting>,
        Part<Reading>,
        Answer<HungUp, Number>,
        Answer<Wrote, Sending>,
        Answer<Called, Calling>,
        Answer<Slept, Woken>,
    );
    type BeginSends = (Asks<Path<Tcp>, Connect, Connecting>,);
    type EndSends = ();

    /// Dials its hub.
    fn begin(settings: LinkSettings, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.to(path::<Tcp>(())).ask(&connect(&settings), &Connecting)?;
        let wait_millis = settings.redial_millis.0;

        Ok(Self {
            settings,
            table: Table::new(),
            revision: 0,
            line: Line::Dialing,
            reading: Streams::new(READING_WINDOW),
            calls: BTreeMap::new(),
            next: 0,
            wait_millis,
        })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Offer> for Link {
    type Sends = (ToSender<Offered>, Asks<Writing, Write, Sending>);

    /// Holds the offer, when its tools fit in the table and the set with them in a frame, and writes the set.
    fn handle(&mut self, offer: Offer, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let room = usize::try_from(self.settings.most_tools).unwrap_or(usize::MAX);
        let names: Vec<String> = offer.tools.iter().map(|sheet| sheet.name.clone()).collect();
        let worker = offer.worker;
        let taken = self.table.take(offer.tools, worker, room);
        let offered = match taken {
            Offered::Yes => self.fitted(&names, worker)?,
            refused => refused,
        };
        if offered == Offered::Yes {
            turn.watch(worker);
            self.revision += 1;
            self.write_sheets(turn)?;
        }
        turn.reply(&offered)?;

        Ok(())
    }
}

impl Handles<Withdraw> for Link {
    type Sends = ();

    /// Stops holding the tool, and, since this turn sends nothing, wakes at once to write the set.
    fn handle(&mut self, withdraw: Withdraw, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let held = self.table.len();
        self.table.withdraw(&withdraw.tool, withdraw.worker);
        let changed = self.table.len() < held;
        if changed {
            self.revision += 1;
            turn.run(&Sleep { until: turn.now() }, &Woken::Resend)?;
        }

        Ok(())
    }
}

impl Handles<Ended> for Link {
    type Sends = (Asks<Writing, Write, Sending>,);

    /// A worker has ended: its tools are dropped, and the set written.
    fn handle(&mut self, ended: Ended, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let held = self.table.len();
        self.table.ended(ended.actor());
        let changed = self.table.len() < held;
        if changed {
            self.revision += 1;
            self.write_sheets(turn)?;
        }

        Ok(())
    }
}

impl Handles<Answer<Connected, Connecting>> for Link {
    type Sends =
        (Asks<Streamed<Reading>, Open<Reading>, Number>, ToAny<Streamed<Reading>, More>, Asks<Writing, Write, Sending>);

    /// Connected, it reads the connection and writes `Hello` and the set; otherwise it waits to dial again.
    fn handle(
        &mut self,
        (answered, Connecting): (Answered<Connected>, Connecting),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Connected::Open { connection, .. }) = answered else {
            turn.log(Level::Info, format_args!("the hub was not reached: {answered:?}"));
            return self.wait(turn);
        };
        let reader = self.reading.open(turn, connection.protocol(), Read, ())?;
        self.line = Line::Open { connection, reader };
        let hello = Up::Hello { revision: REVISION, name: self.settings.name.clone() };
        self.write(turn, to_bytes(&hello)?)?;

        self.write_sheets(turn)
    }
}

impl Handles<Part<Reading>> for Link {
    type Sends = (
        ToAny<Streamed<Reading>, More>,
        Asks<Writing, Write, Sending>,
        Asks<Tooling, Call, Calling>,
        ToType<Dialed, Shut>,
    );

    /// A frame from the hub: a call, asked of its worker, or a refusal, which ends the connection, as a frame that
    /// does not read does.
    fn handle(&mut self, part: Part<Reading>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Some(Taken { number, value, .. }) = self.reading.part(turn, part)? else {
            return Ok(());
        };
        if !self.line.is_read_by(number) {
            return Ok(());
        }
        let down = match frames::read::<Down>(&value.bytes, self.settings.most_bytes) {
            Ok(down) => down,
            Err(why) => {
                turn.log(Level::Warn, format_args!("a frame from the hub did not read: {why:?}"));
                return self.hang_up(turn);
            }
        };
        match down {
            Down::Refused { revision } => {
                turn.log(Level::Warn, format_args!("the hub speaks revision {revision}, and this engine {REVISION}"));
                self.hang_up(turn)
            }
            Down::Call { call, revision, tool, input } => self.call(turn, call, revision, &Call { tool, input }),
        }
    }
}

impl Handles<Answer<HungUp, Number>> for Link {
    type Sends = ();

    /// The connection's stream has ended: when it is the open connection's, the root waits to dial again.
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

        self.wait(turn)
    }
}

impl Handles<Answer<Wrote, Sending>> for Link {
    type Sends = (ToType<Dialed, Shut>,);

    /// A write to the open connection that was not sent ends it.
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

    /// A worker's answer, written to the hub: one for a connection that has gone is dropped.
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
            Answered::No(why) => Called::Refused(format!("the tool did not answer: {why:?}")),
        };

        self.answer(turn, call, called)
    }
}

impl Handles<Answer<Slept, Woken>> for Link {
    type Sends = (Asks<Path<Tcp>, Connect, Connecting>, Asks<Writing, Write, Sending>);

    /// Dials again once its wait is over, or writes the set a withdrawal changed.
    fn handle(
        &mut self,
        (slept, woken): (Answered<Slept>, Woken),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        if let Answered::No(why) = slept {
            turn.log(Level::Error, format_args!("a sleep was not answered, for {woken:?}: {why:?}"));
            return Ok(());
        }
        match woken {
            Woken::Redial => {
                let Line::Waiting = self.line else {
                    return Ok(());
                };
                turn.to(path::<Tcp>(())).ask(&connect(&self.settings), &Connecting)?;
                self.line = Line::Dialing;
                Ok(())
            }
            Woken::Resend => self.write_sheets(turn),
        }
    }
}

impl Link {
    /// The offer just taken, of `names` for `worker`, when the set with it fits in a frame: otherwise it is taken back,
    /// and refused as `Full`.
    fn fitted(&mut self, names: &[String], worker: ProtocolRef<Tooling>) -> Result<Offered, Fault> {
        let written = frames::written(&self.sheets(self.revision + 1), self.settings.most_bytes)?;
        let Written::TooLarge = written else {
            return Ok(Offered::Yes);
        };
        for name in names {
            self.table.withdraw(name, worker);
        }

        Ok(Offered::Full)
    }

    /// Every tool it holds, as the set of `revision`.
    fn sheets(&self, revision: u64) -> Up {
        let tools = self
            .table
            .tools()
            .map(|(name, held)| ToolSheet {
                name: String::from(name),
                description: held.description.clone(),
                input: held.input.clone(),
                output: held.output.clone(),
            })
            .collect();

        Up::Sheets { revision, tools }
    }

    /// The hub's call `call` of `revision`: refused when the set has changed since or holds no such tool, and asked of
    /// the tool's worker otherwise. A call shows the hub took the engine's `Hello`, so the wait before dialing again
    /// goes back to the first.
    fn call<S, IW, IC>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        call: u64,
        revision: u64,
        asked: &Call,
    ) -> Result<(), Fault>
    where
        S: Allows<Asks<Writing, Write, Sending>, IW> + Allows<Asks<Tooling, Call, Calling>, IC>,
    {
        self.wait_millis = self.settings.redial_millis.0;
        let is_stale = revision != self.revision;
        if is_stale {
            let changed = Called::Refused(String::from("the engine's tools have changed: describe it again"));
            return self.answer::<_, IW>(turn, call, changed);
        }
        let Some(held) = self.table.tool(&asked.tool) else {
            let unknown = Called::Refused(format!("the engine holds no tool named {}", asked.tool));
            return self.answer::<_, IW>(turn, call, unknown);
        };
        let worker = held.worker;
        let number = self.next;
        self.next += 1;
        turn.to(worker).ask::<_, _, IC>(asked, &Calling { number })?;
        self.calls.insert(number, call);

        Ok(())
    }

    /// Writes what the call `call` came to, or a refusal in its place when it would not fit in a frame.
    fn answer<S, I>(&self, turn: &mut Turn<'_, Self, S>, call: u64, called: Called) -> Result<(), Fault>
    where
        S: Allows<Asks<Writing, Write, Sending>, I>,
    {
        let written = frames::written(&Up::Called { call, called }, self.settings.most_bytes)?;
        let bytes = match written {
            Written::Fits(bytes) => bytes,
            Written::TooLarge => {
                let refused = Called::Refused(String::from("the tool's answer is too large for the link"));
                to_bytes(&Up::Called { call, called: refused })?
            }
        };

        self.write::<_, I>(turn, bytes)
    }

    /// Writes every tool it holds, when connected.
    fn write_sheets<S, I>(&self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<Asks<Writing, Write, Sending>, I>,
    {
        let written = frames::written(&self.sheets(self.revision), self.settings.most_bytes)?;
        let Written::Fits(bytes) = written else {
            turn.log(Level::Error, "the tools held do not fit in a frame, and were not written");
            return Ok(());
        };

        self.write::<_, I>(turn, bytes)
    }

    /// Writes the frame `bytes` to the open connection: with none open, nothing is written, and the next connection is
    /// written the whole set.
    fn write<S, I>(&self, turn: &mut Turn<'_, Self, S>, bytes: Vec<u8>) -> Result<(), Fault>
    where
        S: Allows<Asks<Writing, Write, Sending>, I>,
    {
        let Line::Open { connection, reader } = self.line else {
            return Ok(());
        };
        turn.to(connection.protocol::<Writing, _>()).ask::<_, _, I>(&Write { bytes }, &Sending { reader })?;

        Ok(())
    }

    /// Shuts the open connection, and waits to dial again.
    fn hang_up<S, I>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<ToType<Dialed, Shut>, I>,
    {
        if let Line::Open { connection, .. } = self.line {
            turn.to(connection).send::<_, I>(&Shut)?;
        }

        self.wait(turn)
    }

    /// Forgets the calls of the connection that ended, whose answers are then dropped, and sleeps for the next wait,
    /// doubling the one after, up to the longest.
    fn wait<S>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault> {
        self.calls.clear();
        let until = turn.now() + Duration::from_millis(self.wait_millis);
        turn.run(&Sleep { until }, &Woken::Redial)?;
        self.wait_millis = self.wait_millis.saturating_mul(2).min(self.settings.redial_millis.1);
        self.line = Line::Waiting;

        Ok(())
    }
}

/// What dials the hub `settings` name.
fn connect(settings: &LinkSettings) -> Connect {
    Connect {
        host: settings.hub.clone(),
        port: settings.port,
        framing: Framing::LengthPrefixed { most_bytes: settings.most_bytes },
    }
}
