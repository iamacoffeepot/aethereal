//! The library's tools' tests: their sheets, and what `upgrades` reads from what the engine records of the moves.

use super::{BehindGuest, GuestStanding, HeadRecord, LibraryTool, Upgrading};
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

/// The name `upgrades` gives the guest of the kind `kind` at the root keyed `key`.
fn named(kind: KindName, key: u64) -> String {
    ActorPath::from_root(&PathSegment::new(kind, &key)).to_string()
}

#[test]
fn upgrades_reads_every_head_s_moves_and_every_guest_behind_with_where_it_stands() {
    // Catches a head's count of moves, the fields its movers' old states lacked, or its publish's number lost on the
    // way to the agent; and a guest that is still to be asked, answered `Later`, or is parked dropped, read as another
    // standing, given another kind or publish, or its batches or reason lost.
    let reason = String::from("take_over: save missing");
    let standings = Standings {
        heads: vec![
            HeadMoves { kind: HUD, publish: 4, moved: 2, absent: vec![String::from("show_cooldowns")] },
            HeadMoves { kind: MAP, publish: 5, moved: 0, absent: Vec::new() },
        ],
        behind: vec![
            behind(HUD, 1, 4, Standing::Later { batches: 3 }),
            behind(HUD, 2, 3, Standing::Parked { reason: reason.clone() }),
            behind(MAP, 3, 5, Standing::Unasked),
        ],
    };

    let upgrading = Upgrading::of(&standings);

    let heads = vec![
        HeadRecord { kind: HUD, publish: 4, moved: 2, absent: vec![String::from("show_cooldowns")] },
        HeadRecord { kind: MAP, publish: 5, moved: 0, absent: Vec::new() },
    ];
    let guests = vec![
        BehindGuest { actor: named(HUD, 1), kind: HUD, publish: 4, standing: GuestStanding::Later { batches: 3 } },
        BehindGuest { actor: named(HUD, 2), kind: HUD, publish: 3, standing: GuestStanding::Parked { reason } },
        BehindGuest { actor: named(MAP, 3), kind: MAP, publish: 5, standing: GuestStanding::Unasked },
    ];
    assert_eq!(upgrading, Upgrading { heads, guests });
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
