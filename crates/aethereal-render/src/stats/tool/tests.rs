//! `render_stats`'s sheet, and which calls what is heard answers.

use super::*;
use aethereal_tools::{CommandLine, FlagsFault};

#[test]
fn every_sheet_s_flags_fit_its_input() {
    // Catches a `#[flag]` an `Offer` refuses (`Offered::BadFlags`): the tool would be missing from a running engine,
    // and nothing would answer when it is called.
    let sheets = StatsTool::sheets();
    let refused: Vec<(&str, FlagsFault)> = sheets
        .iter()
        .filter_map(|sheet| CommandLine::of(sheet).err().map(|fault| (sheet.name.as_str(), fault)))
        .collect();

    assert!(refused.is_empty(), "{refused:?}");
}

#[test]
fn a_call_that_asked_for_pass_times_waits_for_them_and_one_that_did_not_is_answered_without() {
    // Catches a call answered before the pass times it asked for are heard, as if the device had none, and a call that
    // asked for none held until they come.
    let took = GpuTimes::Took { elapsed_nanos: 5, passes: Vec::new() };

    assert_eq!(answered(true, &TimesHeard::Awaited), None);
    assert_eq!(answered(false, &TimesHeard::Awaited), Some(PassTimes::Unasked));
    assert_eq!(answered(true, &TimesHeard::Heard(GpuTimes::Unsupported)), Some(PassTimes::Unsupported));
    assert_eq!(
        answered(true, &TimesHeard::Heard(took)),
        Some(PassTimes::Took { elapsed_nanos: 5, passes: Vec::new() })
    );
}
