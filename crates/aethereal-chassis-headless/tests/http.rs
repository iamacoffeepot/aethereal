//! The HTTP capability served on the native host, the test its handler, to clients on threads of their own (ADR-0035,
//! ADR-0036).

use aethereal_actor::{Answered, Tick, Unanswered};
use aethereal_chassis_headless::Headless;
use aethereal_http::{Handling, Header, Http, HttpSettings, Request, Response, Serve, Served, Status};
use aethereal_substrate::Host;
use aethereal_substrate::harness::{Harness, Held, Unheard};
use aethereal_tcp::Tcp;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// A response as the client read it: its head's text and its body.
#[derive(Debug)]
struct Read {
    head: String,
    body: Vec<u8>,
}

/// A host running `settings`, the test the handler of the port it serves.
fn served(settings: HttpSettings) -> Result<(Harness<Headless>, u16), Unheard> {
    let mut harness =
        Harness::of(Headless::program().with_cycle::<(Tick,)>().root::<Tcp>().root_with::<Http>(settings))?;
    let http = harness.root::<Http>();
    let handler = harness.protocol::<Handling>();
    let serving = harness.ask(http, &Serve { port: 0, handler })?;
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
fn client(port: u16, sent: &'static [u8], responses: usize) -> JoinHandle<(Vec<Read>, bool)> {
    #[allow(clippy::disallowed_methods)] // The client is a native thread of its own, outside the engine.
    thread::spawn(move || {
        let mut stream = connect(port);
        assert!(stream.write_all(sent).is_ok(), "the client writes");
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
    assert_eq!(first.request.body, b"hello");
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
fn a_refused_request_reaches_no_handler_and_closes_once_its_response_is_sent() -> Result<(), Unheard> {
    // Catches a request that must be refused handed on to the handler, and a refused request closed before its response
    // is sent or never closed.
    let (mut harness, port) = served(HttpSettings::default())?;
    let client = client(port, b"POST /echo HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n", 1);

    let (refused, closed) = heard(&mut harness, client);

    assert!(refused[0].head.starts_with("HTTP/1.1 501"), "{refused:?}");
    assert!(closed, "a refused request's connection closes once its response is sent");
    assert_eq!(harness.rest(), Vec::new());

    Ok(())
}

#[test]
fn a_handler_that_never_responds_is_timed_out() -> Result<(), Unheard> {
    // Catches a handler that never responds left waiting without a `504`.
    let settings = HttpSettings { answer_within_millis: 200, ..HttpSettings::default() };
    let (mut harness, port) = served(settings)?;
    let client = client(port, b"GET /silent HTTP/1.1\r\n\r\n", 1);

    let _silent = harness.asked::<Request>()?;

    let (timed_out, _) = heard(&mut harness, client);
    assert!(timed_out[0].head.starts_with("HTTP/1.1 504"), "{timed_out:?}");

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
