//! The HTTP capability served on the native host, the test its handler, to clients on threads of their own (ADR-0035,
//! ADR-0036).

use aethereal_actor::{
    Actor, Answer, Answered, Anyone, Asks, Beginning, ChildOf, Ended, Ending, Enough, Fault, Handles, More, Number,
    Open, Part, Receives, Serving, StreamKey, Streamed, Streams, ToAny, ToSender, Top, Turn, Unanswered,
};
use aethereal_chassis_headless::Headless;
use aethereal_chassis_headless::waiting::{self, BUDGET};
use aethereal_data::{Bound, Plain};
use aethereal_http::{
    BlobBytesTooLow, BodyRead, Handling, Header, Http, HttpSettings, Mount, ReadLimitTooLow, Request, RequestBody,
    Response, ResponseBody, ResponseBytes, ResponseLength, ResponseSource, Serve, Served, SourceEnd, Status,
    StreamedBody, StreamedRequest,
};
use aethereal_substrate::harness::{Harness, Held, Unheard};
use aethereal_substrate::{Bootstrap, Bootstrapping, EngineSettings, Host, Unbuilt};
use aethereal_tcp::{Interface, Tcp};
use std::io::{Read as _, Write as _};
use std::mem;
use std::net::TcpStream;
use std::num::NonZeroU32;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// A response as the client read it: its head's text and its body.
#[derive(Debug)]
struct Read {
    head: String,
    body: Vec<u8>,
}

/// A host running `settings`, the test the handler of the port it serves.
fn served(settings: HttpSettings) -> Result<(Harness<Headless>, u16), Unheard> {
    let engine = EngineSettings {
        read_limit: settings.most_read_bytes().try_into().unwrap_or(u32::MAX),
        ..EngineSettings::default()
    };
    let mut harness = Harness::of(Headless::program().with_engine(engine).root::<Tcp>().root_with::<Http>(settings))?;
    let http = harness.root::<Http>();
    let handler = harness.protocol::<Handling>();
    let serving = harness.ask(http, &Serve { port: 0, interface: Interface::Settings, handler, mounts: Vec::new() })?;
    let Answered::Yes(Served::Bound { port, .. }) = harness.answer(serving)? else {
        panic!("the port is bound");
    };

    Ok((harness, port))
}

/// A connection to `port`, which gives up on a read after ten seconds.
fn connect(port: u16) -> TcpStream {
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

/// A client on a thread of its own, outside the engine, which sends `sent` to `port` and reads `responses` responses,
/// and whether the server then closed.
fn client(port: u16, sent: impl AsRef<[u8]> + Send + 'static, responses: usize) -> JoinHandle<(Vec<Read>, bool)> {
    #[allow(clippy::disallowed_methods)] // The client is a native thread of its own, outside the engine.
    thread::spawn(move || {
        let mut stream = connect(port);
        assert!(stream.write_all(sent.as_ref()).is_ok(), "the client writes");
        let mut buffer = Vec::new();
        let read = (0..responses).map(|_| response(&mut stream, &mut buffer)).collect();
        let closed = closes(&mut stream);

        (read, closed)
    })
}

/// What `client` read, the harness stepping the host until it has.
fn heard(harness: &mut Harness<Headless>, client: JoinHandle<(Vec<Read>, bool)>) -> (Vec<Read>, bool) {
    while !client.is_finished() {
        harness.step();
    }
    let Ok(read) = client.join() else {
        panic!("the client finished");
    };

    read
}

/// `request` echoed: `200` with its body, and its method in `x-method`.
fn echoed(request: &Held<Request>) -> Response {
    let mut response = Response::new(Status::Ok, "text/plain", request.request.body.clone());
    response.headers.push(Header::new("x-method", &format!("{:?}", request.request.method)));

    response
}

#[test]
fn requests_sent_ahead_are_answered_in_order() -> Result<(), Unheard> {
    // Catches a body framed past its `Content-Length` into the next request, bytes read past a request lost, credit
    // withheld while a request is out and never granted back so the second request is never read, responses written
    // out of order or with the wrong length, and `Connection: close` not honoured.
    // Chunks of 16 bytes, one granted at a time, so the second request is read only once the credit withheld while the
    // first was out is granted back.
    let settings = HttpSettings { chunk_bytes: 16, window: 1, ..HttpSettings::default() };
    let (mut harness, port) = served(settings)?;
    let ahead = b"POST /echo HTTP/1.1\r\nContent-Length: 5\r\n\r\nhelloGET /echo HTTP/1.1\r\nConnection: close\r\n\r\n";
    let client = client(port, ahead, 2);

    let first = harness.asked::<Request>()?;
    assert_eq!(harness.read(&first.request.body), Some(&b"hello"[..]));
    let answer = echoed(&first);
    harness.reply(first, &answer)?;
    let second = harness.asked::<Request>()?;
    let answer = echoed(&second);
    harness.reply(second, &answer)?;

    let (answered, closed) = heard(&mut harness, client);
    assert!(answered[0].head.starts_with("HTTP/1.1 200 OK"), "{answered:?}");
    assert!(answered[0].head.contains("x-method: Post"), "{answered:?}");
    assert!(answered[1].head.starts_with("HTTP/1.1 200 OK"), "{answered:?}");
    assert!(answered[1].head.contains("x-method: Get"), "{answered:?}");
    assert!(answered[1].head.contains("connection: close"), "{answered:?}");
    assert_eq!(answered[0].body, b"hello");
    assert!(answered[1].body.is_empty());
    assert!(closed, "a request that asked to close is closed once answered");

    Ok(())
}

#[test]
fn a_chunked_request_cut_across_reads_is_handed_on_whole_and_the_request_after_it_follows() -> Result<(), Unheard> {
    // Catches a chunked body handed on before its last chunk, or with its size lines and trailer left in it; the bytes
    // after its trailer lost or taken into the body, so the pipelined request after it is never answered; and
    // `100 Continue` written only for a `Content-Length`, so a client that waits to send a chunked body stalls.
    // Chunks of 16 bytes, so the body's size lines, data, and trailer are cut across reads.
    let settings = HttpSettings { chunk_bytes: 16, window: 1, ..HttpSettings::default() };
    let (mut harness, port) = served(settings)?;
    let ahead = b"POST /echo HTTP/1.1\r\nTransfer-Encoding: chunked\r\nExpect: 100-continue\r\n\r\n\
        5\r\nhello\r\n7;name=value\r\n, world\r\n0\r\nx-trailer: passed\r\n\r\n\
        GET /echo HTTP/1.1\r\nConnection: close\r\n\r\n";
    let client = client(port, ahead, 3);

    let first = harness.asked::<Request>()?;
    assert_eq!(harness.read(&first.request.body), Some(&b"hello, world"[..]));
    let answer = echoed(&first);
    harness.reply(first, &answer)?;
    let second = harness.asked::<Request>()?;
    let answer = echoed(&second);
    harness.reply(second, &answer)?;

    let (answered, closed) = heard(&mut harness, client);
    assert!(answered[0].head.starts_with("HTTP/1.1 100 Continue"), "{answered:?}");
    assert!(answered[1].head.contains("x-method: Post"), "{answered:?}");
    assert_eq!(answered[1].body, b"hello, world");
    assert!(answered[2].head.contains("x-method: Get"), "{answered:?}");
    assert!(closed, "the request after the chunked one asked to close");

    Ok(())
}

#[test]
fn a_chunked_request_decoded_past_the_most_body_bytes_is_refused() -> Result<(), Unheard> {
    // Catches a chunked body, which has no length to check before it is read, gathered past `most_body_bytes`
    // and handed on, where a body with a `Content-Length` that long is refused.
    let settings = HttpSettings { most_body_bytes: 8, ..HttpSettings::default() };
    let (mut harness, port) = served(settings)?;
    let client = client(
        port,
        b"POST /echo HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n5\r\nworld\r\n0\r\n\r\n",
        1,
    );

    let (refused, closed) = heard(&mut harness, client);

    assert!(refused[0].head.starts_with("HTTP/1.1 413"), "{refused:?}");
    assert!(closed, "a refused request's connection closes once its response is sent");
    assert_eq!(harness.rest(), Vec::new());

    Ok(())
}

#[test]
fn a_refused_request_reaches_no_handler_and_closes_once_its_response_is_sent() -> Result<(), Unheard> {
    // Catches a request that must be refused handed on to the handler, a refused request closed before its response
    // is sent or never closed, and a transfer coding the exchange does not decode read as if it were chunked.
    let (mut harness, port) = served(HttpSettings::default())?;
    let client = client(port, b"POST /echo HTTP/1.1\r\nTransfer-Encoding: gzip\r\n\r\n", 1);

    let (refused, closed) = heard(&mut harness, client);

    assert!(refused[0].head.starts_with("HTTP/1.1 501"), "{refused:?}");
    assert!(closed, "a refused request's connection closes once its response is sent");
    assert_eq!(harness.rest(), Vec::new());

    Ok(())
}

#[test]
fn a_handler_that_never_responds_is_timed_out() -> Result<(), Unheard> {
    // Catches a handler that never responds left waiting without a `504`, and a `504` written before the request's
    // deadline: the deadline is the exchange's ask of its handler's, and the engine answers it `Expired` then.
    let settings = HttpSettings { answer_within_millis: 200, ..HttpSettings::default() };
    let (mut harness, port) = served(settings)?;
    #[allow(clippy::disallowed_methods)] // The test times the request against its deadline on real time.
    let sent = Instant::now();
    let client = client(port, b"GET /silent HTTP/1.1\r\n\r\n", 1);

    let _silent = harness.asked::<Request>()?;

    let (timed_out, _) = heard(&mut harness, client);
    assert!(timed_out[0].head.starts_with("HTTP/1.1 504"), "{timed_out:?}");
    assert!(sent.elapsed() >= Duration::from_millis(200), "it waited for its deadline: {:?}", sent.elapsed());

    Ok(())
}

#[test]
fn an_idle_connection_is_closed_once_its_wait_for_a_request_runs_out() -> Result<(), Unheard> {
    // Catches an exchange that never times its first wait: a peer that connects and sends nothing would hold a
    // connection, and a slot of the server's, for good.
    let settings = HttpSettings { idle_within_millis: 200, ..HttpSettings::default() };
    let (mut harness, port) = served(settings)?;
    let client = client(port, b"", 0);

    let (_, closed) = heard(&mut harness, client);

    assert!(closed, "the server closes a connection that sent nothing in time");

    Ok(())
}

#[test]
fn a_deadline_of_a_request_already_answered_does_not_end_the_next() -> Result<(), Unheard> {
    // Catches a sleep for a phase the exchange has left still acting when it comes: the first request's deadline,
    // passed while the connection waits between requests, would answer `504` or close it.
    let settings = HttpSettings { answer_within_millis: 200, ..HttpSettings::default() };
    let (mut harness, port) = served(settings)?;
    #[allow(clippy::disallowed_methods)] // The client is a native thread of its own, outside the engine.
    let client = thread::spawn(move || {
        let mut stream = connect(port);
        let mut buffer = Vec::new();
        assert!(stream.write_all(b"GET /first HTTP/1.1\r\n\r\n").is_ok(), "the client writes");
        let first = response(&mut stream, &mut buffer);
        // Past the first request's deadline, so a sleep left from it would have come.
        #[allow(clippy::disallowed_methods)] // The client waits on real time, outside the engine.
        thread::sleep(Duration::from_millis(400));
        assert!(stream.write_all(b"GET /second HTTP/1.1\r\nConnection: close\r\n\r\n").is_ok(), "the client writes");
        let second = response(&mut stream, &mut buffer);

        (vec![first, second], closes(&mut stream))
    });

    for _ in 0..2 {
        let request = harness.asked::<Request>()?;
        let answer = echoed(&request);
        harness.reply(request, &answer)?;
    }

    let (answered, _) = heard(&mut harness, client);
    assert!(answered[0].head.starts_with("HTTP/1.1 200"), "{answered:?}");
    assert!(answered[1].head.starts_with("HTTP/1.1 200"), "{answered:?}");

    Ok(())
}

#[test]
fn a_handler_that_ended_is_a_503_and_one_that_failed_a_500() -> Result<(), Unheard> {
    // Catches an exchange that mistakes why its handler did not answer, or hangs on it, since a test's stand-in could
    // only ever answer.
    let cases = [
        (Unanswered::Ended, "HTTP/1.1 503"),
        (Unanswered::Failed, "HTTP/1.1 500"),
        (Unanswered::Unreplied, "HTTP/1.1 500"),
    ];
    for (why, status) in cases {
        let (mut harness, port) = served(HttpSettings::default())?;
        let client = client(port, b"GET /gone HTTP/1.1\r\nConnection: close\r\n\r\n", 1);

        let request = harness.asked::<Request>()?;
        harness.unanswered(request, why)?;

        let (read, _) = heard(&mut harness, client);
        assert!(read[0].head.starts_with(status), "{why:?}: {read:?}");
    }

    Ok(())
}

#[test]
fn http_does_not_start_where_a_request_could_not_be_read() {
    // Catches the check dropped, which lets one client end a handler with a request sent in small writes.
    let settings = HttpSettings::default();
    let needed = settings.most_read_bytes();
    let engine = EngineSettings::default();
    let program = Headless::program().with_engine(engine).root::<Tcp>().root_with::<Http>(settings);

    let Err(Unheard::Program(why)) = Harness::of(program) else {
        panic!("the program does not start");
    };

    assert!(why.contains(&ReadLimitTooLow { read_limit: 1024, needed }.to_string()), "{why}");
}

#[test]
fn http_does_not_start_where_a_requests_body_could_not_be_held() {
    // Catches the check dropped, which lets every request whose body the blobs cannot hold be answered `503`.
    let settings = HttpSettings::default();
    let read_limit = settings.most_read_bytes().try_into().unwrap_or(u32::MAX);
    let needed = settings.most_blob_bytes();
    let engine = EngineSettings { read_limit, mail_bytes: 1 << 24, blob_bytes: 1 << 20, ..EngineSettings::default() };
    let program = Headless::program().with_engine(engine).root::<Tcp>().root_with::<Http>(settings);

    let Err(Unheard::Program(why)) = Harness::of(program) else {
        panic!("the program does not start");
    };

    assert!(why.contains(&BlobBytesTooLow { blob_bytes: 1 << 20, needed }.to_string()), "{why}");
}

/// What a reader's begin is given: nothing.
#[derive(Plain)]
#[kind(name = "aether.test.http.unset")]
struct Unset;

/// A root that handles a mount's requests: it opens each body's stream, kept one part ahead, keeps each part as it
/// comes, and once the body ends answers the request `200` with every byte it took. A stream is opened and its parts
/// taken only by [`Streams`], which only an actor holds.
#[derive(Bound)]
#[kind(name = "aether.test.http.reader")]
struct Reader {
    /// Each body's stream, and the request it answers once the body ends.
    bodies: Streams<RequestBody, aethereal_actor::Held<Response>>,
    /// Each part taken, in order.
    parts: Vec<Vec<u8>>,
    /// How each body was read.
    read: Vec<BodyRead>,
}

impl ChildOf for Reader {
    type Parent = Top;
}

impl Actor for Reader {
    type Config = Unset;
    type Key = ();
    type Handles = (StreamedRequest, Part<RequestBody>, Answer<BodyRead, Number>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Unset, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { bodies: Streams::new(NonZeroU32::MIN), parts: Vec::new(), read: Vec::new() })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Bootstrap<Headless> for Reader {
    const SECTION: &'static str = "reader";
    type Settings = ();
    type Needs = ();
    type Provides = ();

    async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, Headless, Self>) -> Result<(), Unbuilt> {
        cx.begin(&(), Unset)
    }
}

impl Handles<StreamedRequest> for Reader {
    type Sends = (
        ToSender<Response>,
        Asks<Streamed<RequestBody>, Open<RequestBody>, Number>,
        ToAny<Streamed<RequestBody>, More>,
    );
    type From = Anyone;

    fn handle(&mut self, request: StreamedRequest, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let held = turn.hold()?;
        self.bodies.open(turn, request.body, request.read_body(), held)?;

        Ok(())
    }
}

impl Handles<Part<RequestBody>> for Reader {
    type Sends = (ToAny<Streamed<RequestBody>, More>,);
    type From = Anyone;

    fn handle(&mut self, part: Part<RequestBody>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Some(taken) = self.bodies.part(turn, part)? else {
            return Ok(());
        };
        self.parts.push(turn.read(&taken.value.bytes).to_vec());

        Ok(())
    }
}

impl Receives<Answer<BodyRead, Number>> for Reader {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, number): (Answered<BodyRead>, Number),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Some(held) = self.bodies.answered(number) else {
            return Ok(());
        };
        if let Answered::Yes(read) = answered {
            self.read.push(read);
        }
        let mut body = turn.buffer(self.parts.iter().map(Vec::len).sum());
        body.extend(self.parts.iter().flatten());
        let Ok(body) = turn.put(body) else {
            return Ok(());
        };
        turn.answer(held, &Response::new(Status::Ok, "application/octet-stream", body))?;

        Ok(())
    }
}

/// A host running `settings` with a reader, which serves a port to the test, and streams what is under `/upload` to
/// the reader, up to `most_body_bytes` of each body.
fn mounted(settings: HttpSettings, most_body_bytes: u64) -> Result<(Harness<Headless>, u16), Unheard> {
    let engine = EngineSettings {
        read_limit: settings.most_read_bytes().try_into().unwrap_or(u32::MAX),
        ..EngineSettings::default()
    };
    let program = Headless::program().with_engine(engine).root::<Tcp>().root_with::<Http>(settings).root::<Reader>();
    let mut harness = Harness::of(program)?;
    let http = harness.root::<Http>();
    let handler = harness.protocol::<Handling>();
    let reader = harness.root::<Reader>().protocol();
    let mounts = vec![Mount { prefix: String::from("/upload"), most_body_bytes, handler: reader }];

    let serving = harness.ask(http, &Serve { port: 0, interface: Interface::Settings, handler, mounts })?;
    let Answered::Yes(Served::Bound { port, .. }) = harness.answer(serving)? else {
        panic!("the port is bound");
    };

    Ok((harness, port))
}

/// The reader once it has been told how a body was read, the harness stepping the host until it has or the wait is
/// spent.
fn reader(harness: &mut Harness<Headless>) -> Result<&Reader, Unheard> {
    let reader = harness.root::<Reader>();
    waiting::until(BUDGET, || {
        harness.step();

        harness.state(reader).is_ok_and(|reader| !reader.read.is_empty())
    });

    harness.state(reader)
}

/// A body of `length` bytes, each its place's letter.
fn letters(length: u8) -> Vec<u8> {
    (0..length).map(|place| b'a' + place % 26).collect()
}

/// `body` written in chunks of seven bytes, the last chunk and an empty trailer after them.
fn chunked(body: &[u8]) -> Vec<u8> {
    let mut written = Vec::new();
    for chunk in body.chunks(7) {
        written.extend_from_slice(format!("{:x}\r\n", chunk.len()).as_bytes());
        written.extend_from_slice(chunk);
        written.extend_from_slice(b"\r\n");
    }
    written.extend_from_slice(b"0\r\n\r\n");

    written
}

#[test]
fn a_body_streams_to_its_mount_whole_and_in_order_across_small_reads() -> Result<(), Unheard> {
    // Catches a mount's body gathered whole before it is handed on, a part sent past the reader's credit of one and
    // dropped, bytes lost or reordered where a read cuts a chunk's size line or data, a chunk's framing left in what
    // the reader is sent, and the server's `most_body_bytes` held to in place of the mount's, which would refuse both.
    // Chunks of 16 bytes, one granted at a time, so the body comes in many reads.
    let settings = HttpSettings { chunk_bytes: 16, window: 1, most_body_bytes: 8, ..HttpSettings::default() };
    let body = letters(200);
    let by_length =
        [b"POST /upload/pack HTTP/1.1\r\nContent-Length: 200\r\nConnection: close\r\n\r\n".to_vec(), body.clone()]
            .concat();
    let in_chunks = [
        b"POST /upload/pack HTTP/1.1\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec(),
        chunked(&body),
    ]
    .concat();
    for sent in [by_length, in_chunks] {
        let (mut harness, port) = mounted(settings.clone(), 1 << 20)?;
        let client = client(port, sent, 1);

        let (answered, closed) = heard(&mut harness, client);
        let reader = reader(&mut harness)?;
        assert_eq!(reader.read, [BodyRead::Whole]);
        assert_eq!(reader.parts.concat(), body);
        assert!(reader.parts.len() > 1, "the body came in parts as it was read: {}", reader.parts.len());
        assert!(answered[0].head.starts_with("HTTP/1.1 200"), "{answered:?}");
        assert_eq!(answered[0].body, body);
        assert!(closed, "a request that asked to close is closed once answered");
    }

    Ok(())
}

#[test]
fn a_chunked_body_past_its_mounts_limit_ends_too_large_and_is_refused() -> Result<(), Unheard> {
    // Catches a mount's limit not checked as its chunked body is decoded, so a body without a length runs past it
    // unbounded, and a body refused without telling the reader why its stream ended.
    let (mut harness, port) = mounted(HttpSettings::default(), 8)?;
    let client = client(
        port,
        b"POST /upload HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n5\r\nworld\r\n0\r\n\r\n",
        1,
    );

    let (refused, closed) = heard(&mut harness, client);

    assert!(refused[0].head.starts_with("HTTP/1.1 413"), "{refused:?}");
    assert!(closed, "a refused request's connection closes once its response is sent");
    assert_eq!(reader(&mut harness)?.read, [BodyRead::TooLarge]);

    Ok(())
}

#[test]
fn a_body_that_stops_coming_ends_stalled_and_the_connection_closes() -> Result<(), Unheard> {
    // Catches a streamed body left without a deadline, so a peer that stops sending holds the exchange and the reader
    // for good, and one closed without telling the reader why its stream ended.
    let settings = HttpSettings { stall_within_millis: 200, ..HttpSettings::default() };
    let (mut harness, port) = mounted(settings, 1 << 20)?;
    let client = client(port, b"POST /upload HTTP/1.1\r\nContent-Length: 100\r\n\r\nonly some", 0);

    let (_, closed) = heard(&mut harness, client);

    assert!(closed, "the server closes a body that stopped coming");
    let reader = reader(&mut harness)?;
    assert_eq!(reader.read, [BodyRead::Stalled]);
    assert_eq!(reader.parts.concat(), b"only some");

    Ok(())
}

#[test]
fn a_request_after_a_streamed_body_waits_for_its_answer_and_one_outside_every_mount_is_asked_whole()
-> Result<(), Unheard> {
    // Catches the bytes after a streamed body read into it or lost, so the request after it is never answered; that
    // request asked before the streamed one is answered, so responses go out of order; and a request under no mount
    // streamed, or held to the mount's limit, where it is asked of the server's handler whole.
    let (mut harness, port) = mounted(HttpSettings::default(), 1 << 20)?;
    let ahead = b"POST /upload HTTP/1.1\r\nContent-Length: 6\r\n\r\nstreamPOST /echo HTTP/1.1\r\nContent-Length: 5\r\nConnection: close\r\n\r\nwhole";
    let client = client(port, ahead, 2);

    let whole = harness.asked::<Request>()?;
    assert_eq!(harness.read(&whole.request.body), Some(&b"whole"[..]));
    let answer = echoed(&whole);
    harness.reply(whole, &answer)?;

    let (answered, closed) = heard(&mut harness, client);
    assert_eq!(answered[0].body, b"stream");
    assert!(answered[1].head.contains("x-method: Post"), "{answered:?}");
    assert_eq!(answered[1].body, b"whole");
    assert!(closed, "the request after the streamed one asked to close");
    assert_eq!(reader(&mut harness)?.read, [BodyRead::Whole]);

    Ok(())
}

/// What a source sends for each body it is opened for: `parts` parts of `part_bytes` bytes, each part its place's
/// letter, then the end `ends` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.http.plan")]
struct Plan {
    parts: u32,
    part_bytes: u32,
    ends: Ends,
}

/// How a source ends a body it sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.http.ends")]
enum Ends {
    /// Whole, once every part is sent.
    Whole,
    /// Failed, once this many parts are sent.
    FailsAfter(u32),
}

impl Plan {
    /// The body it sends whole.
    fn body(self) -> Vec<u8> {
        (0..self.parts).flat_map(|place| part(place, self.part_bytes)).collect()
    }
}

/// The part a source sends at `place`, of `bytes` bytes.
fn part(place: u32, bytes: u32) -> Vec<u8> {
    let letter = b'a' + u8::try_from(place % 26).unwrap_or(0);

    vec![letter; usize::try_from(bytes).unwrap_or(0)]
}

/// A root that sends streamed responses' bodies: each [`ResponseSource`] stream opened on it is sent as its plan says,
/// a part for each credit it has. It keeps the most credit it was ever granted ahead, and how each stream ended.
#[derive(Bound)]
#[kind(name = "aether.test.http.source")]
struct Source {
    plan: Plan,
    serving: Serving<ResponseSource>,
    /// The parts sent of the stream open.
    sent: u32,
    /// The most parts it was ever let send ahead.
    most_credit: u32,
    /// How each stream ended, in order.
    ended: Vec<SourceEnd>,
}

impl ChildOf for Source {
    type Parent = Top;
}

impl Actor for Source {
    type Config = Plan;
    type Key = ();
    type Handles = (Plan, Open<ResponseSource>, More, Enough, Ended);
    type BeginSends = ();
    type EndSends = ();

    fn begin(plan: Plan, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { plan, serving: Serving::new(), sent: 0, most_credit: 0, ended: Vec::new() })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Bootstrap<Headless> for Source {
    const SECTION: &'static str = "source";
    type Settings = ();
    type Needs = ();
    type Provides = ();

    async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, Headless, Self>) -> Result<(), Unbuilt> {
        cx.begin(&(), Plan { parts: 0, part_bytes: 0, ends: Ends::Whole })
    }
}

impl Handles<Plan> for Source {
    type Sends = ();
    type From = Anyone;

    fn handle(&mut self, plan: Plan, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.plan = plan;

        Ok(())
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
        loop {
            let Some(stream) = self.serving.get_mut(key) else {
                return Ok(());
            };
            self.most_credit = self.most_credit.max(stream.credit());
            let has_credit = stream.credit() > 0;
            let has_parts = self.sent < self.plan.parts;
            let sends = has_credit && has_parts;
            if !sends {
                break;
            }
            let Ok(bytes) = turn.put(part(self.sent, self.plan.part_bytes)) else {
                return Ok(());
            };
            assert!(stream.send(turn, ResponseBytes { bytes }).is_ok(), "a part within credit sends");
            self.sent += 1;
            if self.plan.ends == Ends::FailsAfter(self.sent) {
                return self.finish(turn, key, SourceEnd::Failed);
            }
        }
        if self.sent == self.plan.parts {
            return self.finish(turn, key, SourceEnd::Whole);
        }

        Ok(())
    }
}

impl Handles<Enough> for Source {
    type Sends = ();
    type From = Anyone;

    fn handle(&mut self, enough: Enough, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let Some(stopped) = self.serving.stop(turn, &enough) else {
            return Ok(());
        };
        stopped.finish(turn, &SourceEnd::Stopped)?;
        self.ended.push(SourceEnd::Stopped);

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

impl Source {
    /// Ends the stream `key` with `end`.
    fn finish<S>(&mut self, turn: &mut Turn<'_, Self, S>, key: StreamKey, end: SourceEnd) -> Result<(), Fault> {
        if let Some(stream) = self.serving.take(key) {
            stream.finish(turn, &end)?;
            self.ended.push(end);
        }

        Ok(())
    }
}

/// A host running `settings` with a source, which serves a port to the test, and the port.
fn sourced(settings: HttpSettings) -> Result<(Harness<Headless>, u16), Unheard> {
    let engine = EngineSettings {
        read_limit: settings.most_read_bytes().try_into().unwrap_or(u32::MAX),
        ..EngineSettings::default()
    };
    let program = Headless::program().with_engine(engine).root::<Tcp>().root_with::<Http>(settings).root::<Source>();
    let mut harness = Harness::of(program)?;
    let http = harness.root::<Http>();
    let handler = harness.protocol::<Handling>();
    let serving = harness.ask(http, &Serve { port: 0, interface: Interface::Settings, handler, mounts: Vec::new() })?;
    let Answered::Yes(Served::Bound { port, .. }) = harness.answer(serving)? else {
        panic!("the port is bound");
    };

    Ok((harness, port))
}

/// The response to the next request asked of the test: `200`, its body streamed from the source as `plan` says, of
/// `length`.
fn stream_next(harness: &mut Harness<Headless>, plan: Plan, length: ResponseLength) -> Result<(), Unheard> {
    let source = harness.root::<Source>();
    harness.tell(source, &plan)?;
    let request = harness.asked::<Request>()?;
    let body = StreamedBody { source: source.protocol(), body: 1, length };
    let response = Response {
        status: Status::Ok,
        headers: vec![Header::new("content-type", "application/octet-stream")],
        body: ResponseBody::Streamed(body),
    };

    harness.reply(request, &response)
}

/// The source once it has ended `count` streams, the harness stepping the host until it has or the wait is spent.
fn source(harness: &mut Harness<Headless>, count: usize) -> Result<&Source, Unheard> {
    let source = harness.root::<Source>();
    waiting::until(BUDGET, || {
        harness.step();

        harness.state(source).is_ok_and(|source| source.ended.len() >= count)
    });

    harness.state(source)
}

/// A response as a client of any framing reads it: its head's text, its body decoded, and whether the body ended where
/// its framing said, not cut short by the connection's close.
#[derive(Debug)]
struct Framed {
    head: String,
    body: Vec<u8>,
    whole: bool,
}

/// More of `stream` into `buffer`: false once the server has closed it.
fn more(stream: &mut TcpStream, buffer: &mut Vec<u8>) -> bool {
    let mut chunk = [0; 4096];
    let Ok(count) = stream.read(&mut chunk) else {
        panic!("the server sends within ten seconds");
    };
    buffer.extend_from_slice(&chunk[..count]);

    count > 0
}

/// The next response on `stream`, `buffer` holding what was read past the last one: its body by its
/// `Content-Length`, in chunks, or until the close, and none when it answers a `HEAD`, which is `head_only`. A plain
/// decoder of its own, to check the exchange's framing against.
fn framed(stream: &mut TcpStream, buffer: &mut Vec<u8>, head_only: bool) -> Framed {
    let end = loop {
        let ended = buffer.windows(4).position(|window| window == b"\r\n\r\n");
        if let Some(end) = ended {
            break end;
        }
        assert!(more(stream, buffer), "the server closed before a whole head");
    };
    let head = String::from_utf8_lossy(&buffer[..end]).into_owned();
    buffer.drain(..end + 4);
    let length = head.lines().find_map(|line| line.strip_prefix("content-length: ")).map(str::parse::<usize>);
    let chunked = head.lines().any(|line| line == "transfer-encoding: chunked");
    if head_only {
        return Framed { head, body: Vec::new(), whole: true };
    }

    if let Some(Ok(length)) = length {
        while buffer.len() < length {
            if !more(stream, buffer) {
                let body = mem::take(buffer);
                return Framed { head, body, whole: false };
            }
        }
        let body = buffer.drain(..length).collect();
        return Framed { head, body, whole: true };
    }
    if !chunked {
        while more(stream, buffer) {}
        let body = mem::take(buffer);
        return Framed { head, body, whole: true };
    }
    let mut body = Vec::new();
    loop {
        let line_end = loop {
            let ended = buffer.windows(2).position(|window| window == b"\r\n");
            if let Some(line_end) = ended {
                break line_end;
            }
            if !more(stream, buffer) {
                return Framed { head, body, whole: false };
            }
        };
        let size = str::from_utf8(&buffer[..line_end]).ok().and_then(|size| usize::from_str_radix(size, 16).ok());
        let Some(size) = size else {
            panic!("a chunk's size line parses: {:?}", String::from_utf8_lossy(&buffer[..line_end]));
        };
        let needed = line_end + 2 + size + 2;
        while buffer.len() < needed {
            if !more(stream, buffer) {
                body.extend_from_slice(&buffer[(line_end + 2).min(buffer.len())..]);
                return Framed { head, body, whole: false };
            }
        }
        assert_eq!(&buffer[needed - 2..needed], b"\r\n", "a chunk's data ends its line");
        body.extend_from_slice(&buffer[line_end + 2..line_end + 2 + size]);
        buffer.drain(..needed);
        if size == 0 {
            return Framed { head, body, whole: true };
        }
    }
}

/// A client on a thread of its own that sends `sent` to `port` and reads a response framed any way for each of
/// `heads_only`, a `HEAD`'s when true, and whether the server then closed.
fn framed_client(port: u16, sent: &'static [u8], heads_only: &'static [bool]) -> JoinHandle<(Vec<Framed>, bool)> {
    #[allow(clippy::disallowed_methods)] // The client is a native thread of its own, outside the engine.
    thread::spawn(move || {
        let mut stream = connect(port);
        assert!(stream.write_all(sent).is_ok(), "the client writes");
        let mut buffer = Vec::new();
        let read = heads_only.iter().map(|&head_only| framed(&mut stream, &mut buffer, head_only)).collect();
        let is_read = buffer.is_empty();
        let closed = is_read && closes(&mut stream);

        (read, closed)
    })
}

/// What `client` read, the harness stepping the host until it has.
fn heard_framed(harness: &mut Harness<Headless>, client: JoinHandle<(Vec<Framed>, bool)>) -> (Vec<Framed>, bool) {
    while !client.is_finished() {
        harness.step();
    }
    let Ok(read) = client.join() else {
        panic!("the client finished");
    };

    read
}

#[test]
fn a_streamed_response_of_unknown_length_arrives_whole_in_order_and_chunked() -> Result<(), Unheard> {
    // Catches a streamed body's parts written out of order, lost, or framed wrong (a size in decimal, a chunk's line
    // ending missing, no last chunk), and credit granted as a part is taken as well as once it is sent, which lets the
    // source run past its window (checked by mutation: granting with `part` in place of `part_held` fails this test).
    let settings = HttpSettings { window: 2, ..HttpSettings::default() };
    let (mut harness, port) = sourced(settings)?;
    let plan = Plan { parts: 60, part_bytes: 100, ends: Ends::Whole };
    let client = framed_client(port, b"GET /stream HTTP/1.1\r\nConnection: close\r\n\r\n", &[false]);

    stream_next(&mut harness, plan, ResponseLength::Unknown)?;

    let (read, closed) = heard_framed(&mut harness, client);
    assert!(read[0].head.starts_with("HTTP/1.1 200 OK"), "{:?}", read[0].head);
    assert!(read[0].head.contains("transfer-encoding: chunked"), "{:?}", read[0].head);
    assert!(!read[0].head.contains("content-length"), "{:?}", read[0].head);
    assert!(read[0].whole, "the body ended with its last chunk");
    assert_eq!(read[0].body, plan.body());
    assert!(closed, "a request that asked to close is closed once its streamed response is written");
    let source = source(&mut harness, 1)?;
    assert_eq!(source.ended, [SourceEnd::Whole]);
    assert!(source.most_credit <= 2, "the source ran {} parts ahead of a window of 2", source.most_credit);

    Ok(())
}

#[test]
fn a_streamed_response_of_known_length_or_to_http_1_0_is_written_without_chunks() -> Result<(), Unheard> {
    // Catches a known length written as chunks, or its `Content-Length` missing; and chunks written to an HTTP/1.0
    // client, which cannot read them, where its body runs until the connection closes.
    let plan = Plan { parts: 30, part_bytes: 70, ends: Ends::Whole };
    let (mut harness, port) = sourced(HttpSettings::default())?;
    let client = framed_client(port, b"GET /stream HTTP/1.1\r\nConnection: close\r\n\r\n", &[false]);

    stream_next(&mut harness, plan, ResponseLength::Known(2100))?;

    let (read, closed) = heard_framed(&mut harness, client);
    assert!(read[0].head.contains("content-length: 2100"), "{:?}", read[0].head);
    assert!(!read[0].head.contains("transfer-encoding"), "{:?}", read[0].head);
    assert_eq!(read[0].body, plan.body());
    assert!(closed);

    let (mut harness, port) = sourced(HttpSettings::default())?;
    let client = framed_client(port, b"GET /stream HTTP/1.0\r\n\r\n", &[false]);

    stream_next(&mut harness, plan, ResponseLength::Unknown)?;

    let (read, _) = heard_framed(&mut harness, client);
    assert!(read[0].head.contains("connection: close"), "{:?}", read[0].head);
    assert!(!read[0].head.contains("transfer-encoding"), "{:?}", read[0].head);
    assert!(!read[0].head.contains("content-length"), "{:?}", read[0].head);
    assert_eq!(read[0].body, plan.body(), "the body runs until the close");

    Ok(())
}

#[test]
fn a_source_that_fails_midway_closes_the_connection_without_ending_the_body() -> Result<(), Unheard> {
    // Catches a failed source's body ended with its last chunk, which a client would take for the whole body, and a
    // connection kept open after a head whose status can no longer say the body broke.
    let (mut harness, port) = sourced(HttpSettings::default())?;
    let plan = Plan { parts: 10, part_bytes: 50, ends: Ends::FailsAfter(3) };
    let client = framed_client(port, b"GET /stream HTTP/1.1\r\n\r\n", &[false]);

    stream_next(&mut harness, plan, ResponseLength::Unknown)?;

    let (read, _) = heard_framed(&mut harness, client);
    assert!(read[0].head.starts_with("HTTP/1.1 200 OK"), "{:?}", read[0].head);
    assert!(!read[0].whole, "the body is cut short by the close, not ended");
    assert!(plan.body().starts_with(&read[0].body), "what came is the body's start");
    assert_eq!(source(&mut harness, 1)?.ended, [SourceEnd::Failed]);

    Ok(())
}

#[test]
fn a_head_request_gets_a_streamed_responses_head_and_its_source_is_stopped_unread() -> Result<(), Unheard> {
    // Catches a streamed body written in answer to a `HEAD`, its `Content-Length` dropped from the head, and a source
    // left open and waiting to be read for a body that is never sent.
    let (mut harness, port) = sourced(HttpSettings::default())?;
    let plan = Plan { parts: 30, part_bytes: 70, ends: Ends::Whole };
    let client = framed_client(port, b"HEAD /stream HTTP/1.1\r\nConnection: close\r\n\r\n", &[true]);

    stream_next(&mut harness, plan, ResponseLength::Known(2100))?;

    let (read, closed) = heard_framed(&mut harness, client);
    assert!(read[0].head.contains("content-length: 2100"), "{:?}", read[0].head);
    assert!(closed, "nothing follows the head before the close");
    assert_eq!(source(&mut harness, 1)?.ended, [SourceEnd::Stopped]);

    Ok(())
}

#[test]
fn a_request_pipelined_after_a_streamed_response_is_answered_after_it() -> Result<(), Unheard> {
    // Catches the request after a streamed response asked, and its response written, while the streamed body is still
    // being written, which interleaves the two on the connection; and a connection that does not take the next request
    // once a streamed response ends.
    let (mut harness, port) = sourced(HttpSettings::default())?;
    let plan = Plan { parts: 40, part_bytes: 100, ends: Ends::Whole };
    let ahead =
        b"GET /stream HTTP/1.1\r\n\r\nPOST /echo HTTP/1.1\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello";
    let client = framed_client(port, ahead, &[false, false]);

    stream_next(&mut harness, plan, ResponseLength::Unknown)?;
    let echo = harness.asked::<Request>()?;
    let answer = echoed(&echo);
    harness.reply(echo, &answer)?;

    let (read, closed) = heard_framed(&mut harness, client);
    assert!(read[0].whole, "{:?}", read[0].head);
    assert_eq!(read[0].body, plan.body());
    assert!(read[1].head.contains("x-method: Post"), "{:?}", read[1].head);
    assert_eq!(read[1].body, b"hello");
    assert!(closed);

    Ok(())
}
