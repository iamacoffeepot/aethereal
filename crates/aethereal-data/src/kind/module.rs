//! A kind's id, computed where the kind is defined, and written into a guest's module (ADR-0006, ADR-0054).
//!
//! Built for a guest, each kind with no parameters writes its id, 8 bytes, into the module's section
//! `aethereal.kinds` from a static of its own. The linker joins every same-named section of the objects it links,
//! in the order it takes them, so the section is a run of whole ids with no count and no order, and a kind two types
//! declare is written twice: a host reads it as a set. A guest is linked with fat LTO, which loads every crate the
//! guest names into one object, so every kind of those crates is listed, whether or not its code is reached.

/// Computes the id of the kind `$kind`, so a kind whose id cannot be computed stops the build where it is defined,
/// and, built for a guest, writes the id into the module's `aethereal.kinds` section. `#[derive(Plain)]` and
/// `#[derive(Bound)]` call it for each type `#[kind]` names that has no parameters.
///
/// It is defined twice, as this crate is built for a guest or not, so the derive never knows the target.
#[cfg(all(feature = "guest", target_family = "wasm"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __kind_id {
    ($kind:ty) => {
        const _: () = {
            #[unsafe(link_section = "aethereal.kinds")]
            #[used]
            static ID: [u8; 8] = <$kind as $crate::Kind>::KIND.id().plain_bytes();
        };
    };
}

/// Computes the id of the kind `$kind`, so a kind whose id cannot be computed stops the build where it is defined.
/// Built for anything but a guest, nothing is written.
#[cfg(not(all(feature = "guest", target_family = "wasm")))]
#[doc(hidden)]
#[macro_export]
macro_rules! __kind_id {
    ($kind:ty) => {
        const _: $crate::KindId = <$kind as $crate::Kind>::KIND.id();
    };
}
