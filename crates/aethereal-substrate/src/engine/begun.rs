//! What a turn's list holds once the children that failed to begin are taken out (ADR-0009).

use crate::id::spawned;
use aethereal_actor::{Effect, What};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

/// The effects of one turn that the engine applies: `effects` without what a failed `begin` left.
///
/// A child whose `begin` returned a fault was never live. Its spawn is not
/// applied, and neither is anything it asked for, nor anything asked for by an
/// actor it stood up, but for the lines they logged. A parent may spawn at that key again in the same turn,
/// and that later child is kept.
pub(super) fn begun(effects: &[Effect]) -> Cow<'_, [Effect]> {
    // A child whose `begin` failed is reported by a `Faulted` of its own, and nothing is taken out without one. Most
    // turns have none.
    let faults = effects.iter().any(|effect| matches!(effect.what(), What::Faulted));
    if !faults {
        return Cow::Borrowed(effects);
    }

    let failed = failed_spawns(effects);
    let mut never_live = BTreeSet::new();
    let mut kept = Vec::with_capacity(effects.len());
    for (position, effect) in effects.iter().enumerate() {
        let is_by_never_live = never_live.contains(&effect.by().bits());
        let is_report = *effect.what() == What::Faulted;
        if let Some((child, _)) = spawned(effect) {
            let fails = is_by_never_live || failed.contains(&position);
            if fails {
                never_live.insert(child.id.bits());
            } else {
                never_live.remove(&child.id.bits());
                kept.push(effect.clone());
            }
        } else {
            let is_line = matches!(effect.what(), What::Log { .. });
            let is_lost = is_by_never_live && !is_line;
            let is_dropped = is_lost || is_report;
            if !is_dropped {
                kept.push(effect.clone());
            }
        }
    }

    Cow::Owned(kept)
}

/// The positions of the spawns whose child's `begin` failed: each is followed by a `Faulted` by that child.
fn failed_spawns(effects: &[Effect]) -> BTreeSet<usize> {
    let mut latest_spawn = BTreeMap::new();
    let mut failed = BTreeSet::new();
    for (position, effect) in effects.iter().enumerate() {
        if let Some((child, _)) = spawned(effect) {
            latest_spawn.insert(child.id.bits(), position);
        } else if *effect.what() == What::Faulted {
            failed.extend(latest_spawn.remove(&effect.by().bits()));
        }
    }

    failed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::child_id;
    use aethereal_actor::{Code, Declared, Level, SeatNumber, Seated};
    use aethereal_data::{ActorId, Kind, KindId, KindName, PathSegment, Plain};

    /// The types of the children these tests stand up.
    #[derive(Plain)]
    struct Branch;

    impl Kind for Branch {
        const NAME: KindName = KindName::new("aether.test.branch");
    }

    #[derive(Plain)]
    struct Leaf;

    impl Kind for Leaf {
        const NAME: KindName = KindName::new("aether.test.leaf");
    }

    const BRANCH: KindName = Branch::NAME;
    const LEAF: KindName = Leaf::NAME;
    /// What their types declare: nothing they send or handle, which this does not read.
    const BRANCH_TYPE: &Declared = &Declared::new(BRANCH, KindId::of::<Branch>(), &[], &[], &[]);
    const LEAF_TYPE: &Declared = &Declared::new(LEAF, KindId::of::<Leaf>(), &[], &[], &[]);

    #[test]
    fn failed_begin_takes_out_its_child_and_all_beneath() {
        // Catches a failed child's spawn or asks reaching the table, its logged lines lost with them, a second
        // child at the same key dropped with the first, and an unrelated actor's effects lost.
        let seated = |id, seat| Seated { id, seat: SeatNumber::new(seat) };
        let me = seated(ActorId::new(1), 0);
        let branch = seated(child_id(me.id, &PathSegment::new(BRANCH, &())), 1);
        let leaf = seated(child_id(branch.id, &PathSegment::new(LEAF, &())), 2);
        let spawn_branch = Effect::new(me, What::Spawn { from: Code::Linked(BRANCH_TYPE), seat: branch.seat });
        let spawn_leaf = Effect::new(branch, What::Spawn { from: Code::Linked(LEAF_TYPE), seat: leaf.seat });
        let line = Effect::new(leaf, What::Log { level: Level::Error, text: String::from("no file") });
        let effects = vec![
            spawn_branch.clone(),
            spawn_leaf.clone(),
            line.clone(),
            Effect::new(leaf, What::Close),
            Effect::new(branch, What::Faulted),
            Effect::new(me, What::Close),
            spawn_branch.clone(),
            spawn_leaf.clone(),
        ];

        assert_eq!(*begun(&effects), [line, Effect::new(me, What::Close), spawn_branch, spawn_leaf]);
    }
}
