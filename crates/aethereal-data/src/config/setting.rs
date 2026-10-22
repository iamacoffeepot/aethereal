//! A value a program sets in code for one field, typed by the field's handle (ADR-0028).

use super::document::Json;
use super::{ConfigFault, Field};
use crate::{Bare, Budget, Encode, JsonError, Schema, SchemaNodes, Writer, to_json};
use alloc::boxed::Box;
use core::fmt;
use core::marker::PhantomData;

/// A value for one field of the settings kind `S`, of the type the field's handle names: what a program sets in code
/// (ADR-0028).
///
/// It is made by [`Field::to`], so the value is of the field's type, checked when the program is built.
///
/// ```
/// use aethereal_data::config::{Overlay, Parts};
/// use aethereal_data::{Budget, Plain};
///
/// #[derive(Plain, Default)]
/// #[kind(name = "example.show.settings")]
/// #[settings]
/// struct Show {
///     seconds: u32,
/// }
///
/// let parts = Parts::new().with::<Show>("show")?;
/// let mut code = Overlay::new("code");
/// code.put("show", Show::SECONDS.to(3))?;
///
/// let mut configuration = parts.configure(&[code], &mut Budget::new(1 << 20))?;
/// assert_eq!(configuration.take::<Show>("show")?.seconds, 3);
/// # Ok::<(), Box<dyn core::error::Error>>(())
/// ```
///
/// A value of another type does not build.
///
/// ```compile_fail
/// use aethereal_data::Plain;
///
/// #[derive(Plain, Default)]
/// #[kind(name = "example.show.settings")]
/// #[settings]
/// struct Show {
///     seconds: u32,
/// }
///
/// let setting = Show::SECONDS.to("three");
/// ```
pub struct Setting<S> {
    field: &'static str,
    value: Box<dyn Written>,
    of: PhantomData<fn() -> S>,
}

impl<S> Setting<S> {
    /// The name of the field it sets.
    #[must_use]
    pub const fn field(&self) -> &'static str {
        self.field
    }

    /// The value, as the document holds it.
    pub(super) fn json(&self) -> Result<Json, ConfigFault> {
        self.value.json()
    }
}

impl<S> fmt::Debug for Setting<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Setting").field("field", &self.field).finish_non_exhaustive()
    }
}

impl<S, V: Encode<Bare> + Schema + 'static> Field<S, V> {
    /// The field set to `value`, for a program to set in code (ADR-0028).
    #[must_use]
    pub fn to(self, value: V) -> Setting<S> {
        Setting { field: self.name(), value: Box::new(value), of: PhantomData }
    }
}

/// A value of a type that states its schema, its type erased: what a [`Setting`] holds.
trait Written {
    /// The value as JSON, written through its schema as ADR-0007 writes every kind.
    fn json(&self) -> Result<Json, ConfigFault>;
}

impl<V: Encode<Bare> + Schema> Written for V {
    /// The value is in hand, so what writing it holds grows with its size and needs no budget of its own.
    fn json(&self) -> Result<Json, ConfigFault> {
        let fault = |error: JsonError| ConfigFault::Json(error.fault());
        let mut out = Writer::<Bare>::new(());
        self.encode(&mut out).map_err(JsonError::from).map_err(fault)?;
        let mut budget = Budget::new(usize::MAX);
        let text = to_json(&SchemaNodes::of(&V::SHAPE), &out.into_bytes(), &mut budget).map_err(fault)?;

        Json::read(&text)
    }
}
