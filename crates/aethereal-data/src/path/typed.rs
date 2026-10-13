//! Typed keys: the value an actor's code holds where a path holds a key's name (ADR-0008).

use crate::path::KeyName;
use alloc::string::ToString;

/// A key that has a name: what tells the actors of an instanced type apart (ADR-0008).
///
/// Each value gives a different name, and a name reads back as the value that
/// wrote it, so a path keeps one spelling.
///
/// ```rust
/// use aethereal_data::{KeyName, TypedKey};
///
/// #[derive(Debug, PartialEq)]
/// enum Side {
///     Left,
///     Right,
/// }
///
/// const LEFT: KeyName = KeyName::new("left");
/// const RIGHT: KeyName = KeyName::new("right");
///
/// impl TypedKey for Side {
///     fn to_name(&self) -> KeyName {
///         match self {
///             Self::Left => LEFT,
///             Self::Right => RIGHT,
///         }
///     }
///
///     fn from_name(name: &KeyName) -> Option<Self> {
///         match name {
///             name if *name == LEFT => Some(Self::Left),
///             name if *name == RIGHT => Some(Self::Right),
///             _ => None,
///         }
///     }
/// }
///
/// assert_eq!(Side::from_name(&Side::Right.to_name()), Some(Side::Right));
/// assert_eq!(Side::from_name(&KeyName::new("middle")), None);
/// ```
pub trait TypedKey: Sized {
    /// The name this key is written as in a path.
    fn to_name(&self) -> KeyName;

    /// The key `name` was written from, or none when `name` is not a key of this type.
    fn from_name(name: &KeyName) -> Option<Self>;
}

/// A `u64` is a key, written in decimal with no sign and no leading zero (ADR-0008).
impl TypedKey for u64 {
    fn to_name(&self) -> KeyName {
        KeyName::from_conforming(&self.to_string())
    }

    fn from_name(name: &KeyName) -> Option<Self> {
        let text = name.text();
        let is_number = text.bytes().all(|byte| byte.is_ascii_digit());
        let is_padded = text.len() > 1 && text.starts_with('0');
        let is_canonical = is_number && !is_padded;

        if is_canonical {
            text.parse().ok()
        } else {
            None
        }
    }
}

/// The key an actor's type names: `()` for a singleton, a [`TypedKey`] for an instanced type (ADR-0008).
///
/// It holds for `()` and for every `TypedKey`, and for nothing else: an
/// author implements `TypedKey`, never this.
///
/// ```compile_fail
/// use aethereal_data::{ActorKey, KeyName};
///
/// struct Mine;
///
/// impl ActorKey for Mine {
///     fn name(&self) -> Option<KeyName> {
///         None
///     }
/// }
/// ```
pub trait ActorKey: sealed::Sealed {
    /// The key's name, or none for a singleton, which has no key.
    fn name(&self) -> Option<KeyName>;
}

impl ActorKey for () {
    fn name(&self) -> Option<KeyName> {
        None
    }
}

impl<K: TypedKey> ActorKey for K {
    fn name(&self) -> Option<KeyName> {
        Some(self.to_name())
    }
}

mod sealed {
    use super::TypedKey;

    /// Held by `()` and every `TypedKey`, so `ActorKey` holds for nothing else.
    pub trait Sealed {}

    impl Sealed for () {}

    impl<K: TypedKey> Sealed for K {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_key_reads_back_at_its_ends() {
        // Catches a number written with padding or a sign, and a reader that loses the widest value.
        for number in [0, 7, 340, u64::MAX] {
            assert_eq!(u64::from_name(&number.to_name()), Some(number));
        }
        assert_eq!(0u64.to_name(), KeyName::new("0"));
        assert_eq!(u64::MAX.to_name(), KeyName::new("18446744073709551615"));
    }

    #[test]
    fn number_key_has_one_spelling() {
        // Catches a reader that takes a second spelling of a number, or a name that is not one.
        for text in ["007", "00", "7a", "left", "18446744073709551616"] {
            assert_eq!(u64::from_name(&KeyName::from_conforming(text)), None, "the key `{text}`");
        }
    }
}
