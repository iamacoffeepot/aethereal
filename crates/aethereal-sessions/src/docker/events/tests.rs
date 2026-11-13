use super::Lines;
use crate::{ContainerId, DockerError, Event};

/// An id as Docker writes one.
const ID: &str = "0a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f9";

fn container() -> ContainerId {
    ContainerId::read(ID).unwrap_or_else(|| panic!("the id is 64 hex digits"))
}

/// A line of events as Docker writes one, for a container's `action`, with `attributes`.
fn line(kind: &str, action: &str, attributes: &str) -> String {
    format!(
        r#"{{"status":"{action}","id":"{ID}","Type":"{kind}","Action":"{action}","Actor":{{"ID":"{ID}","Attributes":{{{attributes}}}}},"scope":"local","time":1763020800,"timeNano":1763020800123456789}}"#
    ) + "\n"
}

#[test]
fn events_are_read_as_their_lines_end_however_the_reads_are_cut() {
    // Catches a line cut between reads lost, read twice, or read before it is whole; a death's exit code read from the
    // wrong member; and an action the client does not follow, or an event not of a container, read as one it does.
    let stream = [
        line("container", "start", r#""aethereal.test":"1","name":"probe""#),
        line("container", "exec_start: sh", r#""name":"probe""#),
        line("network", "connect", r#""container":"x""#),
        line("container", "oom", r#""name":"probe""#),
        line("container", "die", r#""exitCode":"137","name":"probe""#),
    ]
    .concat();
    let expected = vec![
        Event::Started { container: container() },
        Event::OutOfMemory { container: container() },
        Event::Died { container: container(), code: 137 },
    ];

    for cut in 0..=stream.len() {
        let (first, second) = stream.as_bytes().split_at(cut);
        let mut lines = Lines::new();
        let mut events = Vec::new();
        assert_eq!(lines.push(first, &mut events), Ok(()), "cut at {cut}");
        assert_eq!(lines.push(second, &mut events), Ok(()), "cut at {cut}");
        assert_eq!(events, expected, "cut at {cut}");
    }
}

#[test]
fn a_line_that_is_not_an_event_ends_the_stream_after_the_events_before_it() {
    // Catches a line that is not Docker's passed over silently, so a stream that broke reads as one that is quiet.
    let mut lines = Lines::new();
    let mut events = Vec::new();
    let stream = line("container", "start", "") + "{\"Type\":\"container\"}\n";
    assert_eq!(lines.push(stream.as_bytes(), &mut events), Err(DockerError::Malformed));
    assert_eq!(events, [Event::Started { container: container() }]);
}
