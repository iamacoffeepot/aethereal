use super::*;
use crate::section;
use crate::{Actor, Beginning, Declared, Ending, Fault, Handles, Tells, Turn};
use aethereal_data::{Kind, Plain};
use alloc::vec::Vec;

/// A tell, sent at a typed path.
#[derive(Plain)]
#[kind(name = "aether.test.order")]
struct Order;

/// An actor named `name`, keyed by `key`, standing beneath `parent`, that handles orders and
/// may send `sends` as it begins.
macro_rules! actor {
    ($name:ident, $named:literal, $key:ty, $parent:ty, $sends:ty) => {
        #[derive(Plain)]
        struct $name;

        impl Kind for $name {
            const NAME: KindName = KindName::new($named);
        }

        impl ChildOf for $name {
            type Parent = $parent;
        }

        impl Actor for $name {
            type Config = $name;
            type Key = $key;
            type Handles = (Order,);
            type BeginSends = $sends;
            type EndSends = ();

            fn begin(config: $name, _: &mut Beginning<'_, Self, $sends>) -> Result<Self, Fault> {
                Ok(config)
            }

            fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
                Ok(())
            }
        }

        impl Handles<Order> for $name {
            type Sends = ();

            fn handle(&mut self, _: Order, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
                Ok(())
            }
        }
    };
}

actor!(World, "aether.test.world", (), Top, ());
actor!(Army, "aether.test.army", u64, World, ());
actor!(Hud, "aether.test.hud", (), Army, ());
actor!(Squad, "aether.test.squad", u64, Hud, ());
actor!(Teller, "aether.test.teller", (), Top, (Tells<Path<Squad>, Order>, Tells<Path<World>, Order>));

#[test]
fn a_path_names_its_segments_from_the_root_each_key_at_its_segment() {
    // Catches segments filled in leaf first, a key put at a segment other than its own, and a singleton given one.
    let squad = path::<Squad>((3, 7));
    let world = path::<World>(());

    let expected = ActorPath::new("aether.test.world/aether.test.army:3/aether.test.hud/aether.test.squad:7");
    assert_eq!(squad.names(), &expected);
    assert_eq!(world.names(), &ActorPath::new("aether.test.world"));
}

#[test]
fn a_send_at_a_typed_path_reads_back_from_a_guests_section() {
    // Catches the writer of a guest's section and the types a host reads it as drifting apart for a send at a typed
    // path: a segment dropped or read in another order, or a keyed one read as a singleton (ADR-0015, ADR-0022).
    const TYPES: &[&Declared] = &[Declared::of::<Teller>()];
    const LEN: usize = section::len(TYPES);
    const BYTES: [u8; LEN] = section::bytes::<LEN>(TYPES);

    let mut budget = aethereal_data::Budget::new(1 << 16);
    let Ok(made) = aethereal_data::from_bytes::<Vec<section::Made>>(&BYTES, &mut budget) else {
        panic!("a section reads back");
    };

    let leaked: Vec<&Declared> = made.iter().map(section::Made::leak).collect();
    assert_eq!(leaked, TYPES);
}
