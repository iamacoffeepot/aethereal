//! Where `describe_engine` cuts a tool's description when it lists the tool by name.

use super::{EngineTools, first_sentence};
use aethereal_tools::{CommandLine, FlagsFault, Tool};

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

#[test]
fn every_sheet_s_flags_fit_its_input() {
    // Catches a `#[flag]` an `Offer` refuses (`Offered::BadFlags`): the tool would be missing from a running engine,
    // and nothing would answer when it is called.
    let sheets = EngineTools::sheets();
    let refused: Vec<(&str, FlagsFault)> = sheets
        .iter()
        .filter_map(|sheet| CommandLine::of(sheet).err().map(|fault| (sheet.name.as_str(), fault)))
        .collect();

    assert!(refused.is_empty(), "{refused:?}");
}
