use super::{Conversation, Record, Standing};
use aethereal_data::{Budget, from_bytes};

/// The bytes of a record named `a`, its conversation unbegun, of two runs, standing as the selector `standing` and the
/// bytes after it say: as a hub writes it.
fn saved(standing: &[u8]) -> Vec<u8> {
    let head: [u8; 13] = [1, 0, 0, 0, b'a', 0, 0, 0, 0, 2, 0, 0, 0];

    [&head[..], standing].concat()
}

#[test]
fn a_record_an_earlier_hub_saved_reads_as_where_its_session_stood() {
    // Tripwire: where a record says its session stood, read by the place of its case. A case moved, or one put before
    // another, reads every record an earlier hub saved as another case or as none, so its session is left out, woken as
    // if it had no container, or its container never found; the lines a failed start left unread, read as other lines
    // or none, so its next container is written the wrong lines, or loses them; and why that start failed read as
    // other words, or the lines and why read in each other's place; and a bootstrap container read as Claude Code's,
    // so a hub that starts again takes one that is gone for lost work, or as no container, so it is never judged.
    let read = |standing: &[u8]| from_bytes::<Record>(&saved(standing), &mut Budget::new(1 << 10));
    let record = |kept| Record { name: String::from("a"), conversation: Conversation::Unbegun, runs: 2, kept };

    assert_eq!(read(&[1, 0, 0, 0]), Ok(record(Standing::Hibernating)));
    assert_eq!(read(&[2, 0, 0, 0]), Ok(record(Standing::Archived)));
    assert_eq!(read(&[3, 0, 0, 0, 2, 0, 0, 0]), Ok(record(Standing::Starting { run: 2 })));
    assert_eq!(read(&[4, 0, 0, 0]), Ok(record(Standing::Lost)));
    let lines = [&[5, 0, 0, 0, 2, 0, 0, 0][..], &[2, 0, 0, 0, 0, 0, 0, 0], &[9, 0, 0, 0, 0, 0, 0, 0]].concat();
    let unread = [&lines[..], &[2, 0, 0, 0, b'n', b'o']].concat();
    let why = String::from("no");
    assert_eq!(read(&unread), Ok(record(Standing::Unstarted { unread: vec![2, 9], why })));
    let awake = read(&[0, 0, 0, 0, 1, 0, 0, 0, b'c', 2, 0, 0, 0]);
    assert!(matches!(awake, Ok(Record { kept: Standing::Awake { run: 2, .. }, .. })), "{awake:?}");
    let bootstrapping = read(&[6, 0, 0, 0, 1, 0, 0, 0, b'c', 2, 0, 0, 0]);
    assert!(
        matches!(bootstrapping, Ok(Record { kept: Standing::Bootstrapping { run: 2, .. }, .. })),
        "{bootstrapping:?}"
    );
}
