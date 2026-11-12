//! What a screen works out again rather than keep (ADR-0054): a value held beside the screen's state that is written as
//! nothing and read back as its default, so moving the screen onto new code, or taking its state out to seat it
//! elsewhere, drops it, and the screen works it out again. A screen's last tree and the fonts it decoded are such
//! values: neither is plain data, and both are made again by the next `Prepare`, the fonts by asking once more.

use aethereal_data::{Boundary, Decode, DecodeError, Encode, EncodeError, Reader, Schema, Shape, Writer};
use core::ops::{Deref, DerefMut};

/// `T`, held but not kept: written as nothing, and read back as `T::default()`.
#[derive(Debug, Default)]
pub struct Unkept<T>(T);

impl<T> Deref for Unkept<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for Unkept<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

/// Nothing is written, so its shape is the empty tuple's.
impl<T> Schema for Unkept<T> {
    const SHAPE: Shape = Shape::Tuple { elements: &[] };
}

impl<T, B: Boundary> Encode<B> for Unkept<T> {
    fn encode(&self, _: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        Ok(())
    }
}

impl<T: Default, B: Boundary> Decode<B> for Unkept<T> {
    fn decode(_: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        Ok(Self(T::default()))
    }
}
