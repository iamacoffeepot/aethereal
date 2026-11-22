//! Spike: each kind built for a guest writes its id into its module's `aethereal.kinds` section (ADR-0054).
//!
//! The section is the concatenation, in whatever order the linker takes its inputs, of one 8-byte id per kind
//! compiled into the module. The spike also writes each kind's name, NUL-ended, into `aethereal.kindnames`, so a
//! reader can say which kinds the ids are; the real implementation writes only the ids.

/// Writes the id of the kind `$kind` into the module's `aethereal.kinds` section when built for a guest, and nothing
/// otherwise. `#[derive(Plain)]` and `#[derive(Bound)]` call it for each type `#[kind]` names that has no parameters.
#[cfg(all(feature = "guest", target_family = "wasm"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __kind_in_module {
    ($kind:ty) => {
        const _: () = {
            #[unsafe(link_section = "aethereal.kinds")]
            #[used]
            static ID: [u8; 8] = $crate::KindId::of::<$kind>().plain_bytes();

            const NAME: &$crate::KindName = &<$kind as $crate::Kind>::NAME;
            const LEN: usize = NAME.as_str().len() + 1;
            #[unsafe(link_section = "aethereal.kindnames")]
            #[used]
            static NAMED: [u8; LEN] = $crate::__kind_name_bytes::<LEN>(NAME.as_str());
        };
    };
}

/// Built for anything but a guest, a kind writes nothing.
#[cfg(not(all(feature = "guest", target_family = "wasm")))]
#[doc(hidden)]
#[macro_export]
macro_rules! __kind_in_module {
    ($kind:ty) => {};
}

/// `name`'s bytes and a NUL, in `N` bytes, for the spike's name section.
#[doc(hidden)]
#[must_use]
pub const fn kind_name_bytes<const N: usize>(name: &str) -> [u8; N] {
    let mut out = [0u8; N];
    let bytes = name.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        out[index] = bytes[index];
        index += 1;
    }

    out
}
