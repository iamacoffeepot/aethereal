use super::{Entry, Grew, Note, Since, Speaker, Transcript, Turning};
use aethereal_actor::Time;
use aethereal_data::{Budget, Json};

/// The time each entry is added at, where the test does not reason about it.
const AT: Time = Time::ZERO;

fn claude(line: &str) -> Entry {
    let Ok(json) = Json::parse(line, &mut Budget::new(1 << 16)) else {
        panic!("the test's line is JSON");
    };

    Entry::Claude(json)
}

/// The file a transcript of `entries` is kept as.
fn file(entries: Vec<Entry>) -> Vec<u8> {
    let mut transcript = Transcript::new();
    let mut bytes = Vec::new();
    for entry in entries {
        let Ok(frame) = transcript.add(entry, AT).frame() else {
            panic!("the entry encodes");
        };
        bytes.extend(frame);
    }

    bytes
}

#[test]
fn a_transcript_read_back_numbers_on_and_leaves_out_a_frame_cut_short() {
    // Catches a transcript that a crash mid-append leaves unreadable, or one read back whose numbers restart, so a
    // `session_read` after a hub restart answers entries twice or skips them; and one read back listed as grown at a
    // time the hub never saw, or one added to listed as not grown.
    let said = Entry::Said { by: Speaker::Caller, text: String::from("hello") };
    let mut bytes = file(vec![Entry::Hub(Note::Started { run: 1, resumed: false }), said.clone(), said]);
    let whole = Transcript::read(&bytes);
    bytes.truncate(bytes.len() - 3);
    let cut = Transcript::read(&bytes);

    assert_eq!(whole.last(), 3);
    assert_eq!(cut.last(), 2, "the frame cut short is left out, and the ones before kept");
    assert_eq!(cut.grew(), Grew::Unseen);
    let mut cut = cut;
    let later = Time::from_nanos(7);
    assert_eq!(
        cut.add(Entry::Interrupted { by: Speaker::Caller }, later).number,
        3,
        "numbering goes on from the last kept"
    );
    assert_eq!(cut.grew(), Grew::At(later));
    let after: Vec<u64> = whole.after(1, 10).iter().map(|numbered| numbered.number).collect();
    assert_eq!(after, [2, 3]);
    assert_eq!(whole.after(1, 1).len(), 1, "a read answers at most its most");
}

#[test]
fn the_output_a_transcript_holds_is_counted_from_the_latest_container_s_start() {
    // Catches a re-attach that skips the wrong number of bytes of the logs, so lines are recorded twice or lost: each
    // line counts its newline, and a new container's logs begin again from nothing.
    let mut transcript = Transcript::new();
    transcript.add(Entry::Hub(Note::Started { run: 1, resumed: false }), AT);
    transcript.add(claude(r#"{"type":"system","subtype":"init"}"#), AT);
    transcript.add(Entry::Stderr { text: String::from("warn"), bytes: 4 }, AT);
    assert_eq!(transcript.output_bytes(), (35, 5));

    transcript.add(Entry::Hub(Note::Started { run: 2, resumed: true }), AT);
    transcript.add(Entry::Stdout { text: String::from("plain"), bytes: 5 }, AT);
    assert_eq!(transcript.output_bytes(), (6, 0));
}

#[test]
fn a_transcript_knows_whether_a_turn_runs_and_whether_one_ran_since_the_last_compaction() {
    // Catches an idle session compacted when nothing happened since its last compaction, and a re-attached session
    // taken as idle while its turn runs, or as busy for good once its turn ended.
    let mut transcript = Transcript::new();
    assert_eq!((transcript.turning(), transcript.since()), (Turning::Ended, Since::Compacted));
    transcript.add(Entry::Said { by: Speaker::Caller, text: String::from("hi") }, AT);
    assert_eq!(transcript.turning(), Turning::Running);
    transcript.add(claude(r#"{"type":"result","subtype":"success"}"#), AT);
    assert_eq!((transcript.turning(), transcript.since()), (Turning::Ended, Since::Turned));
    transcript.add(claude(r#"{"type":"system","subtype":"compact_boundary"}"#), AT);
    assert_eq!(transcript.since(), Since::Compacted);
    transcript.add(claude(r#"{"type":"system","subtype":"init"}"#), AT);
    assert_eq!(transcript.turning(), Turning::Running, "a turn Claude starts by itself is a turn");
}
