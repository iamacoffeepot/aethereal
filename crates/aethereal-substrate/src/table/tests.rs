//! The table's tests: what a fold makes of spawns and endings, and what a reader then sees.

use super::*;
use crate::id::{child_id, root_id};
use crate::{Code, What};
use aethereal_data::{KeyName, KindName, PathSegment};

const WINDOW: KindName = KindName::new("aether.window");
const SURFACE: KindName = KindName::new("aether.window.surface");
const LAYER: KindName = KindName::new("aether.window.layer");
const LEFT: KeyName = KeyName::new("left");

/// A table of `parts` parts.
fn table(parts: usize) -> ActorTable {
    let Some(parts) = NonZeroUsize::new(parts) else {
        panic!("a table has at least one part");
    };

    ActorTable::new(parts)
}

/// The window root's id, and the effect that makes it.
fn window() -> (ActorId, Effect) {
    let id = root_id(&PathSegment::new(WINDOW, &()));

    (id, Effect::new(id, What::Root { from: Code::Linked(WINDOW) }))
}

/// The id of the surface `left` beneath `parent`, and the effect by which `parent` spawns it.
fn surface(parent: ActorId) -> (ActorId, Effect) {
    let id = child_id(parent, &PathSegment::instanced(SURFACE, LEFT));

    (id, Effect::new(parent, What::SpawnInstanced { from: Code::Linked(SURFACE), key: LEFT }))
}

/// The id of the layer beneath `parent`, and the effect by which `parent` embeds it.
fn layer(parent: ActorId) -> (ActorId, Effect) {
    let id = child_id(parent, &PathSegment::new(LAYER, &()));

    (id, Effect::new(parent, What::Embed { from: Code::Linked(LAYER) }))
}

#[test]
fn spawned_actor_resolves_by_its_path() {
    // Catches an entry kept under the parent's id, a segment built without its key, and a path walked up in the
    // wrong order.
    let (window, root) = window();
    let (surface, spawn) = surface(window);
    let (layer, embed) = layer(surface);
    let mut table = table(4);
    table.fold(&[root, spawn, embed]);

    let path = ActorPath::new("aether.window/aether.window.surface:left/aether.window.layer");
    assert_eq!(table.resolve(&path), Some(layer));
    assert_eq!(table.path(layer), Some(path));
    assert_eq!(table.parent(layer), Some(surface));
    assert_eq!(table.parent(window), None);
}

#[test]
fn ending_marks_only_its_own_actor() {
    // Catches an ending that reaches the actors beneath, and one that marks the parent of the actor that asked.
    let (window, root) = window();
    let (surface, spawn) = surface(window);
    let (layer, embed) = layer(surface);
    let mut table = table(4);
    table.fold(&[root, spawn, embed]);
    table.fold(&[Effect::new(surface, What::Close)]);

    assert_eq!(table.standing(window), Standing::Live);
    assert_eq!(table.standing(surface), Standing::Ended);
    assert_eq!(table.standing(layer), Standing::Live);
    assert_eq!(table.resolve(&ActorPath::new("aether.window/aether.window.surface:left")), None);
}

#[test]
fn failed_turn_ends_its_actor() {
    // Catches a view that follows `Close` and passes over the ending the engine writes for a failed turn.
    let (window, root) = window();
    let mut table = table(1);
    table.fold(&[root, Effect::new(window, What::Failed)]);

    assert_eq!(table.standing(window), Standing::Ended);
}

#[test]
fn ended_path_is_not_made_again() {
    // Catches a second spawn at a used key bringing an ended path back to life.
    let (window, root) = window();
    let (surface, spawn) = surface(window);
    let mut table = table(4);
    table.fold(&[root, spawn.clone(), Effect::new(surface, What::Close), spawn]);

    assert_eq!(table.standing(surface), Standing::Ended);
}

#[test]
fn reader_sees_nothing_before_the_fold() {
    // Catches an ending of an actor the table never held making an entry for it.
    let (window, _) = window();
    let mut table = table(4);
    table.fold(&[Effect::new(window, What::Close)]);

    assert_eq!(table.standing(window), Standing::Absent);
    assert_eq!(table.path(window), None);
}

#[test]
fn part_count_does_not_change_the_table() {
    // Catches a record folded into one part and read from another: every read agrees for one part and for many.
    let (window, root) = window();
    let (surface, spawn) = surface(window);
    let (layer, embed) = layer(surface);
    let effects = [root, spawn, embed, Effect::new(layer, What::Close)];
    let seen = [1, 2, 7, 64].map(|parts| {
        let mut table = table(parts);
        table.fold(&effects);

        [window, surface, layer].map(|id| (table.standing(id), table.path(id), table.parent(id)))
    });

    assert!(seen.iter().all(|read| *read == seen[0]));
    assert_eq!(seen[0][2].0, Standing::Ended);
}
