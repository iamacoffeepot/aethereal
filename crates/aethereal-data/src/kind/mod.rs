//! Kinds: plain data with a name (ADR-0006).

mod id;
pub mod layout;
mod module;
mod name;
#[cfg(test)]
pub mod samples;

pub use id::KindId;
#[doc(hidden)]
pub use module::kind_name_bytes as __kind_name_bytes;
pub use name::KindName;

use crate::Schema;

/// A described type with a name (ADR-0006).
pub trait Kind: Schema {
    /// The kind's name.
    const NAME: KindName;
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
