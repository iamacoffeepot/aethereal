use super::Reading;
use crate::Std;
use crate::sessions::transcript::Entry;

#[test]
fn output_skips_what_the_transcript_holds_and_cuts_lines_across_pieces() {
    // Catches a re-attach that records again what the transcript holds, a line cut between two pieces lost or split,
    // and stderr counted against stdout's skip.
    let mut reading = Reading::skipping((5, 2));
    assert!(reading.read(Std::Stdout, b"seen\n{\"ty").is_empty(), "the skipped bytes and half a line make nothing");
    let entries = reading.read(Std::Stdout, b"pe\":1}\nplain\n");
    let texts: Vec<String> = entries
        .iter()
        .map(|entry| match entry {
            Entry::Claude(json) => format!("claude {}", json.as_str()),
            Entry::Stdout { text, bytes } => format!("stdout {text} {bytes}"),
            other => panic!("only stdout's entries: {other:?}"),
        })
        .collect();
    assert_eq!(texts, ["claude {\"type\":1}", "stdout plain 5"]);

    let entries = reading.read(Std::Stderr, b"x\nboom\n");
    assert_eq!(entries, [Entry::Stderr { text: String::from("boom"), bytes: 4 }]);
}
