//! A handle naming one field of a settings kind, or one field of an element of a settings kind that is a sequence
//! (ADR-0028).

use super::settings::{Members, Sole};
use alloc::vec::Vec;
use core::fmt;
use core::marker::PhantomData;

/// One field of the settings kind `S`, holding a `V`: the handle `#[settings]` emits for each named field, or one made
/// from it for a field of an element of a sequence (ADR-0028).
///
/// `EngineSettings::WORKERS: Field<EngineSettings, NonZeroU32>` names `workers` of `EngineSettings`, so a program's
/// flags name a field that exists, checked when it is built. It names that field in each section read as `S`.
///
/// ```
/// use aethereal_data::Plain;
/// use aethereal_data::config::Field;
///
/// #[derive(Plain, Default)]
/// #[kind(name = "example.window")]
/// #[settings]
/// pub struct WindowSettings {
///     /// The window's inner size.
///     pub size: [u32; 2],
/// }
///
/// #[derive(Plain, Default)]
/// #[kind(name = "example.windows")]
/// #[settings]
/// pub struct WindowsSettings(pub Vec<WindowSettings>);
///
/// // `windows[0].size` in a section `windows` read as `WindowsSettings`.
/// const FIRST_SIZE: Field<WindowsSettings, [u32; 2]> = WindowSettings::SIZE.in_element(0);
/// assert_eq!(FIRST_SIZE.name(), "size");
/// ```
pub struct Field<S, V> {
    within: Within,
    name: &'static str,
    doc: &'static str,
    of: PhantomData<fn() -> (S, V)>,
}

/// Where in a section a handle's field is, fixed as the handle is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Within {
    /// A field of the section itself: `engine.workers`.
    Section,
    /// A field of the element at this index of a section that is a sequence: `windows[0].size`.
    Element(usize),
}

impl<S, V> Field<S, V> {
    /// The field's name in its settings, or in the element of its settings it is in.
    #[must_use]
    pub const fn name(self) -> &'static str {
        self.name
    }

    /// The field's doc comment, its lines joined by newlines.
    #[must_use]
    pub const fn doc(self) -> &'static str {
        self.doc
    }

    /// Where in a section the field is.
    #[cfg_attr(not(feature = "json"), allow(dead_code))]
    pub(crate) const fn within(self) -> Within {
        self.within
    }
}

impl<E: Members, V> Field<E, V> {
    /// This field of the element at `index` of the settings `S`, a sequence of `E` written as the sequence alone:
    /// `WindowSettings::SIZE.in_element(0)` names `windows[0].size`, the first window's size.
    ///
    /// A section with no element at `index` refuses the value a layer sets there, naming its path.
    #[must_use]
    pub const fn in_element<S>(self, index: usize) -> Field<S, V>
    where
        S: Sole<Value = Vec<E>>,
    {
        Field { within: Within::Element(index), name: self.name, doc: self.doc, of: PhantomData }
    }
}

impl<S, V> Clone for Field<S, V> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S, V> Copy for Field<S, V> {}

impl<S, V> fmt::Debug for Field<S, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Field").field("within", &self.within).field("name", &self.name).field("doc", &self.doc).finish()
    }
}

/// The handle of the field `name` of `S`, documented `doc`; only the derive writes one.
#[doc(hidden)]
#[must_use]
pub const fn field<S, V>(name: &'static str, doc: &'static str) -> Field<S, V> {
    Field { within: Within::Section, name, doc, of: PhantomData }
}
