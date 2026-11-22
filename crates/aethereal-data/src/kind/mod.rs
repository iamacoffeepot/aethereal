//! Kinds: plain data with a name (ADR-0006).

mod cycle;
mod graph;
mod id;
pub mod layout;
mod name;
#[cfg(test)]
pub mod samples;
mod shape;

pub use cycle::{CycleMember, DescriptionFault, KindDescription};
pub use graph::kind_id;
pub use id::KindId;
pub use name::KindName;
pub use shape::{KindPtr, KindShape, Site};

use crate::Schema;
use core::marker::PhantomData;

/// A described type with a name (ADR-0006).
///
/// A kind may hold a reference to a protocol that takes it, directly or through other kinds, and its id is computed
/// with its cycle. The derive computes the id of each kind with no parameters where the kind is defined, so a cycle
/// the hash refuses stops the build there, though nothing uses the kind. `To<P>` below stands for a reference to an
/// actor that takes the kinds `P` (ADR-0016).
///
/// A cycle through a generic kind, which has no static node:
///
/// ```compile_fail,E0080
/// # use aethereal_data::{Boundary, Decode, DecodeError, Encode, EncodeError, Kind, Plain, ProtocolShape, Reader, Schema};
/// # use aethereal_data::{Shape, Writer};
/// # use core::marker::PhantomData;
/// # struct To<P>(PhantomData<P>);
/// # trait Protocol { const PROTOCOL: ProtocolShape; }
/// # impl<A: Kind> Protocol for (A,) { const PROTOCOL: ProtocolShape = ProtocolShape::Told(&[A::KIND]); }
/// # impl<P: Protocol> Schema for To<P> {
/// #     const SHAPE: Shape = Shape::Reference { written: &u64::SHAPE, protocol: P::PROTOCOL };
/// # }
/// # impl<B: Boundary, P> Encode<B> for To<P> {
/// #     fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> { 0u64.encode(out) }
/// # }
/// # impl<B: Boundary, P> Decode<B> for To<P> {
/// #     fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> { u64::decode(input).map(|_| Self(PhantomData)) }
/// # }
/// # impl<P: Protocol> Plain for To<P> {}
/// #[derive(Plain)]
/// #[kind(name = "test.held")]
/// struct Held {
///     to: To<(Generic<u8>,)>,
/// }
///
/// #[derive(Plain)]
/// #[kind(name = "test.generic")]
/// struct Generic<T> {
///     back: To<(Held,)>,
///     value: T,
/// }
/// ```
///
/// Two members of one name and one layout, which no order tells apart:
///
/// ```compile_fail,E0080
/// # use aethereal_data::{Boundary, Decode, DecodeError, Encode, EncodeError, Kind, Plain, ProtocolShape, Reader, Schema};
/// # use aethereal_data::{Shape, Writer};
/// # use core::marker::PhantomData;
/// # struct To<P>(PhantomData<P>);
/// # trait Protocol { const PROTOCOL: ProtocolShape; }
/// # impl<A: Kind> Protocol for (A,) { const PROTOCOL: ProtocolShape = ProtocolShape::Told(&[A::KIND]); }
/// # impl<P: Protocol> Schema for To<P> {
/// #     const SHAPE: Shape = Shape::Reference { written: &u64::SHAPE, protocol: P::PROTOCOL };
/// # }
/// # impl<B: Boundary, P> Encode<B> for To<P> {
/// #     fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> { 0u64.encode(out) }
/// # }
/// # impl<B: Boundary, P> Decode<B> for To<P> {
/// #     fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> { u64::decode(input).map(|_| Self(PhantomData)) }
/// # }
/// # impl<P: Protocol> Plain for To<P> {}
/// #[derive(Plain)]
/// #[kind(name = "test.twin")]
/// struct One {
///     to: To<(Two,)>,
/// }
///
/// #[derive(Plain)]
/// #[kind(name = "test.twin")]
/// struct Two {
///     to: To<(One,)>,
/// }
/// ```
///
/// A graph past a cap: a reference that lists more than 1,024 kinds, here one kind 1,025 times.
///
/// ```compile_fail,E0080
/// # use aethereal_data::{Boundary, Decode, DecodeError, Encode, EncodeError, Kind, Plain, ProtocolShape, Reader, Schema};
/// # use aethereal_data::{Shape, Writer};
/// # use core::marker::PhantomData;
/// # struct To<P>(PhantomData<P>);
/// # trait Protocol { const PROTOCOL: ProtocolShape; }
/// # impl<A: Kind> Protocol for (A,) { const PROTOCOL: ProtocolShape = ProtocolShape::Told(&[A::KIND]); }
/// # impl<P: Protocol> Schema for To<P> {
/// #     const SHAPE: Shape = Shape::Reference { written: &u64::SHAPE, protocol: P::PROTOCOL };
/// # }
/// # impl<B: Boundary, P> Encode<B> for To<P> {
/// #     fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> { 0u64.encode(out) }
/// # }
/// # impl<B: Boundary, P> Decode<B> for To<P> {
/// #     fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> { u64::decode(input).map(|_| Self(PhantomData)) }
/// # }
/// # impl<P: Protocol> Plain for To<P> {}
/// struct Many;
///
/// impl Protocol for Many {
///     const PROTOCOL: ProtocolShape = ProtocolShape::Told(&[Wide::KIND; 1025]);
/// }
///
/// #[derive(Plain)]
/// #[kind(name = "test.wide")]
/// struct Wide {
///     to: To<Many>,
/// }
/// ```
pub trait Kind: Schema {
    /// The kind's name.
    const NAME: KindName;
    /// The kind's node in the graph of kinds its references reach, which its id is computed from (ADR-0006).
    ///
    /// The derive gives a non-generic kind a static node, which may be a member of a cycle. Any other kind's node is
    /// made in a constant, as here, and may not be.
    const KIND: KindPtr = KindPtr::of(Unsited::<Self>::NODE);
}

/// The node of a kind with no static one, made in a constant.
struct Unsited<K: ?Sized>(PhantomData<K>);

impl<K: Kind + ?Sized> Unsited<K> {
    const NODE: &'static KindShape = &KindShape { name: K::NAME, site: Site::Unsited, shape: &K::SHAPE };
}

/// A value as the tuple of kinds it is: a kind is one, `()` none. What a handler of mail returns is read so, as the one
/// kind it answers with or none (ADR-0059).
///
/// It is written here, beside `Kind`, as only the crate that owns `Kind` can say that `()` is none: a crate above could
/// not implement one trait for both `()` and every kind, since this crate could implement `Kind` for `()`.
pub trait IntoKinds {
    /// The kinds, as a tuple: `(K,)` for a kind `K`, `()` for `()`.
    type Kinds;

    /// The value as its kinds.
    fn into_kinds(self) -> Self::Kinds;
}

impl<K: Kind> IntoKinds for K {
    type Kinds = (K,);

    fn into_kinds(self) -> (K,) {
        (self,)
    }
}

impl IntoKinds for () {
    type Kinds = ();

    fn into_kinds(self) {}
}
