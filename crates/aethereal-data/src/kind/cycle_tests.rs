//! Kinds that reach each other through references: ids computed as one cycle, alike as a constant and from a
//! description (ADR-0006, ADR-0016).

use super::{CycleFault, Kind, KindDescription, KindId, KindPtr};
use crate::{
    Boundary, Decode, DecodeError, Encode, EncodeError, Plain, ProtocolShape, Reader, Schema, SchemaNodes, Shape,
    Writer, to_bytes,
};
use core::marker::PhantomData;

/// A stand-in for a reference to an actor proven to take the tuple of kinds `P`: what the actor crate's `ProtocolRef`
/// lays out.
struct To<P>(PhantomData<P>);

/// A tuple of kinds, by their nodes.
trait Told {
    const NODES: &'static [KindPtr];
}

impl<A: Kind> Told for (A,) {
    const NODES: &'static [KindPtr] = &[A::KIND];
}

impl<A: Kind, B: Kind> Told for (A, B) {
    const NODES: &'static [KindPtr] = &[A::KIND, B::KIND];
}

impl<P: Told> Schema for To<P> {
    const SHAPE: Shape = Shape::Reference { written: &u64::SHAPE, protocol: ProtocolShape::Told(P::NODES) };
}

impl<B: Boundary, P> Encode<B> for To<P> {
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        0u64.encode(out)
    }
}

impl<B: Boundary, P> Decode<B> for To<P> {
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        u64::decode(input).map(|_| Self(PhantomData))
    }
}

impl<P: Told> Plain for To<P> {}

/// A two-kind cycle: each holds a reference to a protocol that takes the other.
mod pair {
    use super::To;
    use crate::Plain;

    #[derive(Plain)]
    #[kind(name = "test.subscribe")]
    pub struct Subscribe {
        pub to: To<(Update,)>,
    }

    #[derive(Plain)]
    #[kind(name = "test.update")]
    pub struct Update {
        pub back: To<(Subscribe,)>,
        pub value: u32,
    }

    /// Outside the cycle, referring into it.
    #[derive(Plain)]
    #[kind(name = "test.watch")]
    pub struct Watch {
        pub feed: To<(Update, Subscribe)>,
    }
}

/// `pair` with `Update`'s value widened: the same names, one member's layout changed.
mod widened {
    use super::To;
    use crate::Plain;

    #[derive(Plain)]
    #[kind(name = "test.subscribe")]
    pub struct Subscribe {
        pub to: To<(Update,)>,
    }

    #[derive(Plain)]
    #[kind(name = "test.update")]
    pub struct Update {
        pub back: To<(Subscribe,)>,
        pub value: u64,
    }

    #[derive(Plain)]
    #[kind(name = "test.watch")]
    pub struct Watch {
        pub feed: To<(Update, Subscribe)>,
    }
}

/// The window's pointer event, a cycle of one: it holds, two types down, a reference to a protocol that takes it.
mod pointer {
    use super::To;
    use crate::Plain;

    #[derive(Plain)]
    #[kind(name = "test.pointed")]
    pub struct Pointed {
        pub entry: u32,
        pub event: Event,
    }

    #[derive(Plain)]
    pub enum Event {
        Up { x: f32, over: Over },
        Left,
    }

    #[derive(Plain)]
    pub enum Over {
        Nothing,
        Entry { actor: To<(Pointed,)>, entry: u32 },
    }

    /// Outside the cycle: an entry of a stack, naming the protocol that takes `Pointed`.
    #[derive(Plain)]
    #[kind(name = "test.reach")]
    pub struct Reach {
        pub actor: To<(Pointed,)>,
        pub entry: u32,
    }
}

/// An id's 64 bits, read back from its bytes; its value is private.
fn bits(id: KindId) -> u64 {
    let Ok(bytes) = to_bytes(&id) else {
        panic!("an id encodes");
    };
    let Ok(eight) = <[u8; 8]>::try_from(bytes) else {
        panic!("an id is eight bytes");
    };

    u64::from_le_bytes(eight)
}

/// The id `K`'s description makes.
fn described<K: Kind>() -> KindId {
    let Ok(id) = KindDescription::of(K::KIND).id() else {
        panic!("a kind's own description is one cycle");
    };

    id
}

#[test]
fn cycle_ids_are_constants() {
    // Catches a cycle that is not hashed when the program is built, or a fold whose order moves.
    // Tripwire: ids of the cycle hash's members, the `pointed` cycle of one, and kinds outside each that refer in,
    // computed by this hash and checked against the description path below.
    const SUBSCRIBE: KindId = KindId::of::<pair::Subscribe>();
    const UPDATE: KindId = KindId::of::<pair::Update>();
    const WATCH: KindId = KindId::of::<pair::Watch>();
    const POINTED: KindId = KindId::of::<pointer::Pointed>();
    const REACH: KindId = KindId::of::<pointer::Reach>();
    let pinned = [bits(SUBSCRIBE), bits(UPDATE), bits(WATCH), bits(POINTED), bits(REACH)];
    assert_eq!(pinned, PINNED);
}

/// The ids `cycle_ids_are_constants` pins.
const PINNED: [u64; 5] =
    [0x27d5_6a99_a8bc_f760, 0x837a_a9d7_c71d_c731, 0xe298_084c_7f93_7fb0, 0x45f0_aeff_f1a4_63b2, 0xb77a_1c97_f681_43d1];

#[test]
fn descriptions_make_the_constant_ids() {
    // Catches a flat fold that writes a member, a position, or an outside id differently from the constant one.
    assert_eq!(described::<pair::Subscribe>(), KindId::of::<pair::Subscribe>());
    assert_eq!(described::<pair::Update>(), KindId::of::<pair::Update>());
    assert_eq!(described::<pair::Watch>(), KindId::of::<pair::Watch>());
    assert_eq!(described::<pointer::Pointed>(), KindId::of::<pointer::Pointed>());
    assert_eq!(described::<pointer::Reach>(), KindId::of::<pointer::Reach>());
}

#[test]
fn a_cycle_is_described_whole_and_one_outside_alone() {
    // Catches a member described alone, which names its own id, or an outside kind described with the cycle.
    assert!(
        matches!(KindDescription::of(pair::Subscribe::KIND), KindDescription::Cycle { ref members, .. } if members.len() == 2)
    );
    assert!(
        matches!(KindDescription::of(pointer::Pointed::KIND), KindDescription::Cycle { ref members, .. } if members.len() == 1)
    );
    assert!(matches!(KindDescription::of(pair::Watch::KIND), KindDescription::Alone { .. }));
}

#[test]
fn listing_order_does_not_change_an_id() {
    // Catches a host that takes the order a guest lists members in as their positions.
    let KindDescription::Cycle { mut members, this } = KindDescription::of(pair::Update::KIND) else {
        panic!("`Update` is a member of a cycle");
    };
    members.reverse();
    for member in &mut members {
        member.schema = relisted(&member.schema);
    }
    let reversed = KindDescription::Cycle { members, this: 1 - this };

    assert_eq!(reversed.id(), Ok(KindId::of::<pair::Update>()));
}

/// `schema` with its two members' places swapped.
fn relisted(schema: &SchemaNodes) -> SchemaNodes {
    use crate::{ProtocolNode, Referent, SchemaNode};
    let swap = |referent: &Referent| match *referent {
        Referent::Member(listed) => Referent::Member(1 - listed),
        Referent::Kind(id) => Referent::Kind(id),
    };
    let Ok(bytes) = to_bytes(schema) else {
        panic!("a schema encodes");
    };
    let Ok(mut nodes) = crate::from_bytes::<std::vec::Vec<SchemaNode>>(&bytes, &mut crate::Budget::new(1 << 16)) else {
        panic!("a schema decodes as its nodes");
    };
    for node in &mut nodes {
        if let SchemaNode::Reference { protocol: ProtocolNode::Told(kinds), .. } = node {
            *kinds = kinds.iter().map(swap).collect();
            kinds.sort_unstable();
        }
    }
    let Ok(bytes) = to_bytes(&nodes) else {
        panic!("nodes encode");
    };
    let Ok(schema) = crate::from_bytes::<SchemaNodes>(&bytes, &mut crate::Budget::new(1 << 16)) else {
        panic!("relisted nodes are a schema");
    };

    schema
}

#[test]
fn a_changed_member_changes_every_id_that_reaches_it() {
    // Catches a cycle hash blind to a member's layout, or an outside kind blind to the cycle it refers into.
    assert_ne!(KindId::of::<widened::Subscribe>(), KindId::of::<pair::Subscribe>());
    assert_ne!(KindId::of::<widened::Update>(), KindId::of::<pair::Update>());
    assert_ne!(KindId::of::<widened::Watch>(), KindId::of::<pair::Watch>());
}

#[test]
fn a_cycle_redefined_alike_has_the_same_ids() {
    // Catches an id that depends on where a kind is defined: the site tells nodes apart, it is never hashed.
    mod again {
        use super::To;
        use crate::Plain;

        #[derive(Plain)]
        #[kind(name = "test.subscribe")]
        pub struct Subscribe {
            pub to: To<(Update,)>,
        }

        #[derive(Plain)]
        #[kind(name = "test.update")]
        pub struct Update {
            pub back: To<(Subscribe,)>,
            pub value: u32,
        }
    }

    assert_eq!(KindId::of::<again::Subscribe>(), KindId::of::<pair::Subscribe>());
    assert_eq!(KindId::of::<again::Update>(), KindId::of::<pair::Update>());
}

#[test]
fn members_of_a_cycle_have_distinct_ids() {
    // Catches members given one id: a cycle's hash without each member's position.
    assert_ne!(KindId::of::<pair::Subscribe>(), KindId::of::<pair::Update>());
}

#[test]
fn a_described_cycle_that_is_not_one_is_refused() {
    // Catches a host that hashes members which do not reach each other, so two descriptions of one kind differ.
    let KindDescription::Cycle { members, .. } = KindDescription::of(pair::Update::KIND) else {
        panic!("`Update` is a member of a cycle");
    };
    let Some(first) = members.first().cloned() else {
        panic!("a cycle has members");
    };
    let alone = KindDescription::Cycle { members: std::vec::Vec::from([first]), this: 0 };

    assert!(matches!(alone.id(), Err(CycleFault::MemberPastList | CycleFault::NotOneCycle)));
}
