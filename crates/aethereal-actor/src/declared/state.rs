//! The schema of an actor type's state, its names included (ADR-0004, ADR-0054).

use aethereal_data::{Kind, SchemaNodes, Shape};
use alloc::borrow::Cow;
use core::marker::PhantomData;

/// The schema of an actor type's state, its names included: what a state taken out of an actor is read by, field by
/// field (ADR-0004, ADR-0054). An actor is a kind (ADR-0009), so its layout is the one the type's id is made from.
#[derive(Debug, Clone, Copy)]
pub enum StateSchema {
    /// The shape of a type the code is built with: a native type, or a guest's own as its section is written.
    Built(&'static Shape),
    /// The schema a guest's section states for a type of its blob, as its host read it (ADR-0015). Kept flat, as it
    /// was read: a shape unfolds what its nodes share, and a schema a guest wrote could unfold past any walk's end.
    Read(&'static SchemaNodes),
}

impl StateSchema {
    /// The schema of the state of `K`, as it is built.
    #[must_use]
    pub const fn of<K: Kind>() -> Self {
        Self::Built(Shaped::<K>::SHAPE)
    }

    /// Its flat form, its names included: what a state is read by.
    #[must_use]
    pub fn nodes(self) -> Cow<'static, SchemaNodes> {
        match self {
            Self::Built(shape) => Cow::Owned(SchemaNodes::of(shape)),
            Self::Read(nodes) => Cow::Borrowed(nodes),
        }
    }
}

/// `K`'s shape held by reference, so a constant can keep it.
struct Shaped<K>(PhantomData<K>);

impl<K: Kind> Shaped<K> {
    const SHAPE: &'static Shape = &K::SHAPE;
}
