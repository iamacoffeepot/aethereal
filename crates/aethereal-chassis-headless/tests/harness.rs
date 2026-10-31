//! A test as an HTTP handler on the native host, to a client on a thread of its own (ADR-0035, ADR-0036).

use aethereal_actor::{Answered, Tick};
use aethereal_chassis_headless::Headless;
use aethereal_http::{Handling, Http, Request, Response, Serve, Served, Status};
use aethereal_substrate::Host;
use aethereal_substrate::harness::{Harness, Unheard};
use aethereal_tcp::Tcp;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::thread;
use std::time::Duration;

#[test]
fn the_harness_answers_a_real_client() -> Result<(), Unheard> {
    let program = Headless::program().with_cycle::<(Tick,)>().root::<Tcp>().root::<Http>();
    let mut harness = Harness::of(program)?;
    let http = harness.root::<Http>();
    let handler = harness.protocol::<Handling>();

    let serving = harness.ask(http, &Serve { port: 0, handler })?;
    let Answered::Yes(Served::Bound { port, .. }) = harness.answer(serving)? else {
        panic!("the port is bound");
    };

    #[allow(clippy::disallowed_methods)] // The client stands outside the engine, as a real one does.
    let client = thread::spawn(move || {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("the server accepts");
        stream.set_read_timeout(Some(Duration::from_secs(30))).expect("a timeout is set");
        stream
            .write_all(b"GET /hello HTTP/1.1\r\nhost: test\r\nconnection: close\r\n\r\n")
            .expect("the request is written");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("the response is read");

        response
    });

    let request = harness.asked::<Request>()?;
    assert_eq!(request.request.path, "/hello");
    harness.reply(request, &Response::new(Status::Ok, "text/plain", b"hi".to_vec()))?;

    // The response goes out over steps the harness runs until the client has read it.
    while !client.is_finished() {
        harness.step();
    }
    let response = client.join().expect("the client ran");
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.ends_with("hi"), "{response}");

    Ok(())
}
