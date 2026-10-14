//! What a turn's list holds once the children that failed to begin are taken out (ADR-0009).

use crate::id::spawned;
use aethereal_actor::{Effect, What};
use std::collections::{BTreeMap, BTreeSet};

/// The effects of one turn that the engine applies: `effects` without what a failed `begin` left.
///
/// A child whose `begin` returned a fault was never live. Its spawn is not
/// applied, and neither is anything it asked for, nor anything asked for by an
/// actor it stood up, but for the lines they logged. A parent may spawn at that key again in the same turn,
/// and that later child is kept.
pub(super) fn begun(effects: Vec<Effect>) -> Vec<Effect> {
    // Most turns spawn nothing, and then nothing is taken out.
    let spawns = effects.iter().any(|effect| spawned(effect).is_some());
    if !spawns {
        return effects;
    }

    let failed = failed_spawns(&effects);
    let mut never_live = BTreeSet::new();
    let mut kept = Vec::with_capacity(effects.len());
    for (position, effect) in effects.into_iter().enumerate() {
        let is_by_never_live = never_live.contains(&effect.by().bits());
        let is_report = *effect.what() == What::Faulted;
        if let Some((child, _)) = spawned(&effect) {
            let fails = is_by_never_live || failed.contains(&position);
            if fails {
                never_live.insert(child.bits());
            } else {
                never_live.remove(&child.bits());
                kept.push(effect);
            }
        } else {
            let is_line = matches!(effect.what(), What::Log { .. });
            let is_lost = is_by_never_live && !is_line;
            let is_dropped = is_lost || is_report;
            if !is_dropped {
                kept.push(effect);
            }
        }
    }

    kept
}

/// The positions of the spawns whose child's `begin` failed: each is followed by a `Faulted` by that child.
fn failed_spawns(effects: &[Effect]) -> BTreeSet<usize> {
    let mut latest_spawn = BTreeMap::new();
    let mut failed = BTreeSet::new();
    for (position, effect) in effects.iter().enumerate() {
        if let Some((child, _)) = spawned(effect) {
            latest_spawn.insert(child.bits(), position);
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
    use aethereal_actor::{Code, Level};
    use aethereal_data::{ActorId, KindName, PathSegment};

    const BRANCH: KindName = KindName::new("aether.test.branch");
    const LEAF: KindName = KindName::new("aether.test.leaf");

    #[test]
    fn failed_begin_takes_out_its_child_and_all_beneath() {
        // Catches a failed child's spawn or asks reaching the table, its logged lines lost with them, a second
        // child at the same key dropped with the first, and an unrelated actor's effects lost.
        let me = ActorId::new(1);
        let branch = child_id(me, &PathSegment::new(BRANCH, &()));
        let leaf = child_id(branch, &PathSegment::new(LEAF, &()));
        let spawn_branch = Effect::new(me, What::Spawn { from: Code::Linked(BRANCH) });
        let spawn_leaf = Effect::new(branch, What::Spawn { from: Code::Linked(LEAF) });
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

        assert_eq!(begun(effects), [line, Effect::new(me, What::Close), spawn_branch, spawn_leaf]);
    }
}
