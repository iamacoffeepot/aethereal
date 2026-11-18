//! Bytes a stack writes of its own, put as a blob from a buffer of its turn's pool (ADR-0042).

use aethereal_actor::{Bytes, Guard, Refused, Turn};

/// `bytes`, copied into a buffer the turn's pool hands out, and put.
pub fn put<Me, F: Guard<Me>>(turn: &mut Turn<'_, Me, F>, bytes: &[u8]) -> Result<Bytes, Refused> {
    let (turn, pool) = turn.split();
    let mut buffer = pool.buffer(bytes.len());
    buffer.extend_from_slice(bytes);

    turn.put(buffer)
}
