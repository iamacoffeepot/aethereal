//! Kinds that reach each other through references: ids computed as one cycle, alike as a constant and from a
//! description (ADR-0006, ADR-0016).

use crate::{
    Boundary, Budget, Decode, DecodeError, DescriptionFault, Encode, EncodeError, Kind, KindDescription, KindId, Plain,
    ProtocolNode, ProtocolShape, Reader, Referent, Schema, SchemaNode, SchemaNodes, Shape, Writer, from_bytes,
    to_bytes,
};
use alloc::vec::Vec;
use core::marker::PhantomData;

/// A stand-in for a reference to an actor proven to take the protocol `P`: what the actor crate's `ProtocolRef` lays
/// out, written as eight bytes.
struct To<P>(PhantomData<P>);

/// A protocol a [`To`] names, by its kinds' nodes.
trait Protocol {
    const PROTOCOL: ProtocolShape;
}

impl<A: Kind> Protocol for (A,) {
    const PROTOCOL: ProtocolShape = ProtocolShape::Told(&[A::KIND]);
}

impl<A: Kind, B: Kind> Protocol for (A, B) {
    const PROTOCOL: ProtocolShape = ProtocolShape::Told(&[A::KIND, B::KIND]);
}

impl<P: Protocol> Schema for To<P> {
    const SHAPE: Shape = Shape::Reference { written: &u64::SHAPE, protocol: P::PROTOCOL };
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

impl<P: Protocol> Plain for To<P> {}

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

/// A cycle through a named protocol: a request whose sender is reached through the protocol that takes it, answered
/// with a reply that names the same protocol, so the cycle runs through an entry and its answer.
mod asking {
    use super::{Protocol, To};
    use crate::{Answered, Kind, Plain, ProtocolShape};

    pub struct Asking;

    impl Protocol for Asking {
        const PROTOCOL: ProtocolShape = ProtocolShape::Named(&[Answered { kind: Ask::KIND, answers: &[Reply::KIND] }]);
    }

    #[derive(Plain)]
    #[kind(name = "test.ask")]
    pub struct Ask {
        pub back: To<Asking>,
    }

    #[derive(Plain)]
    #[kind(name = "test.reply")]
    pub struct Reply {
        pub to: To<Asking>,
        pub value: u32,
    }

    /// Outside the cycle: names the protocol.
    #[derive(Plain)]
    #[kind(name = "test.hold")]
    pub struct Hold {
        pub asking: To<Asking>,
    }
}

/// A cycle of one through the answer of a kind outside it: `Echo` names a protocol whose one entry, `Note`, is answered
/// with `Echo`.
mod echoing {
    use super::{Protocol, To};
    use crate::{Answered, Kind, Plain, ProtocolShape};

    pub struct Echoing;

    impl Protocol for Echoing {
        const PROTOCOL: ProtocolShape = ProtocolShape::Named(&[Answered { kind: Note::KIND, answers: &[Echo::KIND] }]);
    }

    #[derive(Plain)]
    #[kind(name = "test.note")]
    pub struct Note {
        pub pitch: u8,
    }

    #[derive(Plain)]
    #[kind(name = "test.echo")]
    pub struct Echo {
        pub to: To<Echoing>,
    }
}

/// A cycle of two kinds of one name, told apart by their layouts with members counted but not told apart.
mod same {
    use super::To;
    use crate::Plain;

    #[derive(Plain)]
    #[kind(name = "test.same")]
    pub struct First {
        pub to: To<(Second,)>,
        pub value: u8,
    }

    #[derive(Plain)]
    #[kind(name = "test.same")]
    pub struct Second {
        pub to: To<(First,)>,
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
fn described<K: Kind>() -> Result<KindId, DescriptionFault> {
    KindDescription::of(K::KIND).id()
}

#[test]
fn cycle_ids_are_pinned() {
    // Catches a fold whose bytes move from ADR-0006's text: a cycle hashed in another order, a position or a count
    // written otherwise, or a named protocol's answers folded otherwise inside a cycle than outside one.
    // Tripwire: ids computed from ADR-0006's text by a separate program, for a two-kind cycle and a kind outside it
    // that refers in, a cycle of one and a kind referring in, a cycle through a named protocol's answer and a kind
    // outside it, a cycle of one through the answer of a kind outside it, and a cycle of two kinds of one name.
    const PINNED: [KindId; 11] = [
        KindId::of::<pair::Subscribe>(),
        KindId::of::<pair::Update>(),
        KindId::of::<pair::Watch>(),
        KindId::of::<pointer::Pointed>(),
        KindId::of::<pointer::Reach>(),
        KindId::of::<asking::Ask>(),
        KindId::of::<asking::Reply>(),
        KindId::of::<asking::Hold>(),
        KindId::of::<echoing::Echo>(),
        KindId::of::<same::First>(),
        KindId::of::<same::Second>(),
    ];
    let expected = [
        0x27d5_6a99_a8bc_f760,
        0x837a_a9d7_c71d_c731,
        0xe298_084c_7f93_7fb0,
        0x45f0_aeff_f1a4_63b2,
        0xb77a_1c97_f681_43d1,
        0xdb04_12d2_ac26_ed2c,
        0x7259_927c_5874_4e3d,
        0xc679_32dd_3207_3336,
        0x8b03_ee92_2a61_ac4a,
        0xd4d1_1388_2d5e_fef5,
        0x5aee_71dc_0c9c_dd43,
    ];

    assert_eq!(PINNED.map(bits), expected);
}

#[test]
fn descriptions_make_the_constant_ids() {
    // Catches a flat fold that writes a member, a position, an outside id, or an answer differently from the constant
    // one.
    assert_eq!(described::<pair::Subscribe>(), Ok(KindId::of::<pair::Subscribe>()));
    assert_eq!(described::<pair::Update>(), Ok(KindId::of::<pair::Update>()));
    assert_eq!(described::<pair::Watch>(), Ok(KindId::of::<pair::Watch>()));
    assert_eq!(described::<pointer::Pointed>(), Ok(KindId::of::<pointer::Pointed>()));
    assert_eq!(described::<pointer::Reach>(), Ok(KindId::of::<pointer::Reach>()));
    assert_eq!(described::<asking::Ask>(), Ok(KindId::of::<asking::Ask>()));
    assert_eq!(described::<asking::Reply>(), Ok(KindId::of::<asking::Reply>()));
    assert_eq!(described::<asking::Hold>(), Ok(KindId::of::<asking::Hold>()));
    assert_eq!(described::<echoing::Echo>(), Ok(KindId::of::<echoing::Echo>()));
    assert_eq!(described::<same::First>(), Ok(KindId::of::<same::First>()));
    assert_eq!(described::<same::Second>(), Ok(KindId::of::<same::Second>()));
}

#[test]
fn a_cycle_is_described_whole_and_one_outside_alone() {
    // Catches a member described alone, which would name its own id, or a kind outside described with the cycle.
    let subscribe = KindDescription::of(pair::Subscribe::KIND);
    let pointed = KindDescription::of(pointer::Pointed::KIND);

    assert!(matches!(subscribe, KindDescription::Cycle { ref members, .. } if members.len() == 2));
    assert!(matches!(pointed, KindDescription::Cycle { ref members, .. } if members.len() == 1));
    assert!(matches!(KindDescription::of(pair::Watch::KIND), KindDescription::Alone { .. }));
}

#[test]
fn listing_order_does_not_change_an_id() {
    // Catches a host that takes the order a guest lists members in as their positions.
    let KindDescription::Cycle { mut members, this } = KindDescription::of(asking::Reply::KIND) else {
        panic!("`Reply` is a member of a cycle");
    };
    members.reverse();
    for member in &mut members {
        member.schema = relisted(&member.schema);
    }
    let reversed = KindDescription::Cycle { members, this: 1 - this };

    assert_eq!(reversed.id(), Ok(KindId::of::<asking::Reply>()));
}

#[test]
fn listing_order_does_not_change_the_ids_of_one_name() {
    // Catches a host that orders members of one name by the order they are listed in rather than by their layouts.
    let KindDescription::Cycle { mut members, this } = KindDescription::of(same::Second::KIND) else {
        panic!("`Second` is a member of a cycle");
    };
    members.reverse();
    for member in &mut members {
        member.schema = relisted(&member.schema);
    }
    let reversed = KindDescription::Cycle { members, this: 1 - this };

    assert_eq!(reversed.id(), Ok(KindId::of::<same::Second>()));
}

/// `schema`, of a member of a two-kind cycle, with the two members' places swapped.
fn relisted(schema: &SchemaNodes) -> SchemaNodes {
    let Ok(bytes) = to_bytes(schema) else {
        panic!("a schema encodes");
    };
    let Ok(mut nodes) = from_bytes::<Vec<SchemaNode>>(&bytes, &mut Budget::new(1 << 16)) else {
        panic!("a schema decodes as its nodes");
    };
    for node in &mut nodes {
        match node {
            SchemaNode::Reference { protocol: ProtocolNode::Told(kinds), .. } => swapped(kinds),
            SchemaNode::Reference { protocol: ProtocolNode::Named(entries), .. } => {
                for entry in entries.iter_mut() {
                    entry.kind = swap(entry.kind);
                    swapped(&mut entry.answers);
                }
                entries.sort_unstable();
            }
            _ => {}
        }
    }
    let Ok(bytes) = to_bytes(&nodes) else {
        panic!("nodes encode");
    };
    let Ok(schema) = from_bytes::<SchemaNodes>(&bytes, &mut Budget::new(1 << 16)) else {
        panic!("relisted nodes are a schema");
    };

    schema
}

/// `referent` with the two members' places swapped.
const fn swap(referent: Referent) -> Referent {
    match referent {
        Referent::Member(listed) => Referent::Member(1 - listed),
        Referent::Kind(id) => Referent::Kind(id),
    }
}

/// `referents` with the two members' places swapped, in ascending order again.
fn swapped(referents: &mut [Referent]) {
    for referent in referents.iter_mut() {
        *referent = swap(*referent);
    }
    referents.sort_unstable();
}

#[test]
fn a_changed_member_changes_every_id_that_reaches_it() {
    // Catches a cycle hash blind to a member's layout, or a kind outside blind to the cycle it refers into.
    assert_ne!(KindId::of::<widened::Subscribe>(), KindId::of::<pair::Subscribe>());
    assert_ne!(KindId::of::<widened::Update>(), KindId::of::<pair::Update>());
    assert_ne!(KindId::of::<widened::Watch>(), KindId::of::<pair::Watch>());
}

#[test]
fn a_cycle_redefined_alike_has_the_same_ids() {
    // Catches an id that depends on where a kind is defined: the site tells nodes apart, and is never hashed.
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
fn a_described_cycle_that_is_not_one_is_refused() {
    // Catches a host that hashes members which do not reach each other, so two descriptions of one kind differ.
    let KindDescription::Cycle { members, .. } = KindDescription::of(pair::Update::KIND) else {
        panic!("`Update` is a member of a cycle");
    };
    let first = members.into_iter().take(1).collect();
    let alone = KindDescription::Cycle { members: first, this: 0 };

    assert_eq!(alone.id(), Err(DescriptionFault::MemberPastList));
}

#[test]
fn a_member_described_alone_is_refused() {
    // Catches a host that hashes a schema naming a member by its place as a kind in no cycle, or panics on it.
    let KindDescription::Cycle { members, .. } = KindDescription::of(pointer::Pointed::KIND) else {
        panic!("`Pointed` is a member of a cycle");
    };
    let Some(member) = members.into_iter().next() else {
        panic!("a cycle has members");
    };
    let alone = KindDescription::Alone { name: member.name, schema: member.schema };

    assert_eq!(alone.id(), Err(DescriptionFault::MemberAlone));
}
