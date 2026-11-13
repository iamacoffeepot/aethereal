//! Docker's events (ADR-0057): `GET /events` writes one JSON object a line, as each thing happens, and the lines are
//! read across the reads they arrive in as typed events.

#[cfg(test)]
mod tests;

use super::json::{field, text, tree};
use super::{ContainerId, DockerError, Event};
use aethereal_data::Plain;
use core::{mem, str};

/// The most bytes of one event's line: far past what Docker writes for one, so a longer one is not Docker's.
const MOST_LINE_BYTES: usize = 64 * 1024;

/// The bytes of a line of events not yet whole, across the reads they arrive in.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub(super) struct Lines {
    partial: Vec<u8>,
}

impl Lines {
    /// At the start of the stream.
    pub(super) const fn new() -> Self {
        Self { partial: Vec::new() }
    }

    /// Reads `bytes`, the next of the stream, adding to `events` each event of a line they end.
    ///
    /// # Errors
    ///
    /// [`DockerError::Malformed`] for a line that is not an event, or one past its most bytes: the stream can be read
    /// no further. The events before it are added.
    pub(super) fn push(&mut self, bytes: &[u8], events: &mut Vec<Event>) -> Result<(), DockerError> {
        for line in bytes.split_inclusive(|&byte| byte == b'\n') {
            self.partial.extend_from_slice(line);
            if self.partial.len() > MOST_LINE_BYTES {
                return Err(DockerError::Malformed);
            }
            let is_whole = self.partial.ends_with(b"\n");
            if !is_whole {
                continue;
            }
            let whole = mem::take(&mut self.partial);
            let written = str::from_utf8(&whole).map_err(|_| DockerError::Malformed)?.trim();
            if written.is_empty() {
                continue;
            }
            if let Read::Event(event) = event(written)? {
                events.push(event);
            }
        }

        Ok(())
    }
}

/// What a line of events said.
enum Read {
    Event(Event),
    /// Something the client does not follow, which a filtered stream does not send.
    Unfollowed,
}

/// What a line of events says.
fn event(line: &str) -> Result<Read, DockerError> {
    let read = tree(line).ok_or(DockerError::Malformed)?;
    let root = read.root();
    let kind = field(&read, root, "Type").and_then(text).ok_or(DockerError::Malformed)?;
    let action = field(&read, root, "Action").and_then(text).ok_or(DockerError::Malformed)?;
    let actor = field(&read, root, "Actor").ok_or(DockerError::Malformed)?;
    let id = field(&read, actor, "ID").and_then(text).ok_or(DockerError::Malformed)?;
    if kind != "container" {
        return Ok(Read::Unfollowed);
    }
    let container = ContainerId::read(id).ok_or(DockerError::Malformed)?;
    let happened = match action {
        "start" => Event::Started { container },
        "die" => {
            let attributes = field(&read, actor, "Attributes").ok_or(DockerError::Malformed)?;
            let code = field(&read, attributes, "exitCode").and_then(text).and_then(|code| code.parse().ok());
            let code = code.ok_or(DockerError::Malformed)?;
            Event::Died { container, code }
        }
        "oom" => Event::OutOfMemory { container },
        _ => return Ok(Read::Unfollowed),
    };

    Ok(Read::Event(happened))
}
