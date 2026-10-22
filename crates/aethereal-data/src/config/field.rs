//! A handle naming one field of a settings kind (ADR-0028).

use core::fmt;
use core::marker::PhantomData;

/// One field of the settings kind `S`, holding a `V`: the handle `#[settings]` emits for each named field (ADR-0028).
///
/// `EngineSettings::WORKERS: Field<EngineSettings, NonZeroU32>` names `workers` of `EngineSettings`, so a program's
/// flags name a field that exists, checked when it is built. It names that field in each section read as `S`.
pub struct Field<S, V> {
    name: &'static str,
    of: PhantomData<fn() -> (S, V)>,
}

impl<S, V> Field<S, V> {
    /// The field's name in its settings.
    #[must_use]
    pub const fn name(self) -> &'static str {
        self.name
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
        f.debug_tuple("Field").field(&self.name).finish()
    }
}

/// The handle of the field `name` of `S`; only the derive writes one.
#[doc(hidden)]
#[must_use]
pub const fn field<S, V>(name: &'static str) -> Field<S, V> {
    Field { name, of: PhantomData }
}
