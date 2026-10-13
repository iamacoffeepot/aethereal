//! The table's tests: what a reader sees, what is refused, and that everything written is found.

use super::*;
use aethereal_data::KindName;
#[cfg(not(target_arch = "wasm32"))]
use std::thread;

const WINDOW: KindName = KindName::new("aether.window");
const SURFACE: KindName = KindName::new("aether.window.surface");

/// The root `aether.window:<key>` made live in `table`.
fn window(table: &ActorTable, key: u64) -> ActorId {
    let Ok(id) = table.add_root(&PathSegment::new(WINDOW, &key)) else {
        panic!("the root `aether.window:{key}` is free");
    };

    id
}

/// The segment `aether.window.surface:<key>`.
fn surface(key: u64) -> PathSegment {
    PathSegment::new(SURFACE, &key)
}

/// The path of the child `segment` names beneath `parent`, as the table holds it.
fn beneath(table: &ActorTable, parent: ActorId, segment: &PathSegment) -> ActorPath {
    let Some(Ok(path)) = table.path(parent).map(|above| above.child(segment)) else {
        panic!("the parent is in the table, and a child fits beneath it");
    };

    path
}

#[test]
fn reader_sees_the_table_at_its_change() {
    // Catches a reader shown a later spawn or a later close, and a close that reaches back before it was made.
    let table = ActorTable::new();
    let parent = window(&table, 0);
    let before_spawn = table.latest();
    let Ok(child) = table.add_child(parent, &surface(0)) else {
        panic!("the child is free");
    };
    let path = beneath(&table, parent, &surface(0));
    let before_close = table.latest();
    assert_eq!(table.standing(child, before_spawn), Standing::Absent);
    assert_eq!(table.standing(child, before_close), Standing::Live);
    assert_eq!(table.resolve(&path, before_spawn), None);
    assert_eq!(table.resolve(&path, before_close), Some(child));

    let closed = table.close(child);
    assert_eq!(closed, Ok(table.latest()));
    assert_eq!(table.close(child), Err(NotLive));
    assert_eq!(table.standing(child, before_close), Standing::Live);
    assert_eq!(table.standing(child, table.latest()), Standing::Closed);
    assert_eq!(table.resolve(&path, before_close), Some(child));
    assert_eq!(table.resolve(&path, table.latest()), None);
    assert_eq!(table.path(child), Some(path));
}

#[test]
fn used_path_is_never_spawned_again() {
    // Catches a closed path handed out again, and a key refused beneath a parent that never used it.
    let table = ActorTable::new();
    let parent = window(&table, 0);
    let other = window(&table, 1);
    let Ok(child) = table.add_child(parent, &surface(0)) else {
        panic!("the child is free");
    };
    assert_eq!(table.add_child(parent, &surface(0)), Err(Refused::KeyTaken));
    assert_eq!(table.close(child).map(|_| ()), Ok(()));
    let after_close = table.latest();

    assert_eq!(table.add_child(parent, &surface(0)), Err(Refused::KeyTaken));
    assert_eq!(table.add_root(&PathSegment::new(WINDOW, &0u64)), Err(Refused::KeyTaken));
    assert_eq!(table.latest(), after_close);
    assert!(table.add_child(other, &surface(0)).is_ok());
}

#[test]
fn child_needs_a_live_parent_and_room_beneath_it() {
    // Catches a child made beneath nothing or beneath a closed actor, and a depth limit one off.
    let table = ActorTable::new();
    let parent = window(&table, 0);
    assert_eq!(table.add_child(child_id(parent, &surface(0)), &surface(0)), Err(Refused::ParentNotLive));
    assert_eq!(table.close(parent).map(|_| ()), Ok(()));
    assert_eq!(table.add_child(parent, &surface(0)), Err(Refused::ParentNotLive));

    let mut deepest = window(&table, 1);
    for _ in 1..ActorPath::MAX_DEPTH {
        let Ok(deeper) = table.add_child(deepest, &surface(0)) else {
            panic!("a path may be {} segments deep", ActorPath::MAX_DEPTH);
        };
        deepest = deeper;
    }
    assert_eq!(table.add_child(deepest, &surface(0)), Err(Refused::TooDeep));
}

#[test]
fn second_path_with_one_id_is_refused() {
    // Catches a second path taking the place of the first under one id, and the two read as one actor.
    let table = ActorTable::new();
    let first = ActorPath::new("aether.window");
    let second = ActorPath::new("aether.window.surface");
    let id = path_id(&first);
    assert_eq!(table.insert(id, first.clone()), Ok(id));
    assert_eq!(table.insert(id, second.clone()), Err(Refused::IdShared));
    assert_eq!(table.path(id), Some(first));
    assert_eq!(table.resolve(&second, table.latest()), None);
}

#[test]
fn every_entry_is_found_again() {
    // Catches a descent that takes the wrong four bits, or loses a line once nodes are several deep.
    const CHILDREN: u64 = 4096;
    let table = ActorTable::new();
    let parent = window(&table, 0);
    for key in 0..CHILDREN {
        assert!(table.add_child(parent, &surface(key)).is_ok(), "the child `{key}`");
    }

    let latest = table.latest();
    for key in 0..CHILDREN {
        let path = beneath(&table, parent, &surface(key));
        let id = child_id(parent, &surface(key));
        assert_eq!(table.resolve(&path, latest), Some(id), "the child `{key}`");
        assert_eq!(table.path(id), Some(path), "the child `{key}`");
    }
    assert_eq!(table.standing(child_id(parent, &surface(CHILDREN)), latest), Standing::Absent);
}

// Native only: the browser host runs the engine on one thread (ADR-0001).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn writers_in_separate_branches_lose_nothing() {
    // Catches a line lost when two writers reach one node together, and a latest number that skips or stalls.
    const WRITERS: u64 = 4;
    const CHILDREN: u64 = 500;
    let table = ActorTable::new();
    let parents: Vec<ActorId> = (0..WRITERS).map(|key| window(&table, key)).collect();
    thread::scope(|scope| {
        for parent in &parents {
            let table = &table;
            scope.spawn(move || {
                for key in 0..CHILDREN {
                    assert!(table.add_child(*parent, &surface(key)).is_ok(), "the child `{key}`");
                }
            });
        }
    });

    let latest = table.latest();
    assert_eq!(latest, Change(WRITERS + WRITERS * CHILDREN));
    for parent in parents {
        for key in 0..CHILDREN {
            assert_eq!(table.standing(child_id(parent, &surface(key)), latest), Standing::Live, "the child `{key}`");
        }
    }
}
