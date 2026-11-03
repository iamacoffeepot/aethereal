//! A connection's gathered write, against a socket that takes a few bytes at a time.

use super::{Gathered, write_gathered};
use std::io::{self, ErrorKind, IoSlice};

/// A socket that takes at most `most` bytes a call, and would wait on every other call.
struct Trickle {
    taken: Vec<u8>,
    most: usize,
    waits: bool,
}

impl io::Write for Trickle {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.write_vectored(&[IoSlice::new(bytes)])
    }

    fn write_vectored(&mut self, slices: &[IoSlice<'_>]) -> io::Result<usize> {
        self.waits = !self.waits;
        if !self.waits {
            return Err(ErrorKind::WouldBlock.into());
        }
        let before = self.taken.len();
        for slice in slices {
            let room = self.most - (self.taken.len() - before);
            self.taken.extend_from_slice(&slice[..slice.len().min(room)]);
        }

        Ok(self.taken.len() - before)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_write_taken_in_part_goes_on_where_it_stopped_across_its_parts() {
    // Catches a write resumed from its first part, or from the start of the part it stopped in, once the socket is
    // writable again, a part left out after an empty one, and a socket that would wait written to again at once.
    let mut socket = Trickle { taken: Vec::new(), most: 3, waits: false };
    let pieces: [&[u8]; 4] = [b"he", b"", b"llo, ", b"world"];
    let mut written = 0;

    let mut waits = 0;
    for _ in 0..8 {
        let gathered = write_gathered(&mut socket, pieces, &mut written);
        if gathered != Ok(Gathered::Blocked) {
            break;
        }
        waits += 1;
    }

    assert_eq!(socket.taken, b"hello, world");
    assert_eq!(written, 12);
    assert_eq!(waits, 3);
}
