//! How fast a streamed HTTP response's body goes from its source to a client over loopback (ADR-0035).
//!
//! Run with `cargo run --release -p aethereal-chassis-headless --example http_stream [mebibytes=N] [unsent=BYTES]`.
//! A source serves a body of `N` MiB (by default 64) in parts of 4 KiB, 64 KiB, and 1 MiB, each part the same blob,
//! so the bench costs the exchange and the connection, not the source. The body is written chunked (its length not
//! known) and with a `Content-Length` (known), to a client on a thread of its own that reads it to the close. Each line
//! is the best of three runs: MiB a second, from the request written to the close read.
//!
//! The program is the HTTP server on TCP with the engine's default settings, but for the connection's most unsent
//! bytes, `unsent` (by default 8 MiB): a write of a window of 1 MiB parts must fit it whole. The host is stepped by the
//! harness, which is the server's handler and answers the request with the streamed body (ADR-0036).

#![allow(clippy::print_stdout)] // A bench prints its table.
#![allow(clippy::cast_precision_loss)] // A rate printed to one place loses nothing it shows.

use aethereal_actor::{
    Actor, Answered, Anyone, Beginning, Bytes, ChildOf, Ended, Ending, Enough, Fault, Handles, More, Open, Part,
    Receives, Serving, ToAny, ToSender, Top, Turn,
};
use aethereal_chassis_headless::Headless;
use aethereal_data::{Bound, Plain};
use aethereal_http::{
    Handling, Header, Http, HttpSettings, Request, Response, ResponseBody, ResponseBytes, ResponseLength,
    ResponseSource, Serve, Served, SourceEnd, Status, StreamedBody,
};
use aethereal_substrate::harness::{Harness, Unheard};
use aethereal_substrate::{Bootstrap, Bootstrapping, EngineSettings, Host, Unbuilt};
use aethereal_tcp::{Interface, Tcp, TcpSettings};
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

/// What a source sends for each body it is opened for: `parts` parts of `part_bytes` bytes.
#[derive(Debug, Clone, Copy, Plain)]
#[kind(name = "aether.bench.http.plan")]
#[settings]
struct Plan {
    parts: u32,
    part_bytes: u32,
}

/// A root that sends each [`ResponseSource`] stream opened on it the same part, as many times as its plan says, a part
/// for each credit it has.
#[derive(Bound)]
#[kind(name = "aether.bench.http.source")]
struct Source {
    plan: Plan,
    /// The part it sends, put once.
    part: Bytes,
    serving: Serving<ResponseSource>,
    /// The parts sent of the stream open.
    sent: u32,
}

impl ChildOf for Source {
    type Parent = Top;
}

impl Actor for Source {
    type Config = Plan;
    type Key = ();
    type Handles = (Open<ResponseSource>, More, Enough, Ended);
    type BeginSends = ();
    type EndSends = ();

    fn begin(plan: Plan, turn: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        let part_bytes = usize::try_from(plan.part_bytes).unwrap_or(usize::MAX);
        let mut part = turn.buffer(part_bytes);
        part.resize(part_bytes, b'x');
        let Ok(part) = turn.put(part) else {
            panic!("the source's part fits the engine's blobs");
        };

        Ok(Self { plan, part, serving: Serving::new(), sent: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Bootstrap<Headless> for Source {
    const SECTION: &'static str = "source";
    type Settings = Plan;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(plan: Plan, (): (), cx: &mut Bootstrapping<'_, Headless, Self>) -> Result<(), Unbuilt> {
        cx.begin(&(), plan)
    }
}

impl Handles<Open<ResponseSource>> for Source {
    type Sends = (ToSender<SourceEnd>,);
    type From = Anyone;

    fn handle(&mut self, open: Open<ResponseSource>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.serving.open(turn, open)?;
        self.sent = 0;

        Ok(())
    }
}

impl Handles<More> for Source {
    type Sends = (ToAny<(Part<ResponseSource>,), Part<ResponseSource>>,);
    type From = Anyone;

    fn handle(&mut self, more: More, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
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
}

impl Handles<Enough> for Source {
    type Sends = ();
    type From = Anyone;

    fn handle(&mut self, enough: Enough, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        if let Some(stopped) = self.serving.stop(turn, &enough) {
            stopped.finish(turn, &SourceEnd::Stopped)?;
        }

        Ok(())
    }
}

impl Receives<Ended> for Source {
    type Sends = ();

    fn handle(&mut self, ended: Ended, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.serving.ended(&ended);

        Ok(())
    }
}

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

/// One run: a body of `parts` parts of `part_bytes` bytes, its length `known` or not, with `unsent` the connection's
/// most unsent bytes. Answers what the client read.
fn run(parts: u32, part_bytes: u32, known: bool, unsent: u32) -> Result<Read, Unheard> {
    let settings = HttpSettings::default();
    let engine =
        EngineSettings { read_limit: settings.most_read_bytes().try_into().unwrap_or(u32::MAX), ..Default::default() };
    let tcp = TcpSettings { most_unsent_bytes: unsent, ..TcpSettings::default() };
    let program = Headless::program()
        .with_engine(engine)
        .root_with::<Tcp>(tcp)
        .root_with::<Http>(settings)
        .root_with::<Source>(Plan { parts, part_bytes });
    let mut harness = Harness::of(program)?;
    let http = harness.root::<Http>();
    let handler = harness.protocol::<Handling>();
    let serving = harness.ask(http, &Serve { port: 0, interface: Interface::Settings, handler, mounts: Vec::new() })?;
    let Answered::Yes(Served::Bound { port, .. }) = harness.answer(serving)? else {
        panic!("the port is bound");
    };

    let reading = client(port);
    let source = harness.root::<Source>();
    let request = harness.asked::<Request>()?;
    let length = if known {
        ResponseLength::Known(u64::from(parts) * u64::from(part_bytes))
    } else {
        ResponseLength::Unknown
    };
    let body = StreamedBody { source: source.protocol(), body: 1, length };
    let response = Response {
        status: Status::Ok,
        headers: vec![Header::new("content-type", "application/octet-stream")],
        body: ResponseBody::Streamed(body),
    };
    harness.reply(request, &response)?;
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
        for known in [false, true] {
            let runs = (0..3).map(|_| run(parts, part_bytes, known, unsent)).collect::<Result<Vec<_>, _>>();
            let runs = runs.map_err(|unheard| format!("a run was not heard: {unheard:?}"))?;
            let best = runs.iter().min_by_key(|read| read.nanos).ok_or("three runs ran")?;
            let framing = if known {
                "known"
            } else {
                "chunked"
            };
            let whole = if runs.iter().all(|read| read.body_bytes >= total) {
                "whole"
            } else {
                "cut"
            };
            let rate = f64::from(mebibytes) / (best.nanos.max(1) as f64 / 1e9);
            println!("{part_bytes:10}  {framing:8} {rate:8.1}    {whole}");
        }
    }

    Ok(())
}
