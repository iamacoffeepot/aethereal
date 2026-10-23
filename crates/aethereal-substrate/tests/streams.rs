//! Streams as a program runs them: an asker opens them through `Streams`, a receiver serves them through `Serving`, and
//! each part, credit, stop, and answer goes as mail through the engine (ADR-0017).

use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anywhere, Asks, Beginning, ChildOf, Ended, Ending, Enough, Fault, Handles, More,
    Number, Open, Part, Serving, Stream, StreamKey, Streamed, Streams, Time, ToAny, ToSender, ToType, Top, Turn,
    Unanswered, Unsent,
};
use aethereal_data::{Bound, KindName, Plain};
use aethereal_substrate::{Clock, Engine};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::mem;
use std::num::{NonZeroU32, NonZeroUsize};

thread_local! {
    /// What each actor of this thread has said, in order, by who said it.
    static SAID: RefCell<BTreeMap<&'static str, Vec<String>>> = const { RefCell::new(BTreeMap::new()) };
}

/// Notes what `who` did.
fn say(who: &'static str, line: String) {
    SAID.with_borrow_mut(|said| said.entry(who).or_default().push(line));
}

/// What has been said since this was last asked, by who said it.
fn said() -> BTreeMap<&'static str, Vec<String>> {
    SAID.with_borrow_mut(mem::take)
}

/// What a stream is opened with: how many chunks to send, and after how many sent its receiver closes.
#[derive(Plain)]
#[kind(name = "aether.test.stream.read")]
struct Read {
    chunks: u32,
    close_after: Option<u32>,
}

/// One part: the chunk's place in its stream.
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

/// How a log stream ended: a second protocol, whose parts have the same layout as a file's.
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

    const OPEN: KindName = KindName::new("aether.test.stream.files.open");
    const PART: KindName = KindName::new("aether.test.stream.files.part");
}

/// Log streams.
struct Logs;

impl Stream for Logs {
    type Open = Read;
    type Part = Chunk;
    type Done = LogDone;

    const OPEN: KindName = KindName::new("aether.test.stream.logs.open");
    const PART: KindName = KindName::new("aether.test.stream.logs.part");
}

/// What a stream's answers are, for a receiver that serves both protocols with one pump.
trait Ends: Stream<Part = Chunk> {
    const SENT: Self::Done;
    const STOPPED: Self::Done;
}

impl Ends for Files {
    const SENT: ReadDone = ReadDone::Sent;
    const STOPPED: ReadDone = ReadDone::Stopped;
}

impl Ends for Logs {
    const SENT: LogDone = LogDone::Sent;
    const STOPPED: LogDone = LogDone::Stopped;
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
        Ok(Self { files: Serving::new(), logs: Serving::new(), files_left: Vec::new(), logs_left: Vec::new() })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// Opens the stream `open` asks for, to send once credit comes.
fn serve<S: Ends<Open = Read>, Sends, I>(
    serving: &mut Serving<S>,
    left: &mut Vec<Left>,
    turn: &mut Turn<'_, Library, Sends>,
    open: Open<S>,
) -> Result<(), Fault>
where
    Sends: aethereal_actor::Allows<ToSender<S::Done>, I>,
{
    let (key, read) = serving.open::<_, _, I>(turn, open)?;
    left.push(Left { key, next: 0, chunks: read.chunks, close_after: read.close_after });

    Ok(())
}

/// Sends the stream `key` as many chunks as its credit allows, finishing it once the last is sent, or closing the
/// library once it has sent as many as the stream asked it to close after.
fn pump<S: Ends, Sends, I>(
    serving: &mut Serving<S>,
    left: &mut Vec<Left>,
    turn: &mut Turn<'_, Library, Sends>,
    key: StreamKey,
) -> Result<(), Fault>
where
    Sends: aethereal_actor::Allows<ToAny<(Part<S>,), Part<S>>, I>,
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
        match streaming.send::<_, _, I>(turn, Chunk { index: stream.next }) {
            Ok(()) => {}
            Err(Unsent::NoCredit(_)) => break,
            Err(Unsent::Unwritable(error)) => return Err(error.into()),
        }
        stream.next += 1;
        sent += 1;
        let closes = stream.close_after == Some(stream.next);
        if closes {
            say("library", format!("sent {sent}, closing"));
            turn.close();
            return Ok(());
        }
    }
    say("library", format!("sent {sent}"));

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
    turn: &mut Turn<'_, Library, Sends>,
    enough: &Enough,
) -> Result<bool, Fault> {
    let Some(streaming) = serving.stop(turn, enough) else {
        return Ok(false);
    };
    let key = streaming.key();
    left.retain(|stream| stream.key != key);
    streaming.finish(turn, &S::STOPPED)?;
    say("library", String::from("stopped"));

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
            return pump(&mut self.files, &mut self.files_left, turn, key);
        }
        if let Some(key) = self.logs.more(turn, &more) {
            return pump(&mut self.logs, &mut self.logs_left, turn, key);
        }

        Ok(())
    }
}

impl Handles<Enough> for Library {
    type Sends = ();

    fn handle(&mut self, enough: Enough, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let stopped = stop(&mut self.files, &mut self.files_left, turn, &enough)?;
        if !stopped {
            stop(&mut self.logs, &mut self.logs_left, turn, &enough)?;
        }

        Ok(())
    }
}

impl Handles<Ended> for Library {
    type Sends = ();

    fn handle(&mut self, ended: Ended, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let gone = self.files.ended(&ended).len() + self.logs.ended(&ended).len();
        say("library", format!("asker gone with {gone} open"));

        Ok(())
    }
}

/// What a reader keeps for each stream it opens: the stream's name, how many parts it has taken, after how many it
/// stops the stream, and whether it closes itself after its first.
#[derive(Plain)]
#[kind(name = "aether.test.stream.tag")]
struct Tag {
    name: String,
    taken: u32,
    stop_after: Option<u32>,
    close_after_first: bool,
}

/// An order a reader sends itself: `files NAME CHUNKS`, `logs NAME CHUNKS`, each followed by `stop N`, `close N`
/// (its library closes after N chunks), or `leave` (it closes itself after its first part).
#[derive(Plain)]
#[kind(name = "aether.test.stream.order")]
struct Order {
    text: String,
}

/// A reader's orders, and how many parts each of its streams may be ahead.
#[derive(Plain)]
#[kind(name = "aether.test.stream.reader_config")]
struct ReaderConfig {
    window: u32,
    orders: Vec<String>,
}

/// An asker: it spawns a library as it begins, opens the streams its orders name, and says each part and each answer.
#[derive(Bound)]
#[kind(name = "aether.test.stream.reader")]
struct Reader {
    library: ActorRef<Library>,
    files: Streams<Files, Tag>,
    logs: Streams<Logs, Tag>,
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
            return Err(Fault::new(&Order { text: String::from("no library") })?);
        };
        let Some(window) = NonZeroU32::new(config.window) else {
            return Err(Fault::new(&Order { text: String::from("no window") })?);
        };
        for text in config.orders {
            turn.to(turn.me()).send(&Order { text })?;
        }

        Ok(Self { library, files: Streams::new(window), logs: Streams::new(window) })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// What an order asks to open: the stream's name, the request, and what to keep for it.
fn opening(words: &[&str]) -> (Read, Tag) {
    let name = String::from(words[1]);
    let chunks = words[2].parse().unwrap_or_default();
    let count = words.get(4).and_then(|count| count.parse().ok());
    let then = words.get(3).copied();
    let stop_after = count.filter(|_| then == Some("stop"));
    let close_after = count.filter(|_| then == Some("close"));
    let close_after_first = then == Some("leave");

    (Read { chunks, close_after }, Tag { name, taken: 0, stop_after, close_after_first })
}

impl Handles<Order> for Reader {
    type Sends = (
        Asks<Streamed<Files>, Open<Files>, Number>,
        ToAny<Streamed<Files>, More>,
        Asks<Streamed<Logs>, Open<Logs>, Number>,
        ToAny<Streamed<Logs>, More>,
    );

    fn handle(&mut self, order: Order, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let words: Vec<&str> = order.text.split(' ').collect();
        let (read, tag) = opening(&words);
        match words[0] {
            "files" => self.files.open(turn, self.library.protocol(), read, tag)?,
            _ => self.logs.open(turn, self.library.protocol(), read, tag)?,
        };

        Ok(())
    }
}

/// Takes `part`, saying it, and stops its stream or closes the reader when what it kept says to.
fn take<S: Stream<Part = Chunk>, Sends, IM, IE>(
    streams: &mut Streams<S, Tag>,
    turn: &mut Turn<'_, Reader, Sends>,
    part: Part<S>,
) -> Result<(), Fault>
where
    Sends: aethereal_actor::Allows<ToAny<Streamed<S>, More>, IM> + aethereal_actor::Allows<ToAny<Streamed<S>, Enough>, IE>,
{
    let Some((number, chunk, tag)) = streams.part::<_, _, IM>(turn, part)? else {
        return Ok(());
    };
    say("reader", format!("{} {}", tag.name, chunk.index));
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
        take(&mut self.files, turn, part)
    }
}

impl Handles<Part<Logs>> for Reader {
    type Sends = (ToAny<Streamed<Logs>, More>, ToAny<Streamed<Logs>, Enough>);

    fn handle(&mut self, part: Part<Logs>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        take(&mut self.logs, turn, part)
    }
}

/// Says how the stream kept as `tag` ended.
fn ended<D: std::fmt::Debug>(tag: Option<Tag>, answered: Answered<D>) {
    let name = tag.map_or_else(|| String::from("unknown"), |tag| tag.name);
    let how = match answered {
        Answered::Yes(done) => format!("{done:?}"),
        Answered::No(why) => format!("no {why:?}"),
    };
    say("reader", format!("{name} done {how}"));
}

impl Handles<Answer<ReadDone, Number>> for Reader {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, number): (Answered<ReadDone>, Number),
        _: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        ended(self.files.answered(number), answered);

        Ok(())
    }
}

impl Handles<Answer<LogDone, Number>> for Reader {
    type Sends = ();

    fn handle(&mut self, (answered, number): (Answered<LogDone>, Number), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        ended(self.logs.answered(number), answered);

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

/// What a reader that follows `orders`, with streams `window` parts ahead, and its library said, in an engine in
/// `parts` parts that each run at most `hops` fused hops a step.
fn heard((parts, hops): (usize, usize), window: u32, orders: &[&str]) -> BTreeMap<&'static str, Vec<String>> {
    let Some(parts) = NonZeroUsize::new(parts) else {
        panic!("an engine has parts");
    };
    let mut engine = Engine::new(parts, 1024, 1 << 20, 1 << 20, hops).ready();
    let orders = orders.iter().map(|text| String::from(*text)).collect();
    let made = engine.add_root::<Reader>(&(), ReaderConfig { window, orders });
    assert!(made.is_ok(), "the reader begins");
    said();
    for _ in 0..64 {
        engine.step(Time::ZERO, &Stopped);
    }

    said()
}

/// The lines of `said` that begin with `name`, in order.
fn of(said: &BTreeMap<&'static str, Vec<String>>, who: &str, name: &str) -> Vec<String> {
    let lines = said.get(who).map(Vec::as_slice).unwrap_or_default();

    lines.iter().filter(|line| line.starts_with(name)).cloned().collect()
}

#[test]
fn a_stream_sends_its_parts_as_credit_comes_and_then_its_answer() {
    // Catches parts sent past the credit granted, credit not spent as parts go, a part taken without granting one back
    // (the stream would stall and never be answered), and an answer that overtakes its parts.
    for engine in ENGINES {
        let heard = heard(engine, 2, &["files a 5"]);

        assert_eq!(heard["reader"], ["a 0", "a 1", "a 2", "a 3", "a 4", "a done Sent"], "{engine:?}");
        assert_eq!(heard["library"], ["sent 2", "sent 1", "sent 1", "sent 1"], "{engine:?}");
    }
}

#[test]
fn two_streams_to_one_receiver_stay_apart() {
    // Catches two streams of one asker given one number, a part handed what was kept for another stream, and an
    // answer that hands back another stream's.
    for engine in ENGINES {
        let heard = heard(engine, 1, &["files a 3", "files b 2"]);

        assert_eq!(of(&heard, "reader", "a"), ["a 0", "a 1", "a 2", "a done Sent"], "{engine:?}");
        assert_eq!(of(&heard, "reader", "b"), ["b 0", "b 1", "b done Sent"], "{engine:?}");
    }
}

#[test]
fn streams_of_two_protocols_to_one_receiver_stay_apart() {
    // Catches credit or a stop for one protocol's stream taken by another's of the same number, and two protocols'
    // parts of one layout given one kind id.
    for engine in ENGINES {
        let heard = heard(engine, 1, &["files a 3", "logs l 3"]);

        assert_eq!(of(&heard, "reader", "a"), ["a 0", "a 1", "a 2", "a done Sent"], "{engine:?}");
        assert_eq!(of(&heard, "reader", "l"), ["l 0", "l 1", "l 2", "l done Sent"], "{engine:?}");
    }
}

#[test]
fn a_stopped_stream_passes_over_its_late_parts_and_is_answered() {
    // Catches parts already on their way taken after a stop, credit granted for them, and a stopped stream that the
    // receiver never answers, which would keep what its asker kept forever.
    for engine in ENGINES {
        let heard = heard(engine, 4, &["files a 10 stop 1"]);

        assert_eq!(heard["reader"], ["a 0", "a done Stopped"], "{engine:?}");
        assert_eq!(heard["library"], ["sent 4", "sent 1", "stopped"], "{engine:?}");
    }
}

#[test]
fn a_stream_whose_receiver_ends_is_answered_that_it_ended() {
    // Catches the request a receiver holds for a stream let go of unanswered as its receiver ends, which would leave
    // the asker keeping what it kept for good.
    for engine in ENGINES {
        let heard = heard(engine, 4, &["files a 5 close 2"]);

        let ended = format!("a done no {:?}", Unanswered::Ended);

        assert_eq!(heard["reader"], ["a 0", "a 1", ended.as_str()], "{engine:?}");
    }
}

#[test]
fn a_receiver_lets_go_of_the_streams_of_an_asker_that_ended() {
    // Catches an asker left unwatched, so the streams of one that ended are kept, with what they hold, for as long as
    // their receiver lives.
    for engine in ENGINES {
        let heard = heard(engine, 4, &["files a 10 leave"]);

        assert_eq!(heard["library"], ["sent 4", "sent 1", "asker gone with 1 open"], "{engine:?}");
    }
}
