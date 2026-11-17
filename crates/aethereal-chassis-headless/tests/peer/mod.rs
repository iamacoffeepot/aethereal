//! A client of a port the engine serves, on a thread of its own outside the engine, which writes raw bytes and reads
//! each response as it comes (ADR-0035): what the HTTP tests share.

use aethereal_chassis_headless::Headless;
use aethereal_substrate::harness::Harness;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// A response as the client read it: its head's text and its body.
#[derive(Debug)]
pub struct Read {
    pub head: String,
    pub body: Vec<u8>,
}

/// A connection to `port`, which gives up on a read after ten seconds.
pub fn connect(port: u16) -> TcpStream {
    let Ok(stream) = TcpStream::connect(("127.0.0.1", port)) else {
        panic!("the client connects");
    };
    assert!(stream.set_read_timeout(Some(Duration::from_secs(10))).is_ok(), "the read timeout sets");

    stream
}

/// The next response on `stream`, `buffer` holding what was read past the last one.
pub fn response(stream: &mut TcpStream, buffer: &mut Vec<u8>) -> Read {
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
pub fn closes(stream: &mut TcpStream) -> bool {
    let mut byte = [0];

    stream.read(&mut byte).is_ok_and(|count| count == 0)
}

/// A client on a thread of its own, outside the engine, which sends `sent` to `port` and reads `responses` responses,
/// and whether the server then closed.
pub fn client(port: u16, sent: impl AsRef<[u8]> + Send + 'static, responses: usize) -> JoinHandle<(Vec<Read>, bool)> {
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
pub fn heard(harness: &mut Harness<Headless>, client: JoinHandle<(Vec<Read>, bool)>) -> (Vec<Read>, bool) {
    while !client.is_finished() {
        harness.step();
    }
    let Ok(read) = client.join() else {
        panic!("the client finished");
    };

    read
}
