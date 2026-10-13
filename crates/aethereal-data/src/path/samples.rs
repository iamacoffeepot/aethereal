//! What the path tests share.

use crate::path::{KeyName, TypedKey};
use crate::{Budget, DecodeError, Plain, from_bytes, to_bytes};
use alloc::string::String;
use alloc::vec;

/// `text` encoded as a `String` and decoded as a `T`, as text arrives from outside.
pub fn decode_text<T: Plain>(text: &str) -> Result<T, DecodeError> {
    let Ok(bytes) = to_bytes(&String::from(text)) else {
        panic!("text encodes as a string");
    };

    from_bytes(&bytes, &mut Budget::new(1 << 12))
}

/// A key with one value, written `left`: the key of the surface the path tests build.
pub enum Side {
    Left,
}

const LEFT: KeyName = KeyName::new("left");

impl TypedKey for Side {
    fn to_name(&self) -> KeyName {
        LEFT
    }

    fn from_name(name: &KeyName) -> Option<Self> {
        let is_left = *name == LEFT;

        is_left.then_some(Self::Left)
    }
}

/// The text of a path `depth` segments deep, each segment `a`.
pub fn deep(depth: usize) -> String {
    vec!["a"; depth].join("/")
}
