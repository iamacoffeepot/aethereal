use super::*;
use crate::colour::{Palette, Theme};
use crate::console::{Command, Console};
use aethereal_tools::{CommandLine, FlagsFault, Sheets};

#[test]
fn every_sheet_s_flags_fit_its_input() {
    // Catches a `#[flag]` an `Offer` refuses (`Offered::BadFlags`): the overlay's tools, or the chat's words, would be
    // missing from a running engine, and nothing would answer when they are called.
    let sheets: Vec<ToolSheet> =
        OverlayTools::sheets().into_iter().chain(chat_sheets()).chain([theme_sheet()]).collect();
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

/// `set_theme` reads the theme named as its one word, none when left out, and refuses a word that names none; and
/// what it names becomes the palette, while naming none keeps it and answers which is in use. So a theme read from
/// the wrong flag, a list that switches, or an answer that names the theme before the switch is caught.
#[test]
fn set_theme_reads_its_word_and_switches_the_palette() {
    let mut console = Console::default();
    console.hear(Sheets { revision: 1, tools: vec![theme_sheet()] });
    let mut read = |line: &str| match console.submitted(line, 80) {
        Command::Call { input, .. } => from_bytes::<ChooseTheme>(&input, &mut Budget::new(INPUT_BYTES)).ok(),
        Command::Blank | Command::Shown(_) | Command::Clear | Command::Refused(_) => None,
    };

    let stone = read("set_theme stone");
    let listed = read("set_theme");
    let unknown = read("set_theme pink");

    assert_eq!(stone, Some(ChooseTheme { theme: Some(Theme::Stone) }));
    assert_eq!(listed, Some(ChooseTheme { theme: None }));
    assert_eq!(unknown, None);

    let (switched, answered) = chosen(&Palette::default(), &ChooseTheme { theme: Some(Theme::Stone) });
    let (kept, listing) = chosen(&switched, &ChooseTheme { theme: None });

    assert_eq!(switched, Palette::new(Theme::Stone));
    assert_eq!(kept, switched);
    assert_eq!(answered.current, "stone");
    assert_eq!(listing.current, "stone");
    assert_eq!(listing.themes, ["lichen", "slate", "stone"]);
}
