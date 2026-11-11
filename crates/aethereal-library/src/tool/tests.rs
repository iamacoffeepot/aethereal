//! The library's tools' tests: their sheets, and a publish's record read from what the engine records of the moves.

use super::{HeadRecord, KindMoves, KindRecord, LibraryTool, ParkedGuest, WaitingGuest};
use crate::Numbered;
use aethereal_actor::{ActorName, Behind, HeadMoves, Standing, Standings};
use aethereal_data::{ActorPath, KindName, PathSegment};
use aethereal_tools::{CommandLine, FlagsFault, Tool};

const HUD: KindName = KindName::new("aether.test.hud");
const MAP: KindName = KindName::new("aether.test.map");

/// A guest of the kind `kind` behind the head the publish `publish` made, at the root keyed `key`, standing so.
fn behind(kind: KindName, key: u64, publish: u64, standing: Standing) -> Behind {
    let actor = ActorName::Path(ActorPath::from_root(&PathSegment::new(kind.clone(), &key)));

    Behind { actor, kind, publish, standing }
}

/// The name a record gives the guest of the kind `kind` at the root keyed `key`.
fn named(kind: KindName, key: u64) -> String {
    ActorPath::from_root(&PathSegment::new(kind, &key)).to_string()
}

#[test]
fn a_record_holds_only_its_own_publish_s_guests_and_settles_once_none_is_still_to_be_asked() {
    // Catches a guest behind another kind's head, or behind an earlier publish of its own kind, counted in the
    // record; a kind settled while a guest of it is still to be asked, so a publish answers before its wait has
    // anything to show; a publish not yet folded read as settled; and a head another publish made since read as this
    // one's.
    let standings = Standings {
        heads: vec![
            HeadMoves { kind: HUD, publish: 4, moved: 2, absent: vec![String::from("show_cooldowns")] },
            HeadMoves { kind: MAP, publish: 5, moved: 0, absent: Vec::new() },
        ],
        behind: vec![
            behind(HUD, 1, 4, Standing::Later { batches: 3 }),
            behind(HUD, 2, 4, Standing::Parked { reason: String::from("take_over: save missing") }),
            behind(HUD, 3, 3, Standing::Later { batches: 9 }),
            behind(MAP, 4, 5, Standing::Unasked),
        ],
    };

    let hud = KindRecord::of(&Numbered { kind: HUD, publish: 4 }, &standings);
    let map = KindRecord::of(&Numbered { kind: MAP, publish: 5 }, &standings);
    let replaced = KindRecord::of(&Numbered { kind: MAP, publish: 2 }, &standings);
    let unfolded = KindRecord::of(&Numbered { kind: MAP, publish: 6 }, &standings);

    let expected = HeadRecord {
        moved: 2,
        absent: vec![String::from("show_cooldowns")],
        waiting: vec![WaitingGuest { actor: named(HUD, 1), batches: 3 }],
        parked: vec![ParkedGuest { actor: named(HUD, 2), reason: String::from("take_over: save missing") }],
        unasked: Vec::new(),
    };
    assert_eq!(hud.as_ref().map(|record| &record.moves), Some(&KindMoves::Head(expected)));
    assert_eq!(hud.as_ref().map(KindRecord::is_settled), Some(true), "every guest has answered");
    assert_eq!(map.as_ref().map(KindRecord::is_settled), Some(false), "a guest is still to be asked");
    assert_eq!(replaced.as_ref().map(|record| &record.moves), Some(&KindMoves::Replaced { publish: 5 }));
    assert_eq!(replaced.as_ref().map(KindRecord::is_settled), Some(true));
    assert_eq!(unfolded, None, "the engine has not folded the publish yet");
}

#[test]
fn every_sheet_s_flags_fit_its_input() {
    // Catches a `#[flag]` an `Offer` refuses (`Offered::BadFlags`): the tool would be missing from a running engine,
    // and nothing would answer when it is called.
    let sheets = LibraryTool::sheets();
    let refused: Vec<(&str, FlagsFault)> = sheets
        .iter()
        .filter_map(|sheet| CommandLine::of(sheet).err().map(|fault| (sheet.name.as_str(), fault)))
        .collect();

    assert!(refused.is_empty(), "{refused:?}");
}
