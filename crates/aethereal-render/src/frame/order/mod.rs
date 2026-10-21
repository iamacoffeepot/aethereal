//! The order frames that end on textures run in at each `Render`: each after the frames whose textures it reads, so a
//! frame read in a `Render` is read as drawn in it (ADR-0025).

#[cfg(test)]
mod tests;

/// The frames `frames`, in an order that runs each after every frame it reads, where `reads` holds each pair of a
/// frame and a frame whose texture it reads. Frames that do not read each other run in the order of their ids. The
/// render root never lets frames read each other round, as a placement that would is refused; if `reads` went round,
/// those frames would run in the order of their ids, each reading what the other last drew.
pub fn ordered(frames: &[u64], reads: &[(u64, u64)]) -> Vec<u64> {
    let mut waiting: Vec<u64> = frames.to_vec();
    waiting.sort_unstable();
    let mut ran: Vec<u64> = Vec::with_capacity(waiting.len());
    while !waiting.is_empty() {
        let next = waiting.iter().position(|frame| ready(*frame, &waiting, reads)).unwrap_or(0);
        ran.push(waiting.remove(next));
    }

    ran
}

/// Whether `frame` reads none of the frames still `waiting`.
fn ready(frame: u64, waiting: &[u64], reads: &[(u64, u64)]) -> bool {
    !reads.iter().any(|(reader, read)| *reader == frame && waiting.contains(read))
}

/// Whether `from` reads `to`, itself or through frames it reads, where `reads` holds each pair of a frame and a frame
/// whose texture it reads: a frame reads itself.
pub fn reaches(reads: &[(u64, u64)], from: u64, to: u64) -> bool {
    let mut seen = vec![from];
    let mut next = vec![from];
    while let Some(frame) = next.pop() {
        if frame == to {
            return true;
        }
        for (_, read) in reads.iter().filter(|(reader, _)| *reader == frame) {
            let new = !seen.contains(read);
            if new {
                seen.push(*read);
                next.push(*read);
            }
        }
    }

    false
}
