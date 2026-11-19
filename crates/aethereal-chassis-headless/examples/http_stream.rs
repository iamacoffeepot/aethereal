//! How fast a streamed HTTP response's body goes from its source to a client over loopback (ADR-0035).
//!
//! Run with `cargo run --release -p aethereal-chassis-headless --example http_stream [mebibytes=N] [unsent=BYTES]`.
//! A source serves a body of `N` MiB (by default 64) in parts of 4 KiB, 64 KiB, and 1 MiB, each part the same blob,
//! so the bench costs the stack and the connection, not the source. The body is written chunked (its length not known)
//! and with a `Content-Length` (known), to a client on a thread of its own that reads it to the close. Each line is the
//! best of three runs: MiB a second, from the request written to the close read.
//!
//! The program is a stack of its own on TCP (ADR-0035), with the engine's and the stack's default settings, but for the
//! connection's most unsent bytes, `unsent` (by default 8 MiB): a write of a window of 1 MiB parts must fit it whole.
//! The source is the stack's one layer, its handler, and answers the request with the body streamed from itself. The
//! host is stepped by the harness (ADR-0036).

#![allow(clippy::print_stdout)] // A bench prints its table.
#![allow(clippy::cast_precision_loss)] // A rate printed to one place loses nothing it shows.

use aethereal_actor::{
    Answered, Beginning, Bytes, Ended, Enough, Fault, Later, Level, More, Number, Open, Part, Reply, Serving, Slept,
    Speaks, Turn, actor,
};
use aethereal_chassis_headless::Headless;
use aethereal_chassis_headless::waiting::{self, BUDGET};
use aethereal_data::{Bound, Plain};
use aethereal_http::stack::{
    Answering, Binding, BodyRead, Deadline, FromStack, Gather, Gathered, InboundBody, Layers, Listened, OutboundBody,
    Outward, ParkedPart, ParkedResponse, PartStep, Request, Response, ResponseBody, ResponseBytes, ResponseLength,
    Sending, Serving as StackServing, SourceEnd, Stack, StackSettings, StreamedBody, layers,
};
use aethereal_http::{Header, Status};
use aethereal_substrate::harness::{Harness, Unheard};
use aethereal_substrate::{Bootstrap, Bootstrapping, EngineSettings, Host, Unbuilt};
use aethereal_tcp::{Accepting, HungUp, Listening, Reading, Shut, Tcp, TcpSettings, Unbound, Wrote};
use std::env;
use std::error::Error;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::thread::{self, JoinHandle};
use std::time::Instant;

/// The bytes of a mebibyte.
const MEBIBYTE: u32 = 1 << 20;

/// The most bytes of a response's head the client looks through for its end.
const HEAD_BYTES: usize = 4096;

/// The sizes of part a body is sent in.
const PART_BYTES: [u32; 3] = [4 << 10, 64 << 10, MEBIBYTE];

/// What a source sends for each body it is opened for: `parts` parts of `part_bytes` bytes, its length named as
/// `framing` says.
#[derive(Debug, Clone, Copy, Plain)]
#[kind(name = "aether.bench.http.plan")]
#[settings]
struct Plan {
    parts: u32,
    part_bytes: u32,
    framing: Framing,
}

/// How a source's body is framed: whether its length is named.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.bench.http.framing")]
enum Framing {
    /// Its length not known: written in chunks.
    #[default]
    Chunked,
    /// Its length known: written with its `Content-Length`.
    Known,
}

/// A root that answers each request with a body streamed from itself, and sends each [`OutboundBody`] stream opened on
/// it the same part, as many times as its plan says, a part for each credit it has.
#[derive(Bound)]
#[kind(name = "aether.bench.http.source")]
struct Source {
    plan: Plan,
    /// The part it sends, put once.
    part: Bytes,
    serving: Serving<OutboundBody>,
    /// The parts sent of the stream open.
    sent: u32,
}

#[actor]
impl Source {
    fn begin(plan: Plan, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let part_bytes = usize::try_from(plan.part_bytes).unwrap_or(usize::MAX);
        let mut part = Vec::from(turn.buffer(part_bytes));
        part.resize(part_bytes, b'x');
        let Ok(part) = turn.put(part) else {
            panic!("the source's part fits the engine's blobs");
        };

        Ok(Self { plan, part, serving: Serving::new(), sent: 0 })
    }

    #[request]
    fn request(&mut self, _: Request<(), ()>, turn: &mut Turn<'_, Self, FromStack>) -> Result<Response, Fault> {
        let length = match self.plan.framing {
            Framing::Chunked => ResponseLength::Unknown,
            Framing::Known => ResponseLength::Known(u64::from(self.plan.parts) * u64::from(self.plan.part_bytes)),
        };
        let body = StreamedBody { source: turn.me().protocol(), body: 1, length };
        let response = Response {
            status: Status::Ok,
            headers: vec![Header::new("content-type", "application/octet-stream")],
            body: ResponseBody::Streamed(body),
        };

        Ok(response)
    }

    #[request]
    fn open(
        &mut self,
        open: Open<OutboundBody>,
        turn: &mut Turn<'_, Self, Speaks<(Part<OutboundBody>,)>>,
    ) -> Result<Later<SourceEnd>, Fault> {
        let (_, _, later) = self.serving.open(turn, open);
        self.sent = 0;

        Ok(later)
    }

    #[tell]
    fn more(&mut self, more: More, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Some(key) = self.serving.more(turn, &more) else {
            return Ok(());
        };
        let Some(stream) = self.serving.get_mut(key) else {
            return Ok(());
        };
        loop {
            let has_credit = stream.credit() > 0;
            let has_parts = self.sent < self.plan.parts;
            let sends = has_credit && has_parts;
            if !sends {
                break;
            }
            assert!(
                stream.send(turn, ResponseBytes { bytes: self.part.clone() }).is_ok(),
                "a part within credit sends"
            );
            self.sent += 1;
        }
        if self.sent < self.plan.parts {
            return Ok(());
        }
        if let Some(stream) = self.serving.take(key) {
            stream.finish(turn, &SourceEnd::Whole)?;
        }

        Ok(())
    }

    #[tell]
    fn enough(&mut self, enough: Enough, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        if let Some(stopped) = self.serving.stop(turn, &enough) {
            stopped.finish(turn, &SourceEnd::Stopped)?;
        }

        Ok(())
    }

    #[ended]
    fn ended(&mut self, ended: Ended) -> Result<(), Fault> {
        self.serving.ended(&ended);

        Ok(())
    }
}

impl Bootstrap<Headless> for Source {
    const SECTION: &'static str = "source";
    type Settings = Plan;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(plan: Plan, (): (), cx: &mut Bootstrapping<'_, Headless, Self>) -> Result<(), Unbuilt> {
        cx.begin(&(), plan)?;

        Ok(())
    }
}

/// The bench's stack: its one layer is the source, which handles every request.
#[derive(Bound)]
#[kind(name = "aether.bench.http.bench_stack")]
struct BenchStack {
    serving: StackServing<Self, (Source,)>,
}

impl Bootstrap<Headless> for BenchStack {
    const SECTION: &'static str = "bench_stack";
    type Settings = StackSettings;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(
        settings: StackSettings,
        (): (),
        cx: &mut Bootstrapping<'_, Headless, Self>,
    ) -> Result<(), Unbuilt> {
        settings.fit(cx)?;

        cx.begin(&(), settings)?;

        Ok(())
    }
}

#[actor(depends(Tcp, Source))]
impl BenchStack {
    fn begin(settings: StackSettings, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let layers: Layers<(Source,)> = layers(turn)?;

        Ok(Self { serving: StackServing::listen(turn, &settings, layers)? })
    }

    #[answer]
    fn listening(&mut self, listening: (Answered<Listening>, Binding), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let listened = self.serving.listened(listening, turn)?;
        if let Listened::Refused(_) | Listened::Unanswered(_) = listened {
            turn.log(Level::Error, format_args!("no port to serve on: {listened:?}"));
        }

        Ok(())
    }

    #[tell]
    fn accepting(&mut self, part: Part<Accepting>, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.accept(part, turn)
    }

    #[answer]
    fn unbound(&mut self, unbound: (Answered<Unbound>, Number), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.unbound(unbound, turn)
    }

    #[tell]
    fn reading(&mut self, part: Part<Reading>, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.read(part, turn)
    }

    #[answer]
    fn hung_up(&mut self, hung_up: (Answered<HungUp>, Number), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.hung_up(hung_up, turn)
    }

    #[answer]
    fn wrote(&mut self, wrote: (Answered<Wrote>, Sending), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.wrote(wrote, turn)
    }

    #[answer]
    fn response(&mut self, answered: (Answered<Response>, Answering), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.answered(answered, turn)
    }

    #[answer]
    fn slept(&mut self, slept: (Answered<Slept>, Deadline), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.slept(slept, turn)
    }

    #[request]
    fn gather(&mut self, gather: Gather, turn: &mut Turn<'_, Self>) -> Result<Reply<Gathered>, Fault> {
        self.serving.gather(gather, turn)
    }

    #[tell]
    fn shut(&mut self, _: Shut, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.shut(turn)
    }

    #[answer]
    fn outward(
        &mut self,
        returned: (Answered<Outward>, ParkedResponse),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        self.serving.returned(returned, turn)
    }

    #[answer]
    fn part_step(&mut self, passed: (Answered<PartStep>, ParkedPart), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.passed(passed, turn)
    }

    #[request]
    fn open(
        &mut self,
        open: Open<InboundBody>,
        turn: &mut Turn<'_, Self, Speaks<(Part<InboundBody>,)>>,
    ) -> Result<Later<BodyRead>, Fault> {
        self.serving.open_body(open, turn)
    }

    #[tell]
    fn more(&mut self, more: More, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.grant_body(&more, turn)
    }

    #[tell]
    fn enough(&mut self, enough: Enough, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.stop_reading(&enough, turn)
    }

    #[ended]
    fn ended(&mut self, ended: Ended, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.lose_reader(&ended, turn)
    }

    #[tell]
    fn outbound_body(&mut self, part: Part<OutboundBody>, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.take_part(part, turn)
    }

    #[answer]
    fn source_end(&mut self, ended: (Answered<SourceEnd>, Number), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.end_source(ended, turn)
    }
}

impl Stack for BenchStack {}

/// What one run's client read: the bytes after the head, framing and all, and the nanoseconds from its request written
/// to the close.
struct Read {
    body_bytes: u64,
    nanos: u64,
}

/// A client on a thread of its own that asks `port` for the body and reads it to the close.
fn client(port: u16) -> JoinHandle<Read> {
    #[allow(clippy::disallowed_methods)] // A native thread outside the engine, which times itself.
    thread::spawn(move || {
        let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else {
            panic!("the client connects");
        };
        let started = Instant::now();
        assert!(stream.write_all(b"GET /body HTTP/1.1\r\nConnection: close\r\n\r\n").is_ok(), "the client writes");
        let mut chunk = vec![0; 1 << 20];
        let mut first = Vec::new();
        let mut received = 0_u64;
        loop {
            let count = stream.read(&mut chunk).unwrap_or(0);
            if count == 0 {
                break;
            }
            if first.len() < HEAD_BYTES {
                first.extend_from_slice(&chunk[..count.min(HEAD_BYTES)]);
            }
            received += u64::try_from(count).unwrap_or(0);
        }
        let head_bytes = first.windows(4).position(|window| window == b"\r\n\r\n").map_or(0, |end| end + 4);
        let body_bytes = received.saturating_sub(u64::try_from(head_bytes).unwrap_or(0));
        let nanos = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);

        Read { body_bytes, nanos }
    })
}

/// One run: a body of `parts` parts of `part_bytes` bytes, framed as `framing` says, with `unsent` the connection's
/// most unsent bytes. Answers what the client read.
fn run(parts: u32, part_bytes: u32, framing: Framing, unsent: u32) -> Result<Read, Unheard> {
    let settings = StackSettings::default();
    let engine =
        EngineSettings { read_limit: settings.most_read_bytes().try_into().unwrap_or(u32::MAX), ..Default::default() };
    let tcp = TcpSettings { most_unsent_bytes: unsent, ..TcpSettings::default() };
    let program = Headless::program()
        .with_engine(engine)
        .root_with::<Tcp>(tcp)
        .root_with::<Source>(Plan { parts, part_bytes, framing })
        .root_with::<BenchStack>(settings);
    let mut harness = Harness::of(program)?;
    let stack = harness.root::<BenchStack>();
    let mut bound = None;
    waiting::until(BUDGET, || {
        harness.step();
        bound = harness.state(stack).ok().and_then(|stack| stack.serving.port());

        bound.is_some()
    });
    let Some(port) = bound else {
        panic!("the stack binds its port");
    };

    let reading = client(port);
    while !reading.is_finished() {
        harness.step();
    }

    let Ok(read) = reading.join() else {
        panic!("the client finished");
    };

    Ok(read)
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mebibytes: u32 = args.iter().find_map(|arg| arg.strip_prefix("mebibytes=")?.parse().ok()).unwrap_or(64);
    let unsent: u32 = args.iter().find_map(|arg| arg.strip_prefix("unsent=")?.parse().ok()).unwrap_or(8 * MEBIBYTE);
    let total = u64::from(mebibytes) * u64::from(MEBIBYTE);

    println!("part bytes  framing   MiB/s    body");
    for part_bytes in PART_BYTES {
        let parts = u32::try_from(total / u64::from(part_bytes)).unwrap_or(u32::MAX);
        for framing in [Framing::Chunked, Framing::Known] {
            let runs = (0..3).map(|_| run(parts, part_bytes, framing, unsent)).collect::<Result<Vec<_>, _>>();
            let runs = runs.map_err(|unheard| format!("a run was not heard: {unheard:?}"))?;
            let best = runs.iter().min_by_key(|read| read.nanos).ok_or("three runs ran")?;
            let named = match framing {
                Framing::Chunked => "chunked",
                Framing::Known => "known",
            };
            let whole = if runs.iter().all(|read| read.body_bytes >= total) {
                "whole"
            } else {
                "cut"
            };
            let rate = f64::from(mebibytes) / (best.nanos.max(1) as f64 / 1e9);
            println!("{part_bytes:10}  {named:8} {rate:8.1}    {whole}");
        }
    }

    Ok(())
}
