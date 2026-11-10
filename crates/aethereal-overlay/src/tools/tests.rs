use super::*;
use aethereal_tools::{CommandLine, FlagsFault};

#[test]
fn every_sheet_s_flags_fit_its_input() {
    // Catches a `#[flag]` an `Offer` refuses (`Offered::BadFlags`): the overlay's tools would be missing from a
    // running engine, and nothing would answer when they are called.
    let sheets = OverlayTools::sheets();
    let refused: Vec<(&str, FlagsFault)> = sheets
        .iter()
        .filter_map(|sheet| CommandLine::of(sheet).err().map(|fault| (sheet.name.as_str(), fault)))
        .collect();

    assert!(refused.is_empty(), "{refused:?}");
}

/// Bytes are shown in the largest power of 1,024 they reach and against their most as a share, so a figure a
/// thousand times off, or a share of the wrong way round, is caught.
#[test]
fn memory_is_shown_in_powers_of_1024() {
    let blobs = Against { bytes: 3 << 20, most: 1 << 30 };

    assert_eq!(bytes_shown(512), "512 B");
    assert_eq!(bytes_shown(1536), "1.5 KiB");
    assert_eq!(against(blobs), "3.0 MiB of 1.0 GiB (0%)");
    assert_eq!(against(Against { bytes: 7, most: 0 }), "7 B");
}
