//! Plain data that may be a map key or a set element (ADR-0004).

use crate::Plain;
use alloc::string::String;
use alloc::vec::Vec;
use core::num::{NonZeroI8, NonZeroI16, NonZeroI32, NonZeroI64, NonZeroU8, NonZeroU16, NonZeroU32, NonZeroU64};

/// Plain data that may be a map key or a set element (ADR-0004).
///
/// `Ord` is the order ADR-0002 states for the shape.
pub trait Key: Plain + Ord {}

impl Key for u8 {}
impl Key for u16 {}
impl Key for u32 {}
impl Key for u64 {}
impl Key for i8 {}
impl Key for i16 {}
impl Key for i32 {}
impl Key for i64 {}
impl Key for char {}
impl Key for NonZeroU8 {}
impl Key for NonZeroU16 {}
impl Key for NonZeroU32 {}
impl Key for NonZeroU64 {}
impl Key for NonZeroI8 {}
impl Key for NonZeroI16 {}
impl Key for NonZeroI32 {}
impl Key for NonZeroI64 {}
impl Key for String {}
impl Key for Vec<u8> {}
impl<const LENGTH: usize> Key for [u8; LENGTH] {}
