//! What the renderer has made: released only by its maker, and all of it when its maker ends.

use super::*;

/// The actor numbered `number`.
fn actor(number: u64) -> ActorId {
    ActorId::new(number)
}

#[test]
fn an_id_once_released_names_nothing_again() {
    // Catches ids taken back and given again: an actor still holding a released id would draw another's mesh.
    let mut ids = Ids::default();
    let mut made = Made::default();
    let first = ids.next();
    made.keep(first, actor(1), "first");
    let freed = made.release(first, actor(1));
    let second = ids.next();
    made.keep(second, actor(1), "second");

    assert_eq!(freed, Owned::Mine("first"));
    assert_ne!(first, second);
    assert_eq!(made.release(first, actor(1)), Owned::Unknown);
}

#[test]
fn only_its_maker_releases_what_it_made() {
    // Catches a release that does not ask who asked: any actor handed an id could take it from under its maker.
    let mut made = Made::default();
    made.keep(1, actor(1), "mesh");

    assert_eq!(made.release(1, actor(2)), Owned::NotOwner);
    assert_eq!(made.release(1, actor(1)), Owned::Mine("mesh"));
}

#[test]
fn a_maker_that_ends_takes_what_it_made_and_nothing_else() {
    // Catches an ending that releases another actor's things, or leaves some of its own: the GPU would leak, or
    // lose what another still draws.
    let mut made = Made::default();
    made.keep(1, actor(1), "one");
    made.keep(2, actor(2), "two");
    made.keep(3, actor(1), "three");

    assert_eq!(made.release_all(actor(1)), 2);
    assert_eq!(made.release(2, actor(2)), Owned::Mine("two"));
    assert_eq!(made.release(3, actor(1)), Owned::Unknown);
}

#[test]
fn a_maker_is_watched_from_the_first_thing_it_makes_until_it_ends() {
    // Catches a maker watched again for each thing it makes, or never forgotten once it ends.
    let mut makers = Makers::default();

    assert!(makers.made(actor(1)));
    assert!(!makers.made(actor(1)));
    assert!(makers.ended(actor(1)));
    assert!(!makers.ended(actor(1)));
}

#[test]
fn only_its_maker_writes_what_is_kept() {
    // Catches a write let through for an actor that did not make the thing, which would let any actor handed an id
    // rewrite another's records.
    let mut made = Made::default();
    made.keep(1, actor(1), 5);

    assert_eq!(made.owned(1, actor(2)), Owned::NotOwner);
    assert_eq!(made.owned(2, actor(1)), Owned::Unknown);
    if let Owned::Mine(value) = made.owned(1, actor(1)) {
        *value = 6;
    }
    assert_eq!(made.release(1, actor(1)), Owned::Mine(6));
}
