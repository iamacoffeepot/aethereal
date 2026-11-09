//! Where `describe_engine` cuts a tool's description when it lists the tool by name.

use super::first_sentence;

#[test]
fn a_sentence_ends_at_a_point_that_whitespace_or_the_end_follows() {
    // Catches a cut at a decimal point, a cut that drops its point, and a sentence end at a line break missed.
    assert_eq!(first_sentence("Sets the gain to 0.5 by default. Then waits."), "Sets the gain to 0.5 by default.");
    assert_eq!(first_sentence("Captures a frame.\nThe window is named."), "Captures a frame.");
    assert_eq!(first_sentence("Captures a frame."), "Captures a frame.");
}

#[test]
fn a_text_with_no_sentence_end_is_kept_whole() {
    // Catches a description with no point, or only points inside words, cut short or emptied.
    assert_eq!(first_sentence("Says how long a text is"), "Says how long a text is");
    assert_eq!(first_sentence("Reads `config.toml` and v1.2"), "Reads `config.toml` and v1.2");
    assert_eq!(first_sentence(""), "");
}
