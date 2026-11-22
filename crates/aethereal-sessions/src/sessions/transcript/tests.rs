use super::{Compaction, Counted, Ended, Entry, Grew, Note, Numbered, Since, Speaker, Transcript, Turning};
use aethereal_actor::Time;
use aethereal_data::{Budget, Json, from_bytes};

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
    // `read_session` after a hub restart answers entries twice or skips them; and one read back listed as grown at a
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
    // line counts its newline, and a new container's logs begin again from nothing, a run's Claude Code container's
    // after its bootstrap container's, so the start of Claude Code's output is never skipped for what the bootstrap
    // wrote; and a container counted as another's, so a hub that starts again notes it started twice, or never.
    let mut transcript = Transcript::new();
    transcript.add(Entry::Hub(Note::Started { run: 1, resumed: false }), AT);
    transcript.add(claude(r#"{"type":"system","subtype":"init"}"#), AT);
    transcript.add(Entry::Stderr { text: String::from("warn"), bytes: 4 }, AT);
    assert_eq!(transcript.output_bytes(), (35, 5));

    transcript.add(Entry::Hub(Note::Bootstrapping { run: 2 }), AT);
    transcript.add(Entry::Stderr { text: String::from("removed"), bytes: 7 }, AT);
    assert_eq!((transcript.counted(), transcript.output_bytes()), (Counted::Bootstrap(2), (0, 8)));

    transcript.add(Entry::Hub(Note::Started { run: 2, resumed: true }), AT);
    transcript.add(Entry::Stdout { text: String::from("plain"), bytes: 5 }, AT);
    assert_eq!((transcript.counted(), transcript.output_bytes()), (Counted::Run(2), (6, 0)));
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

#[test]
fn a_compaction_runs_from_claude_code_s_status_of_compacting_until_it_ends_whoever_asked_for_it() {
    // Catches a compaction the owner's `/compact` or Claude Code itself began listed as working, as the hub listed only
    // the compactions its wind-down asked for; and one listed as compacting for good once it ended by its boundary,
    // by another status, as a compaction that fails may, by its turn's end, or by its container's.
    let compacting = claude(r#"{"type":"system","subtype":"status","status":"compacting"}"#);
    let ends = [
        claude(r#"{"type":"system","subtype":"compact_boundary","compact_metadata":{"trigger":"auto"}}"#),
        claude(r#"{"type":"system","subtype":"status","status":null}"#),
        claude(r#"{"type":"result","subtype":"success"}"#),
        Entry::Hub(Note::Started { run: 2, resumed: true }),
    ];
    for end in ends {
        let mut transcript = Transcript::new();
        assert_eq!(transcript.compaction(), Compaction::Idle);
        transcript.add(compacting.clone(), AT);
        assert_eq!(transcript.compaction(), Compaction::Running);
        transcript.add(end.clone(), AT);
        assert_eq!(transcript.compaction(), Compaction::Idle, "{end:?}");
    }
}

#[test]
fn a_container_s_stop_an_earlier_hub_saved_reads_as_how_it_stopped() {
    // Tripwire: how a container stopped, as a transcript's file keeps it, read by the place of its case. `Ended`'s
    // first case, once named for an idle stop and now for any stop to hibernate, keeps its place, so a transcript an
    // earlier hub saved reads its idle stops as hibernations, and its archives as archives.
    let read = |why: u8| {
        from_bytes::<Numbered>(
            &[3, 0, 0, 0, 0, 0, 0, 0, 5, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, why, 0, 0, 0],
            &mut Budget::new(1 << 10),
        )
    };
    let stopped = |why| Numbered { number: 3, entry: Entry::Hub(Note::Stopped { run: 1, why }) };

    assert_eq!(read(0), Ok(stopped(Ended::Hibernated)));
    assert_eq!(read(1), Ok(stopped(Ended::Archived)));
}

/// A line of the container's stderr.
fn stderr(text: &str) -> Entry {
    Entry::Stderr { text: String::from(text), bytes: u64::try_from(text.len()).unwrap_or(u64::MAX) }
}

#[test]
fn what_a_bootstrap_container_said_last_is_its_own_last_lines_in_the_order_written() {
    // Catches a failed start's refusal showing what an earlier container wrote, so the owner reads another run's
    // reason; its lines shown last first; and more of them than asked, or fewer when it wrote fewer.
    let mut transcript = Transcript::new();
    transcript.add(Entry::Hub(Note::Started { run: 1, resumed: false }), AT);
    transcript.add(stderr("Claude Code's own warning"), AT);
    transcript.add(Entry::Hub(Note::Bootstrapping { run: 2 }), AT);
    assert_eq!(transcript.stderr_tail(3), Vec::<String>::new(), "the run before's lines are not the bootstrap's");

    transcript.add(stderr("aethereal-session-start: removed /session/home/.gitconfig.lock, a lock no git holds"), AT);
    transcript.add(Entry::Said { by: Speaker::Caller, text: String::from("early") }, AT);
    transcript.add(stderr("fatal: unable to access the repository"), AT);
    transcript.add(stderr("aethereal-session-start: the fetch of origin failed with exit code 128"), AT);
    let said = [
        "fatal: unable to access the repository",
        "aethereal-session-start: the fetch of origin failed with exit code 128",
    ];
    assert_eq!(transcript.stderr_tail(2), said);
    assert_eq!(transcript.stderr_tail(9).len(), 3, "every line it wrote, when it wrote fewer");
}
