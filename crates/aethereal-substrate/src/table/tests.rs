//! The table's tests: what a fold makes of spawns and endings, and what a reader then sees.

use super::*;
use crate::Context;
use crate::id::{child_id, root_id};
use aethereal_actor::{Code, Declared, Fault, MailId, SeatNumber, Seated, Spawner, Unreadable, What};
use aethereal_data::{KeyName, Keyed, Kind, KindId, KindName, PathSegment, Plain};

/// The context the effects of these tests were emitted in, which the table does not read.
fn context() -> Context {
    Context { root: MailId { sender: ActorId::new(1), count: 0 }, depth: 1 }
}

/// The types of these actors.
#[derive(Plain)]
struct Window;

impl Kind for Window {
    const NAME: KindName = KindName::new("aether.window");
}

#[derive(Plain)]
struct Surface;

impl Kind for Surface {
    const NAME: KindName = KindName::new("aether.window.surface");
}

#[derive(Plain)]
struct Layer;

impl Kind for Layer {
    const NAME: KindName = KindName::new("aether.window.layer");
}

const WINDOW: KindName = Window::NAME;
const SURFACE: KindName = Surface::NAME;
const LAYER: KindName = Layer::NAME;
/// What the types of these actors declare: nothing they send or handle, which the table does not read.
const WINDOW_TYPE: &Declared =
    &Declared::new(WINDOW, KindId::of::<Window>(), Keyed::Singleton, Spawner::Anyone, &[], &[], &[]);
const SURFACE_TYPE: &Declared =
    &Declared::new(SURFACE, KindId::of::<Surface>(), Keyed::Instanced, Spawner::Anyone, &[], &[], &[]);
const LAYER_TYPE: &Declared =
    &Declared::new(LAYER, KindId::of::<Layer>(), Keyed::Singleton, Spawner::Anyone, &[], &[], &[]);
const LEFT: KeyName = KeyName::new("left");

/// A table of `parts` parts.
fn table(parts: usize) -> ActorTable {
    let Some(parts) = NonZeroUsize::new(parts) else {
        panic!("a table has at least one part");
    };

    ActorTable::new(parts)
}

/// The actor `id` stands for, in a seat of its own: numbered by the id's low byte.
fn at(id: ActorId) -> Seated {
    Seated { id, seat: SeatNumber::new(id.bits() & 0xff) }
}

/// The window root's id, and the effect that makes it.
fn window() -> (ActorId, Effect) {
    let id = root_id(&PathSegment::new(WINDOW, &()));

    (id, Effect::new(at(id), What::Root { from: Code::Linked(WINDOW_TYPE) }))
}

/// The id of the surface `left` beneath `parent`, and the effect by which `parent` spawns it.
fn surface(parent: ActorId) -> (ActorId, Effect) {
    let id = child_id(parent, &PathSegment::instanced(SURFACE, LEFT));
    let spawn = What::SpawnInstanced { from: Code::Linked(SURFACE_TYPE), key: LEFT, seat: at(id).seat };

    (id, Effect::new(at(parent), spawn))
}

/// The id of the layer beneath `parent`, and the effect by which `parent` spawns it.
fn layer(parent: ActorId) -> (ActorId, Effect) {
    let id = child_id(parent, &PathSegment::new(LAYER, &()));

    (id, Effect::new(at(parent), What::Spawn { from: Code::Linked(LAYER_TYPE), seat: at(id).seat }))
}

#[test]
fn spawned_actor_resolves_by_its_path() {
    // Catches an entry kept under the parent's id, a segment built without its key, a path walked up in the wrong
    // order, and a child kept in its parent's seat.
    let (window, root) = window();
    let (surface, spawn) = surface(window);
    let (layer, spawn_layer) = layer(surface);
    let mut table = table(4);
    table.fold(&[root, spawn, spawn_layer], &context());

    let path = ActorPath::new("aether.window/aether.window.surface:left/aether.window.layer");
    assert_eq!(table.resolve(&path), Some(layer));
    assert_eq!(table.path(layer), Some(path));
    assert_eq!(table.parent(layer), Some(surface));
    assert_eq!(table.parent(window), None);
    assert_eq!([window, surface, layer].map(|id| table.seat(id)), [window, surface, layer].map(|id| Some(at(id).seat)));
}

#[test]
fn ending_takes_out_only_its_own_actor() {
    // Catches an ending that reaches the actors beneath, one that takes out the parent of the actor that asked, and
    // a child whose path no longer resolves once the actor above it has gone.
    let (window, root) = window();
    let (surface, spawn) = surface(window);
    let (layer, spawn_layer) = layer(surface);
    let mut table = table(4);
    table.fold(&[root, spawn, spawn_layer], &context());
    table.fold(&[Effect::new(at(surface), What::Close)], &context());

    assert_eq!([window, surface, layer].map(|id| table.is_live(id)), [true, false, true]);
    assert_eq!(table.resolve(&ActorPath::new("aether.window/aether.window.surface:left")), None);
    let beneath = ActorPath::new("aether.window/aether.window.surface:left/aether.window.layer");
    assert_eq!(table.resolve(&beneath), Some(layer));
}

#[test]
fn failed_turn_ends_its_actor() {
    // Catches a view that follows `Close` and passes over the ending the engine writes for a failed turn.
    let (window, root) = window();
    let mut table = table(1);
    let Ok(fault) = Fault::new(&Unreadable) else {
        panic!("a reason that holds nothing encodes");
    };
    table.fold(&[root, Effect::new(at(window), What::Failed(fault))], &context());

    assert!(!table.is_live(window));
}

#[test]
#[should_panic(expected = "already held")]
fn actor_made_under_a_held_id_stops_the_engine() {
    // Catches a second actor made under a live actor's id quietly replacing the first.
    let (window, root) = window();
    let (_, spawn) = surface(window);
    let mut table = table(4);
    table.fold(&[root, spawn.clone(), spawn], &context());
}

#[test]
fn reader_sees_nothing_before_the_fold() {
    // Catches an ending of an actor the table never held making an entry for it.
    let (window, _) = window();
    let mut table = table(4);
    table.fold(&[Effect::new(at(window), What::Close)], &context());

    assert!(!table.is_live(window));
    assert_eq!(table.path(window), None);
}

#[test]
fn part_count_does_not_change_the_table() {
    // Catches a record folded into one part and read from another: every read agrees for one part and for many.
    let (window, root) = window();
    let (surface, spawn) = surface(window);
    let (layer, spawn_layer) = layer(surface);
    let effects = [root, spawn, spawn_layer, Effect::new(at(layer), What::Close)];
    let seen = [1, 2, 7, 64].map(|parts| {
        let mut table = table(parts);
        table.fold(&effects, &context());

        [window, surface, layer].map(|id| (table.is_live(id), table.path(id), table.parent(id)))
    });

    assert!(seen.iter().all(|read| *read == seen[0]));
    assert!(!seen[0][2].0);
}
