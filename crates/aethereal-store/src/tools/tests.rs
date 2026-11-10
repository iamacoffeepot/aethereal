//! The store's tools' tests: every sheet they offer is one a holder takes.

use super::StoreTools;
use aethereal_tools::{CommandLine, FlagsFault, Tool};

#[test]
fn every_sheet_s_flags_fit_its_input() {
    // Catches a `#[flag]` an `Offer` refuses (`Offered::BadFlags`): the tool would be missing from a running engine, and
    // nothing would answer when it is called.
    let sheets = StoreTools::sheets();
    let refused: Vec<(&str, FlagsFault)> = sheets
        .iter()
        .filter_map(|sheet| CommandLine::of(sheet).err().map(|fault| (sheet.name.as_str(), fault)))
        .collect();

    assert!(refused.is_empty(), "{refused:?}");
}
