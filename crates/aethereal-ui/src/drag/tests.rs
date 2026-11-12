//! What a drag carries and what a target takes: a payload reads back only as the kind it was made of, and a target
//! takes the kind its method takes last.

use super::{DropTarget, Payload};
use aethereal_data::{KindId, Plain};

/// An item a test drags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.item")]
pub struct Item {
    pub id: u32,
}

/// A spell a test drags: laid out as an item is, a kind of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.spell")]
pub struct Spell {
    pub id: u32,
}

/// What a test's target raises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placed {
    At(u32),
}

/// A method a drop calls: its slot, then the item dropped.
fn place_item(_: u32, _: Spell, _: Item) {}

#[test]
fn a_payload_reads_back_only_as_the_kind_it_was_made_of() {
    // Catches a payload read by its bytes alone, so a spell dropped on an item's slot reads as the item of the same
    // number and equips it.
    let payload = Payload::of(Item { id: 7 });

    assert_eq!(payload.read::<Item>(), Some(Item { id: 7 }));
    assert_eq!(payload.read::<Spell>(), None);
}

#[test]
fn a_target_takes_the_kind_its_method_takes_last() {
    // Catches the kind taken from another parameter of the method than its last, the one the payload is handed as, so a
    // slot whose method takes a spell before the item takes spells and refuses items.
    let target = DropTarget::new(Placed::At(3), place_item);

    assert!(target.takes(KindId::of::<Item>()));
    assert!(!target.takes(KindId::of::<Spell>()));
}
