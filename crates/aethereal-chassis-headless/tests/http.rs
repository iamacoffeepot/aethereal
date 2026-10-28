//! The HTTP capability served on the native host, to a client on a thread of its own (ADR-0035).

use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, ChildOf, Ending, Fault, Handles, More, Number, On, Open, Part, Path,
    Streamed, Streams, Tick, ToAny, ToType, Top, Turn, path,
};
use aethereal_chassis_headless::{Exit, Headless};
use aethereal_data::{Bound, Plain};
use aethereal_http::{
    Exchange, Header, Http, HttpSettings, Incoming, Requests, Respond, Response, Serve, Served, Status, Stopped, Take,
};
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};
use aethereal_tcp::Tcp;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// The port the handler was served on, once it was: 0 until then.
static PORT: AtomicU16 = AtomicU16::new(0);

/// Whether the client is done, so the handler stops the program.
static DONE: AtomicBool = AtomicBool::new(false);

/// What the handler keeps for its serve: nothing.
#[derive(Plain)]
#[kind(name = "aether.test.http.asked")]
struct Asked;

/// A root that serves a port the host picks and answers each request two ticks after it came, so the connection
/// reads on while the request is out: on `/echo`, `200` with the request's body and its method in `x-method`; on `/silent`, never. It stops the
/// program once the client is done, or after a thousand ticks.
#[derive(Bound)]
#[kind(name = "aether.test.http.handler")]
struct Handler {
    requests: Streams<Requests, ()>,
    /// The requests to answer, each with the tick it is answered on.
    waiting: Vec<(Incoming, u64)>,
    ticks: u64,
}

impl ChildOf for Handler {
    type Parent = Top;
}

impl Actor for Handler {
    type Config = Asked;
    type Key = ();
    type Handles = (On<Tick>, Answer<Served, Asked>, Part<Requests>, Answer<Stopped, Number>);
    type BeginSends = (Asks<Path<Http>, Serve, Asked>,);
    type EndSends = ();

    fn begin(_: Asked, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Tick>();
        turn.to(path::<Http>(())).ask(&Serve { port: 0 }, &Asked)?;

        Ok(Self { requests: Streams::new(NonZeroU32::MIN.saturating_add(3)), waiting: Vec::new(), ticks: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Bootstrap<Headless> for Handler {
    const SECTION: &'static str = "handler";
    type Settings = ();
    type Needs = ();
    type Provides = ();

    async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, Headless, Self>) -> Result<(), Unbuilt> {
        cx.begin(&(), Asked)
    }
}

impl Handles<On<Tick>> for Handler {
    type Sends = (ToType<Exchange, Respond>,);

    fn handle(&mut self, _: Tick, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.ticks += 1;
        let (due, later): (Vec<_>, Vec<_>) = self.waiting.drain(..).partition(|&(_, on)| on <= self.ticks);
        self.waiting = later;
        for (incoming, _) in due {
            let mut response = Response::new(Status::Ok, "text/plain", incoming.request.body);
            response.headers.push(Header::new("x-method", &format!("{:?}", incoming.request.method)));
            turn.to(incoming.exchange).send(&Respond { response })?;
        }
        let is_done = DONE.load(Ordering::SeqCst);
        let is_long = self.ticks == 1000;
        let stops = is_done || is_long;
        if stops {
            turn.quit();
        }

        Ok(())
    }
}

impl Handles<Answer<Served, Asked>> for Handler {
    type Sends = (Asks<Streamed<Requests>, Open<Requests>, Number>, ToAny<Streamed<Requests>, More>);

    fn handle(
        &mut self,
        (answered, _): (Answered<Served>, Asked),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Served::Bound { server, port }) = answered else {
            turn.quit();
            return Ok(());
        };
        self.requests.open(turn, server.protocol(), Take, ())?;
        PORT.store(port, Ordering::SeqCst);

        Ok(())
    }
}

impl Handles<Part<Requests>> for Handler {
    type Sends = (ToAny<Streamed<Requests>, More>,);

    fn handle(&mut self, part: Part<Requests>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Some(taken) = self.requests.part(turn, part)? else {
            return Ok(());
        };
        let incoming = taken.value;
        let is_silent = incoming.request.path == "/silent";
        if !is_silent {
            self.waiting.push((incoming, self.ticks + 2));
        }

        Ok(())
    }
}

impl Handles<Answer<Stopped, Number>> for Handler {
    type Sends = ();

    fn handle(&mut self, (_, number): (Answered<Stopped>, Number), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.requests.answered(number);

        Ok(())
    }
}

/// A response as the client read it: its head's text and its body.
#[derive(Debug)]
struct Read {
    head: String,
    body: Vec<u8>,
}

/// A connection to the served port, which gives up on a read after ten seconds.
fn connect() -> TcpStream {
    #[allow(clippy::disallowed_methods)] // The client waits on the host's real threads, which run on real time.
    let started = Instant::now();
    let port = loop {
        let port = PORT.load(Ordering::SeqCst);
        if port != 0 {
            break port;
        }
        assert!(started.elapsed() < Duration::from_secs(30), "the handler is served within 30 seconds");
        #[allow(clippy::disallowed_methods)] // The client is a native thread of its own, outside the engine.
        thread::sleep(Duration::from_millis(5));
    };
    let Ok(stream) = TcpStream::connect(("127.0.0.1", port)) else {
        panic!("the client connects");
    };
    assert!(stream.set_read_timeout(Some(Duration::from_secs(10))).is_ok(), "the read timeout sets");

    stream
}

/// The next response on `stream`, `buffer` holding what was read past the last one.
fn response(stream: &mut TcpStream, buffer: &mut Vec<u8>) -> Read {
    let mut chunk = [0; 4096];
    loop {
        let ended = buffer.windows(4).position(|window| window == b"\r\n\r\n");
        if let Some(end) = ended {
            let head = String::from_utf8_lossy(&buffer[..end]).into_owned();
            let length = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length: "))
                .and_then(|length| length.parse::<usize>().ok())
                .unwrap_or(0);
            let whole = buffer.len() >= end + 4 + length;
            if whole {
                let body = buffer[end + 4..end + 4 + length].to_vec();
                buffer.drain(..end + 4 + length);
                return Read { head, body };
            }
        }
        let Ok(count) = stream.read(&mut chunk) else {
            panic!("a response comes within ten seconds");
        };
        assert_ne!(count, 0, "the server closed before a whole response");
        buffer.extend_from_slice(&chunk[..count]);
    }
}

/// Whether the server closes `stream` once its response is read: the next read finds the end.
fn closes(stream: &mut TcpStream) -> bool {
    let mut byte = [0];

    stream.read(&mut byte).is_ok_and(|count| count == 0)
}

/// The client's three conversations: two requests sent ahead in one write on a connection kept alive, a request
/// that must be refused, and a request its handler never answers.
fn converse() -> (Vec<Read>, Read, bool, Read, bool) {
    let mut kept = connect();
    let ahead = b"POST /echo HTTP/1.1\r\nContent-Length: 5\r\n\r\nhelloGET /echo HTTP/1.1\r\nConnection: close\r\n\r\n";
    assert!(kept.write_all(ahead).is_ok(), "the client writes");
    let mut buffer = Vec::new();
    let answered = vec![response(&mut kept, &mut buffer), response(&mut kept, &mut buffer)];
    let kept_closes = closes(&mut kept);

    let mut smuggled = connect();
    assert!(
        smuggled.write_all(b"POST /echo HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n").is_ok(),
        "the client writes"
    );
    let refused = response(&mut smuggled, &mut Vec::new());
    let refused_closes = closes(&mut smuggled);

    let mut silent = connect();
    assert!(silent.write_all(b"GET /silent HTTP/1.1\r\n\r\n").is_ok(), "the client writes");
    let timed_out = response(&mut silent, &mut Vec::new());

    (answered, refused, refused_closes, timed_out, kept_closes)
}

#[test]
fn a_server_answers_requests_sent_ahead_in_order_and_refuses_or_times_out_what_it_must() {
    // Catches a body framed past its `Content-Length` into the next request, bytes read past a request lost, credit
    // withheld while a request is out and never granted back so the second request is never read, responses written
    // out of order or with the wrong length, `Connection: close` not honoured, a refused request closed before its
    // response is sent or never closed, and a handler that never responds left waiting without a `504`.
    // Chunks of 16 bytes, one granted at a time, so the second request sent ahead is read only once the credit
    // withheld while the first was out is granted back.
    let settings = HttpSettings { chunk_bytes: 16, window: 1, answer_within_millis: 200, ..HttpSettings::default() };
    let built =
        Headless::program().with_cycle::<(Tick,)>().root::<Tcp>().root_with::<Http>(settings).root::<Handler>().build();
    let Ok(headless) = built else {
        panic!("the program builds");
    };
    #[allow(clippy::disallowed_methods)] // The client is a native thread of its own, outside the engine.
    let client = thread::spawn(|| {
        let conversed = converse();
        DONE.store(true, Ordering::SeqCst);
        conversed
    });

    let exit = headless.run(Duration::from_millis(2), Duration::from_mins(1));
    let Ok((answered, refused, refused_closes, timed_out, kept_closes)) = client.join() else {
        panic!("the client finished");
    };

    assert_eq!(exit, Exit::Stopped);
    let heads: Vec<&str> = answered.iter().map(|read| read.head.as_str()).collect();
    assert!(heads[0].starts_with("HTTP/1.1 200 OK") && heads[0].contains("x-method: Post"), "{heads:?}");
    assert!(heads[1].starts_with("HTTP/1.1 200 OK") && heads[1].contains("x-method: Get"), "{heads:?}");
    assert!(heads[1].contains("connection: close"), "{heads:?}");
    assert_eq!(answered[0].body, b"hello");
    assert!(answered[1].body.is_empty());
    assert!(kept_closes, "a request that asked to close is closed once answered");
    assert!(refused.head.starts_with("HTTP/1.1 501"), "{refused:?}");
    assert!(refused_closes, "a refused request's connection closes once its response is sent");
    assert!(timed_out.head.starts_with("HTTP/1.1 504"), "{timed_out:?}");
}
