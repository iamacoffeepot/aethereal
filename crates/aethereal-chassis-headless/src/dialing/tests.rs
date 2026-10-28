//! The native host's dialer (ADR-0033).

use super::*;
use crate::waiting;
use std::io::{ErrorKind, Read, Write};
use std::net::TcpListener;

/// A dialer on `threads` threads, giving each address a second.
fn dialers(threads: usize) -> Dialers {
    let Some(threads) = NonZeroUsize::new(threads) else {
        panic!("the test dials on some threads");
    };

    Dialers::new(threads, Duration::from_secs(1))
}

/// A listener on a loopback port the host picks, and its port.
fn listening() -> (TcpListener, u16) {
    let Ok(listener) = TcpListener::bind(("127.0.0.1", 0)) else {
        panic!("the test binds a loopback port");
    };
    let Ok(address) = listener.local_addr() else {
        panic!("a bound listener has an address");
    };

    (listener, address.port())
}

#[test]
fn a_dial_connects_to_a_listener_and_hands_back_a_stream_that_does_not_block() {
    // Catches a dial that never wakes the job that polls it, a stream handed back still blocking, so the first read
    // of an actor that polls it stalls its turn, and many dials at once on few threads lost or handed to the wrong
    // dial: each listener answers with its own byte.
    let dialer = dialers(2);
    let listeners: Vec<(TcpListener, u16)> = (0..4).map(|_| listening()).collect();
    let dials: Vec<Dialing<'_>> = listeners.iter().map(|(_, port)| dialer.dial("127.0.0.1", *port)).collect();
    let mut streams = Vec::new();
    for dialing in dials {
        let Ok(stream) = pollster::block_on(dialing) else {
            panic!("the dial connects");
        };
        streams.push(stream);
    }
    let mut byte = [0];
    let mut unsent = streams.iter_mut().map(|stream| stream.read(&mut byte).map_err(|error| error.kind()));
    assert!(unsent.all(|read| read == Err(ErrorKind::WouldBlock)), "a read with nothing sent returns at once");
    for (number, (listener, _)) in listeners.iter().enumerate() {
        let Ok((mut accepted, _)) = listener.accept() else {
            panic!("the listener accepts the dial");
        };
        let byte = u8::try_from(number).unwrap_or(u8::MAX);
        assert!(accepted.write_all(&[byte]).is_ok(), "the listener writes");
    }

    let mut read = Vec::new();
    for mut stream in streams {
        let mut byte = [0];
        waiting::until(waiting::BUDGET, || stream.read(&mut byte).is_ok_and(|count| count == 1));
        read.push(byte[0]);
    }
    assert_eq!(read, [0, 1, 2, 3]);
}

#[test]
fn a_dial_to_a_port_nothing_listens_on_is_refused() {
    // Catches a refused connect answered as a name that found no address, so a caller cannot tell a host that is down
    // from a host that does not exist.
    let dialer = dialers(1);
    let (listener, port) = listening();
    drop(listener);

    assert_eq!(pollster::block_on(dialer.dial("127.0.0.1", port)).err(), Some(Undialed::Refused));
}
