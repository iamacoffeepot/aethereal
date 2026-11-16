use super::{EngineList, EngineListing, ExitedEngine, Origin};
use crate::Trust;
use aethereal_process::ExitStatus;
use aethereal_store::{Binary, StoreHash};
use aethereal_tools::Caller;

/// Engine `engine`, linked with `trust`.
fn linked(engine: u32, trust: Trust) -> EngineListing {
    EngineListing { engine, name: String::from("engine"), tools: 0, origin: Origin::External, trust }
}

/// An engine spawned with `trust` that ended, its stderr saying `said`.
fn ended(trust: Trust, said: &str) -> ExitedEngine {
    let binary = Binary::Hash(StoreHash([7; 32]));

    ExitedEngine { binary, status: ExitStatus::Code(1), stderr: String::from(said), trust }
}

#[test]
fn a_session_is_listed_only_the_engines_it_started_linked_and_ended_and_the_owner_every_one() {
    // Catches the engines that ended left unfiltered, which would show a session the stderr of the owner's spawns and
    // of another session's; a session shown another session's engines, as if being untrusted were enough; and the
    // owner's list cut (ADR-0057).
    let (owner, three, four) = (Trust::Trusted, Trust::Untrusted { session: 3 }, Trust::Untrusted { session: 4 });
    let list = EngineList {
        engines: vec![linked(0, owner), linked(1, three), linked(2, four)],
        exited: vec![ended(owner, "the owner's"), ended(three, "three's"), ended(four, "four's")],
    };

    let shown = list.clone().reachable_by(Caller::Session { session: 3 });
    assert_eq!(shown, EngineList { engines: vec![linked(1, three)], exited: vec![ended(three, "three's")] });
    assert_eq!(list.clone().reachable_by(Caller::Owner), list);
}
