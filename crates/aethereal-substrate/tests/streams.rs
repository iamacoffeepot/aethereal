//! Streams as a program runs them: an asker opens them through `Streams`, a receiver serves them through `Serving`, and
//! each part, credit, stop, and answer goes as mail through the engine (ADR-0017).

use Name::{A, B, L};
use aethereal_actor::{
    Actor, ActorRef, Allows, Answer, Answered, Anywhere, Asks, Beginning, ChildOf, Ended, Ending, Enough, Fault,
    Handles, More, Number, Open, Part, Seated, Serving, Stream, StreamKey, Streamed, Streams, Taken, Time, ToAny,
    ToSender, ToType, Top, Turn, Unanswered, Unsent,
};
use aethereal_data::{Bound, Plain};
use aethereal_substrate::{Clock, Engine};
use std::num::{NonZeroU32, NonZeroUsize};

/// What a file stream is opened with: how many chunks to send, and after how many sent its receiver closes.
#[derive(Plain)]
#[kind(name = "aether.test.stream.read")]
struct Read {
    chunks: u32,
    close_after: Option<u32>,
}

/// One part of a file stream: the chunk's place in its stream.
#[derive(Plain)]
#[kind(name = "aether.test.stream.chunk")]
struct Chunk {
    index: u32,
}

/// How a file stream ended.
#[derive(Debug, Plain)]
#[kind(name = "aether.test.stream.read_done")]
enum ReadDone {
    Sent,
    Stopped,
}

/// What a log stream is opened with, as a file stream is: a second protocol, its kinds of the same layouts as a
/// file stream's.
#[derive(Plain)]
#[kind(name = "aether.test.stream.tail")]
struct Tail {
    lines: u32,
    close_after: Option<u32>,
}

/// One part of a log stream.
#[derive(Plain)]
#[kind(name = "aether.test.stream.line")]
struct Line {
    index: u32,
}

/// How a log stream ended.
#[derive(Debug, Plain)]
#[kind(name = "aether.test.stream.log_done")]
enum LogDone {
    Sent,
    Stopped,
}

/// File streams.
struct Files;

impl Stream for Files {
    type Open = Read;
    type Part = Chunk;
    type Done = ReadDone;
}

/// Log streams.
struct Logs;

impl Stream for Logs {
    type Open = Tail;
    type Part = Line;
    type Done = LogDone;
}

/// Which of the test's two protocols a stream is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Which {
    Files,
    Logs,
}

/// Which stream of a reader's a thing is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Name {
    /// The first of its file streams.
    A,
    /// The second of its file streams.
    B,
    /// Its log stream.
    L,
    /// One it kept nothing for.
    Unknown,
}

/// How a stream ended, as its asker keeps it, whichever protocol it is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Finished {
    /// Its receiver sent it all.
    Sent,
    /// It was stopped.
    Stopped,
    /// It went unanswered for this.
    No(Unanswered),
}

/// What a library did of its streams, as it keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Served {
    /// It sent a stream of `which` this many chunks in one turn, and stopped for want of credit or having sent them all.
    Sent { which: Which, chunks: u32 },
    /// It sent a stream of `which` this many chunks in one turn, and closed.
    Closing { which: Which, chunks: u32 },
    /// It stopped a stream of `which`, as its asker said to.
    Stopped(Which),
    /// An asker went, with this many streams open.
    AskerGone(u64),
}

/// What a reader took of a stream, as it keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
enum Took {
    /// The part at `index` of the stream `name`.
    Part { name: Name, index: u32 },
    /// The answer to the stream `name`.
    Done { name: Name, how: Finished },
}

impl Took {
    /// Whether it is of the stream `name`.
    fn is_of(&self, stream: Name) -> bool {
        let (Self::Part { name, .. } | Self::Done { name, .. }) = self;

        *name == stream
    }
}

/// What the test's two streams share, so one receiver and one asker serve both the same way: how many parts a
/// request asks for and after how many its receiver closes, a part at a place, and the answers.
trait Ends: Stream {
    const WHICH: Which;
    const SENT: Self::Done;
    const STOPPED: Self::Done;

    fn ended(done: Self::Done) -> Finished;

    fn asked(open: Self::Open) -> (u32, Option<u32>);

    fn asking(parts: u32, close_after: Option<u32>) -> Self::Open;

    fn part(index: u32) -> Self::Part;

    fn index(part: &Self::Part) -> u32;
}

impl Ends for Files {
    const WHICH: Which = Which::Files;
    const SENT: ReadDone = ReadDone::Sent;
    const STOPPED: ReadDone = ReadDone::Stopped;

    fn ended(done: ReadDone) -> Finished {
        match done {
            ReadDone::Sent => Finished::Sent,
            ReadDone::Stopped => Finished::Stopped,
        }
    }

    fn asked(read: Read) -> (u32, Option<u32>) {
        (read.chunks, read.close_after)
    }

    fn asking(chunks: u32, close_after: Option<u32>) -> Read {
        Read { chunks, close_after }
    }

    fn part(index: u32) -> Chunk {
        Chunk { index }
    }

    fn index(chunk: &Chunk) -> u32 {
        chunk.index
    }
}

impl Ends for Logs {
    const WHICH: Which = Which::Logs;
    const SENT: LogDone = LogDone::Sent;
    const STOPPED: LogDone = LogDone::Stopped;

    fn ended(done: LogDone) -> Finished {
        match done {
            LogDone::Sent => Finished::Sent,
            LogDone::Stopped => Finished::Stopped,
        }
    }

    fn asked(tail: Tail) -> (u32, Option<u32>) {
        (tail.lines, tail.close_after)
    }

    fn asking(lines: u32, close_after: Option<u32>) -> Tail {
        Tail { lines, close_after }
    }

    fn part(index: u32) -> Line {
        Line { index }
    }

    fn index(line: &Line) -> u32 {
        line.index
    }
}

/// What a library still has to send of one stream.
#[derive(Plain)]
#[kind(name = "aether.test.stream.left")]
struct Left {
    key: StreamKey,
    next: u32,
    chunks: u32,
    close_after: Option<u32>,
}

/// A receiver: it serves file and log streams, sending each chunk its asker's credit allows, and finishing a stream
/// once its last chunk is sent, or once it is stopped. A stream asked to close it after some chunks closes it then.
#[derive(Bound)]
#[kind(name = "aether.test.stream.library")]
struct Library {
    files: Serving<Files>,
    logs: Serving<Logs>,
    files_left: Vec<Left>,
    logs_left: Vec<Left>,
    /// What it did of its streams, in order.
    served: Vec<Served>,
}

/// What a library is made from: nothing.
#[derive(Plain)]
#[kind(name = "aether.test.stream.blank")]
struct Blank;

impl ChildOf for Library {
    type Parent = Anywhere;
}

impl Actor for Library {
    type Config = Blank;
    type Key = u64;
    type Handles = (Open<Files>, Open<Logs>, More, Enough, Ended);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Blank, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self {
            files: Serving::new(),
            logs: Serving::new(),
            files_left: Vec::new(),
            logs_left: Vec::new(),
            served: Vec::new(),
        })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// Opens the stream `open` asks for, to send once credit comes.
fn serve<S: Ends, Sends, I>(
    serving: &mut Serving<S>,
    left: &mut Vec<Left>,
    turn: &mut Turn<'_, Library, Sends>,
    open: Open<S>,
) -> Result<(), Fault>
where
    Sends: Allows<ToSender<S::Done>, I>,
{
    let (key, asked) = serving.open::<_, _, I>(turn, open)?;
    let (chunks, close_after) = S::asked(asked);
    left.push(Left { key, next: 0, chunks, close_after });

    Ok(())
}

/// Sends the stream `key` as many chunks as its credit allows, finishing it once the last is sent, or closing the
/// library once it has sent as many as the stream asked it to close after.
fn pump<S: Ends, Sends, I>(
    serving: &mut Serving<S>,
    left: &mut Vec<Left>,
    served: &mut Vec<Served>,
    turn: &mut Turn<'_, Library, Sends>,
    key: StreamKey,
) -> Result<(), Fault>
where
    Sends: Allows<ToAny<(Part<S>,), Part<S>>, I>,
{
    let Some(place) = left.iter().position(|stream| stream.key == key) else {
        return Ok(());
    };
    let Some(streaming) = serving.get_mut(key) else {
        return Ok(());
    };
    let stream = &mut left[place];
    let mut sent = 0;
    while stream.next < stream.chunks {
        match streaming.send::<_, _, I>(turn, S::part(stream.next)) {
            Ok(()) => {}
            Err(Unsent::NoCredit(_)) => break,
            Err(Unsent::Unwritable(error)) => return Err(error.into()),
        }
        stream.next += 1;
        sent += 1;
        let closes = stream.close_after == Some(stream.next);
        if closes {
            served.push(Served::Closing { which: S::WHICH, chunks: sent });
            turn.close();
            return Ok(());
        }
    }
    served.push(Served::Sent { which: S::WHICH, chunks: sent });

    let all_sent = stream.next == stream.chunks;
    if all_sent {
        left.swap_remove(place);
        if let Some(streaming) = serving.take(key) {
            streaming.finish(turn, &S::SENT)?;
        }
    }

    Ok(())
}

/// Finishes the stream `enough` stops.
fn stop<S: Ends, Sends>(
    serving: &mut Serving<S>,
    left: &mut Vec<Left>,
    served: &mut Vec<Served>,
    turn: &mut Turn<'_, Library, Sends>,
    enough: &Enough,
) -> Result<bool, Fault> {
    let Some(streaming) = serving.stop(turn, enough) else {
        return Ok(false);
    };
    let key = streaming.key();
    left.retain(|stream| stream.key != key);
    streaming.finish(turn, &S::STOPPED)?;
    served.push(Served::Stopped(S::WHICH));

    Ok(true)
}

impl Handles<Open<Files>> for Library {
    type Sends = (ToSender<ReadDone>,);

    fn handle(&mut self, open: Open<Files>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        serve(&mut self.files, &mut self.files_left, turn, open)
    }
}

impl Handles<Open<Logs>> for Library {
    type Sends = (ToSender<LogDone>,);

    fn handle(&mut self, open: Open<Logs>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        serve(&mut self.logs, &mut self.logs_left, turn, open)
    }
}

impl Handles<More> for Library {
    type Sends = (ToAny<(Part<Files>,), Part<Files>>, ToAny<(Part<Logs>,), Part<Logs>>);

    fn handle(&mut self, more: More, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        if let Some(key) = self.files.more(turn, &more) {
            return pump(&mut self.files, &mut self.files_left, &mut self.served, turn, key);
        }
        if let Some(key) = self.logs.more(turn, &more) {
            return pump(&mut self.logs, &mut self.logs_left, &mut self.served, turn, key);
        }

        Ok(())
    }
}

impl Handles<Enough> for Library {
    type Sends = ();

    fn handle(&mut self, enough: Enough, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let stopped = stop(&mut self.files, &mut self.files_left, &mut self.served, turn, &enough)?;
        if !stopped {
            stop(&mut self.logs, &mut self.logs_left, &mut self.served, turn, &enough)?;
        }

        Ok(())
    }
}

impl Handles<Ended> for Library {
    type Sends = ();

    fn handle(&mut self, ended: Ended, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let gone = self.files.ended(&ended).len() + self.logs.ended(&ended).len();
        self.served.push(Served::AskerGone(u64::try_from(gone).unwrap_or(u64::MAX)));

        Ok(())
    }
}

/// What a reader keeps for each stream it opens: the stream's name, how many parts it has taken, after how many it
/// stops the stream, and whether it closes itself after its first.
#[derive(Plain)]
#[kind(name = "aether.test.stream.tag")]
struct Tag {
    name: Name,
    taken: u32,
    stop_after: Option<u32>,
    close_after_first: bool,
}

/// What comes of a stream a reader opens, past its parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Then {
    /// It reads it to the end.
    Reads,
    /// It stops the stream after this many parts.
    StopsAfter(u32),
    /// Its library closes after this many chunks.
    ClosesAfter(u32),
    /// It closes itself after its first part.
    LeavesAfterFirst,
}

/// An order a reader sends itself: to open the stream `name` of the protocol `which`, of `parts` parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.stream.order")]
struct Order {
    which: Which,
    name: Name,
    parts: u32,
    then: Then,
}

impl Order {
    /// The order, stopping the stream after `parts` parts.
    const fn stopping_after(self, parts: u32) -> Self {
        Self { then: Then::StopsAfter(parts), ..self }
    }

    /// The order, its library closing after `chunks` chunks.
    const fn closing_after(self, chunks: u32) -> Self {
        Self { then: Then::ClosesAfter(chunks), ..self }
    }

    /// The order, the reader closing itself after its first part.
    const fn leaving(self) -> Self {
        Self { then: Then::LeavesAfterFirst, ..self }
    }
}

/// An order to read the file stream `name` of `parts` chunks.
const fn read(name: Name, parts: u32) -> Order {
    Order { which: Which::Files, name, parts, then: Then::Reads }
}

/// An order to read the log stream `name` of `parts` lines.
const fn tail(name: Name, parts: u32) -> Order {
    Order { which: Which::Logs, name, parts, then: Then::Reads }
}

/// Why a reader could not begin.
#[derive(Plain)]
#[kind(name = "aether.test.stream.trouble")]
enum Trouble {
    NoLibrary,
    NoWindow,
}

/// A reader's orders, and how many parts each of its streams may be ahead.
#[derive(Plain)]
#[kind(name = "aether.test.stream.reader_config")]
struct ReaderConfig {
    window: u32,
    orders: Vec<Order>,
}

/// An asker: it spawns a library as it begins, opens the streams its orders name, and keeps each part and each answer.
#[derive(Bound)]
#[kind(name = "aether.test.stream.reader")]
struct Reader {
    library: ActorRef<Library>,
    files: Streams<Files, Tag>,
    logs: Streams<Logs, Tag>,
    /// What it took, in order.
    took: Vec<Took>,
}

impl ChildOf for Reader {
    type Parent = Top;
}

impl Actor for Reader {
    type Config = ReaderConfig;
    type Key = ();
    type Handles = (Order, Part<Files>, Part<Logs>, Answer<ReadDone, Number>, Answer<LogDone, Number>);
    type BeginSends = (ToType<Self, Order>,);
    type EndSends = ();

    fn begin(config: ReaderConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let Ok(library) = turn.spawn(&1, Blank) else {
            return Err(Fault::new(&Trouble::NoLibrary)?);
        };
        let Some(window) = NonZeroU32::new(config.window) else {
            return Err(Fault::new(&Trouble::NoWindow)?);
        };
        for order in config.orders {
            turn.to(turn.me()).send(&order)?;
        }

        Ok(Self { library, files: Streams::new(window), logs: Streams::new(window), took: Vec::new() })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// What `order` asks to open: the request, and what to keep for it.
fn opening<S: Ends>(order: Order) -> (S::Open, Tag) {
    let Order { name, parts, then, .. } = order;
    let (stop_after, close_after) = match then {
        Then::StopsAfter(parts) => (Some(parts), None),
        Then::ClosesAfter(chunks) => (None, Some(chunks)),
        Then::Reads | Then::LeavesAfterFirst => (None, None),
    };
    let close_after_first = then == Then::LeavesAfterFirst;

    (S::asking(parts, close_after), Tag { name, taken: 0, stop_after, close_after_first })
}

impl Handles<Order> for Reader {
    type Sends = (
        Asks<Streamed<Files>, Open<Files>, Number>,
        ToAny<Streamed<Files>, More>,
        Asks<Streamed<Logs>, Open<Logs>, Number>,
        ToAny<Streamed<Logs>, More>,
    );

    fn handle(&mut self, order: Order, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        match order.which {
            Which::Files => {
                let (read, tag) = opening::<Files>(order);
                self.files.open(turn, self.library.protocol(), read, tag)?;
            }
            Which::Logs => {
                let (tail, tag) = opening::<Logs>(order);
                self.logs.open(turn, self.library.protocol(), tail, tag)?;
            }
        }

        Ok(())
    }
}

/// Takes `part`, keeping it, and stops its stream or closes the reader when what it kept says to.
fn take<S: Ends, Sends, IM, IE>(
    streams: &mut Streams<S, Tag>,
    took: &mut Vec<Took>,
    turn: &mut Turn<'_, Reader, Sends>,
    part: Part<S>,
) -> Result<(), Fault>
where
    Sends: Allows<ToAny<Streamed<S>, More>, IM> + Allows<ToAny<Streamed<S>, Enough>, IE>,
{
    let Some(Taken { number, value: chunk, kept: tag }) = streams.part::<_, _, IM>(turn, part)? else {
        return Ok(());
    };
    took.push(Took::Part { name: tag.name, index: S::index(&chunk) });
    tag.taken += 1;
    let stops = tag.stop_after == Some(tag.taken);
    let leaves = tag.close_after_first;
    if stops {
        streams.stop::<_, _, IE>(turn, number)?;
    }
    if leaves {
        turn.close();
    }

    Ok(())
}

impl Handles<Part<Files>> for Reader {
    type Sends = (ToAny<Streamed<Files>, More>, ToAny<Streamed<Files>, Enough>);

    fn handle(&mut self, part: Part<Files>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        take(&mut self.files, &mut self.took, turn, part)
    }
}

impl Handles<Part<Logs>> for Reader {
    type Sends = (ToAny<Streamed<Logs>, More>, ToAny<Streamed<Logs>, Enough>);

    fn handle(&mut self, part: Part<Logs>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        take(&mut self.logs, &mut self.took, turn, part)
    }
}

/// How the stream kept as `tag` ended.
fn ended<S: Ends>(tag: Option<Tag>, answered: Answered<S::Done>) -> Took {
    let name = tag.map_or(Name::Unknown, |tag| tag.name);
    let how = match answered {
        Answered::Yes(done) => S::ended(done),
        Answered::No(why) => Finished::No(why),
    };

    Took::Done { name, how }
}

impl Handles<Answer<ReadDone, Number>> for Reader {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, number): (Answered<ReadDone>, Number),
        _: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        self.took.push(ended::<Files>(self.files.answered(number), answered));

        Ok(())
    }
}

impl Handles<Answer<LogDone, Number>> for Reader {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, number): (Answered<LogDone>, Number),
        _: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        self.took.push(ended::<Logs>(self.logs.answered(number), answered));

        Ok(())
    }
}

/// A clock that never moves.
struct Stopped;

impl Clock for Stopped {
    fn nanos(&self) -> u64 {
        0
    }
}

/// Each engine the tests run in: how many parts, and how many fused hops each part runs at most in a step.
const ENGINES: [(usize, usize); 4] = [(1, 0), (1, 64), (4, 0), (4, 64)];

/// What a reader and its library came to.
struct Run {
    /// What the reader took, as of the last step it was live.
    reader: Vec<Took>,
    /// What the library did, as of the last step it was live.
    library: Vec<Served>,
}

/// What came of a reader that follows `orders`, with streams `window` parts ahead, and its library, in an engine in
/// `parts` parts that each run at most `hops` fused hops a step.
fn run((parts, hops): (usize, usize), window: u32, orders: &[Order]) -> Run {
    let Some(parts) = NonZeroUsize::new(parts) else {
        panic!("an engine has parts");
    };
    let mut engine = Engine::new(parts, 1024, 1 << 20, 1 << 20, hops).ready();
    let Ok(id) = engine.add_root::<Reader>(&(), ReaderConfig { window, orders: orders.to_vec() }) else {
        panic!("the reader begins");
    };
    let Some(seat) = engine.table().seat(id) else {
        panic!("the reader is seated");
    };
    let reader = Seated { id, seat };
    let Some(library) = engine.state::<Reader>(reader).map(|reader| reader.library.seated()) else {
        panic!("the reader's state is here");
    };

    // Either may close, so what each kept is read after each step it was live for.
    let mut run = Run { reader: Vec::new(), library: Vec::new() };
    for _ in 0..64 {
        engine.step(Time::ZERO, &Stopped);
        if let Some(state) = engine.state::<Reader>(reader) {
            run.reader.clone_from(&state.took);
        }
        if let Some(state) = engine.state::<Library>(library) {
            run.library.clone_from(&state.served);
        }
    }

    run
}

/// What of `took` is of the stream `name`, in order.
fn of(took: &[Took], name: Name) -> Vec<Took> {
    took.iter().filter(|took| took.is_of(name)).cloned().collect()
}

/// The part at `index` of the stream `name`.
const fn part(name: Name, index: u32) -> Took {
    Took::Part { name, index }
}

/// The answer to the stream `name`.
const fn done(name: Name, how: Finished) -> Took {
    Took::Done { name, how }
}

/// The library sent a file stream this many chunks in a turn.
const fn files(chunks: u32) -> Served {
    Served::Sent { which: Which::Files, chunks }
}

/// The library sent a log stream this many chunks in a turn.
const fn logs(chunks: u32) -> Served {
    Served::Sent { which: Which::Logs, chunks }
}

#[test]
fn a_stream_sends_its_parts_as_credit_comes_and_then_its_answer() {
    // Catches parts sent past the credit granted, credit not spent as parts go, a part taken without granting one back
    // (the stream would stall and never be answered), and an answer that overtakes its parts.
    for engine in ENGINES {
        let run = run(engine, 2, &[read(A, 5)]);

        let parts = [part(A, 0), part(A, 1), part(A, 2), part(A, 3), part(A, 4)];
        assert_eq!(run.reader[..5], parts, "{engine:?}");
        assert_eq!(run.reader[5..], [done(A, Finished::Sent)], "{engine:?}");
        assert_eq!(run.library, [files(2), files(1), files(1), files(1)], "{engine:?}");
    }
}

#[test]
fn two_streams_to_one_receiver_stay_apart() {
    // Catches two streams of one asker given one number, a part handed what was kept for another stream, and an
    // answer that hands back another stream's.
    for engine in ENGINES {
        let run = run(engine, 1, &[read(A, 3), read(B, 2)]);

        let first = [part(A, 0), part(A, 1), part(A, 2), done(A, Finished::Sent)];
        assert_eq!(of(&run.reader, A), first, "{engine:?}");
        assert_eq!(of(&run.reader, B), [part(B, 0), part(B, 1), done(B, Finished::Sent)], "{engine:?}");
    }
}

#[test]
fn streams_of_two_protocols_to_one_receiver_stay_apart() {
    // Catches credit or a stop for one protocol's stream taken by another's of the same number, and two protocols'
    // requests or parts of one layout given one kind id.
    for engine in ENGINES {
        let run = run(engine, 1, &[read(A, 3), tail(L, 3)]);

        let first = [part(A, 0), part(A, 1), part(A, 2), done(A, Finished::Sent)];
        assert_eq!(of(&run.reader, A), first, "{engine:?}");
        let second = [part(L, 0), part(L, 1), part(L, 2), done(L, Finished::Sent)];
        assert_eq!(of(&run.reader, L), second, "{engine:?}");
        assert_eq!(run.library, [files(1), logs(1)].repeat(3), "{engine:?}");
    }
}

#[test]
fn a_stopped_stream_passes_over_its_late_parts_and_is_answered() {
    // Catches parts already on their way taken after a stop, credit granted for them, and a stopped stream that the
    // receiver never answers, which would keep what its asker kept forever.
    for engine in ENGINES {
        let run = run(engine, 4, &[read(A, 10).stopping_after(1)]);

        assert_eq!(run.reader, [part(A, 0), done(A, Finished::Stopped)], "{engine:?}");
        assert_eq!(run.library, [files(4), files(1), Served::Stopped(Which::Files)], "{engine:?}");
    }
}

#[test]
fn a_stream_whose_receiver_ends_is_answered_that_it_ended() {
    // Catches the request a receiver holds for a stream let go of unanswered as its receiver ends, which would leave
    // the asker keeping what it kept for good.
    for engine in ENGINES {
        let run = run(engine, 4, &[read(A, 5).closing_after(2)]);

        let ended = done(A, Finished::No(Unanswered::Ended));
        assert_eq!(run.reader, [part(A, 0), part(A, 1), ended], "{engine:?}");
    }
}

#[test]
fn a_receiver_lets_go_of_the_streams_of_an_asker_that_ended() {
    // Catches an asker left unwatched, so the streams of one that ended are kept, with what they hold, for as long as
    // their receiver lives.
    for engine in ENGINES {
        let run = run(engine, 4, &[read(A, 10).leaving()]);

        assert_eq!(run.library, [files(4), files(1), Served::AskerGone(1)], "{engine:?}");
    }
}
