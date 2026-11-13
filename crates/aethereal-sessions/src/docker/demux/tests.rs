use super::{Demux, Piece};
use crate::{DockerError, Std};

/// A frame on `stream`, Docker's number for it, carrying `payload`.
fn frame(stream: u8, payload: &[u8]) -> Vec<u8> {
    let size = u32::try_from(payload.len()).unwrap_or(u32::MAX).to_be_bytes();

    [&[stream, 0, 0, 0][..], &size, payload].concat()
}

/// Output as a reader of the stream joins it: each run of one stream, in order.
type Runs = Vec<(Std, Vec<u8>)>;

/// `pieces` with each run of one stream joined, as a reader of the stream joins them.
fn joined(pieces: Vec<Piece>) -> Runs {
    let mut runs: Runs = Vec::new();
    for Piece { stream, bytes } in pieces {
        match runs.last_mut() {
            Some((last, run)) if *last == stream => run.extend_from_slice(&bytes),
            _ => runs.push((stream, bytes)),
        }
    }

    runs
}

/// What `demux` reads from `reads`, one after another, and how the last read ended.
fn read(reads: &[&[u8]]) -> (Runs, Result<(), DockerError>) {
    let mut demux = Demux::new();
    let mut pieces = Vec::new();
    for bytes in reads {
        let pushed = demux.push(bytes, &mut pieces);
        if pushed.is_err() {
            return (joined(pieces), pushed);
        }
    }

    (joined(pieces), Ok(()))
}

#[test]
fn interleaved_frames_come_apart_however_the_reads_are_cut() {
    // Catches a header or a payload cut between reads lost, read twice, or read as the other; a zero-length frame read
    // as one that swallows the next header; and stdout and stderr crossed, or their order lost.
    let stream = [
        frame(1, b"hello "),
        frame(2, b"oops"),
        frame(1, b""),
        frame(1, b"world"),
        frame(0, b"!"),
        frame(2, b"\x01\0\0\0\0\0\0\x05"),
        frame(1, b""),
    ]
    .concat();
    let expected = vec![
        (Std::Stdout, b"hello ".to_vec()),
        (Std::Stderr, b"oops".to_vec()),
        (Std::Stdout, b"world!".to_vec()),
        (Std::Stderr, b"\x01\0\0\0\0\0\0\x05".to_vec()),
    ];

    for cut in 0..=stream.len() {
        assert_eq!(read(&[&stream[..cut], &stream[cut..]]), (expected.clone(), Ok(())), "cut at {cut}");
    }
    let bytes: Vec<&[u8]> = stream.chunks(1).collect();
    assert_eq!(read(&bytes), (expected, Ok(())), "a byte at a time");
}

#[test]
fn a_payload_longer_than_its_read_is_handed_on_as_it_comes_and_ends_where_its_header_said() {
    // Catches a payload held until it is whole, so a frame announcing more than has come delivers nothing; and one
    // read past its announced end, taking the next header as output.
    let stream = [frame(1, b"0123456789"), frame(2, b"after")].concat();
    let mut demux = Demux::new();
    let mut pieces = Vec::new();

    assert_eq!(demux.push(&stream[..12], &mut pieces), Ok(()));
    assert_eq!(pieces, [Piece { stream: Std::Stdout, bytes: b"0123".to_vec() }], "what came is handed on");
    pieces.clear();

    assert_eq!(demux.push(&stream[12..], &mut pieces), Ok(()));
    assert_eq!(
        pieces,
        [
            Piece { stream: Std::Stdout, bytes: b"456789".to_vec() },
            Piece { stream: Std::Stderr, bytes: b"after".to_vec() }
        ]
    );
}

#[test]
fn an_error_docker_writes_into_the_stream_ends_it_after_the_output_before_it() {
    // Catches Docker's error stream read as output, its message lost, or the output before it dropped; and a header
    // naming a stream Docker has not read as stdout.
    let stream = [frame(1, b"partial"), frame(3, b"error from daemon\n"), frame(1, b"never")].concat();
    let (read_out, ended) = read(&[&stream]);
    assert_eq!(read_out, [(Std::Stdout, b"partial".to_vec())]);
    assert_eq!(ended, Err(DockerError::Stream { message: String::from("error from daemon") }));

    let (_, unknown) = read(&[&frame(7, b"what")]);
    assert_eq!(unknown, Err(DockerError::Malformed));
}
