//! A state's portable form: what a version of an actor's code reads, by field name, out of the state another version
//! left (ADR-0054).

#[cfg(all(test, feature = "std"))]
mod tests;

use crate::{Within, Written};
use aethereal_data::{Budget, Decode, DecodeError, FieldBytes, NamedFields, Schema, SchemaNodes};
use core::error::Error;
use core::fmt;

/// A state that is read, by field name, into a form of its own out of the state another version of its code left
/// (ADR-0054).
///
/// `#[derive(Portability)]` writes it for a struct: a struct beside it, named `Portable` and the state's name, with
/// the state's fields under the same names, each an `Option` of the field's type, and this trait linking the two.
/// Every field is an `Option`: a field the old state lacks is absent, and the new code gives it a value or works it
/// out from the others.
///
/// ```
/// use aethereal_actor::{OldState, Portability, Portable, TakeOverError, Written};
/// use aethereal_data::{Plain, Schema, SchemaNodes};
///
/// #[derive(Plain)]
/// struct OldHud {
///     health: f32,
///     banner: String,
/// }
///
/// #[derive(Plain, Portability)]
/// struct Hud {
///     health: f32,
///     show_cooldowns: bool,
/// }
///
/// impl Hud {
///     fn from_old(old: Portable<Self>) -> Self {
///         Self { health: old.health.unwrap_or(100.0), show_cooldowns: old.show_cooldowns.unwrap_or(false) }
///     }
/// }
///
/// let old = OldHud { health: 40.0, banner: String::from("north") };
/// let written = Written::of(&old)?;
/// let schema = SchemaNodes::of(&OldHud::SHAPE);
/// let hud = Hud::from_old(Hud::read(&OldState::new(&written, &schema, 1024))?);
///
/// assert_eq!((hud.health, hud.show_cooldowns), (40.0, false));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub trait Portability {
    /// This version's portable form: the state's fields under their names, each an `Option` of the field's type.
    type Form;

    /// The portable form of `old`: each field read by its name, with the schema `old` was written with.
    ///
    /// # Errors
    ///
    /// [`TakeOverError::Mismatch`] for a field `old` holds with another layout, and [`TakeOverError::Unreadable`]
    /// when `old` is not a struct of its schema, or a field does not read as its type within the read limit.
    fn read(old: &OldState<'_>) -> Result<Self::Form, TakeOverError>;
}

/// The portable form of `T`'s state: inside an impl for a state, `Portable<Self>` is its generated struct (ADR-0054).
pub type Portable<T> = <T as Portability>::Form;

/// An actor's state as its old code left it: the bytes it was taken out as at [`Within`], with a pin on each blob
/// they refer to, and the schema its kind had in that code, names included (ADR-0054).
#[derive(Debug, Clone, Copy)]
pub struct OldState<'a> {
    written: &'a Written,
    schema: &'a SchemaNodes,
    read_limit: usize,
}

impl<'a> OldState<'a> {
    /// The state `written` holds, written with `schema`, read allocating at most `read_limit` bytes in all: the
    /// engine's read limit, as every read at [`Within`] is.
    #[must_use]
    pub const fn new(written: &'a Written, schema: &'a SchemaNodes, read_limit: usize) -> Self {
        Self { written, schema, read_limit }
    }

    /// Its top-level fields, to be read by name into a portable form, each within what the fields read before it left
    /// of the read limit.
    ///
    /// # Errors
    ///
    /// [`TakeOverError::Unreadable`] when its schema is not a struct's, or its bytes are not a value of its schema.
    pub fn fields(&self) -> Result<OldFields<'a>, TakeOverError> {
        let found = NamedFields::of(self.schema, self.written.bytes())?;

        Ok(OldFields { found, written: self.written, budget: Budget::new(self.read_limit) })
    }
}

/// An old state's top-level fields, each read by its name at [`Within`], all of them within one read limit
/// (ADR-0054).
pub struct OldFields<'a> {
    found: NamedFields<'a>,
    written: &'a Written,
    budget: Budget,
}

impl OldFields<'_> {
    /// The field `name` read as a `T`: none when the old state has no field of that name.
    ///
    /// It is read with the old state's own pins, so a reference in it is a copy of the pin the old state keeps on its
    /// blob, and a handle in it is its ticket: each comes across as the old state held it. A field no one reads is
    /// never read: its references go with the old state's bytes, and its handles stay with their actor, as a dropped
    /// handle does (ADR-0010, ADR-0014).
    ///
    /// # Errors
    ///
    /// [`TakeOverError::Mismatch`] when the old state holds the field with another layout, and
    /// [`TakeOverError::Unreadable`] when its bytes do not read as a `T` within what is left of the read limit.
    pub fn read<T: Decode<Within> + Schema>(&mut self, name: &'static str) -> Result<Option<T>, TakeOverError> {
        match self.found.field(name, &T::SHAPE) {
            FieldBytes::Absent => Ok(None),
            FieldBytes::OtherLayout => Err(TakeOverError::Mismatch { field: name }),
            FieldBytes::Same(bytes) => {
                let value = self.written.read_part(bytes, &mut self.budget)?;
                Ok(Some(value))
            }
        }
    }
}

/// Why an actor's state does not move onto a version of its code (ADR-0054).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TakeOverError {
    /// The old state holds the field `field` with another layout. A default would quietly throw its value away, so
    /// the move is refused; a version that changes a field's type names the field anew.
    Mismatch {
        /// The field's name.
        field: &'static str,
    },
    /// The old state does not read: it is not a struct of the schema it was written with, or a field does not read as
    /// its new type, or reading it would take the read past the engine's limit.
    Unreadable(DecodeError),
    /// The old state has no field `field`, which this version cannot work out: what a hand-written `take_over` refuses
    /// with, through [`Self::missing`].
    Missing {
        /// The field's name.
        field: &'static str,
    },
    /// The layouts differ and this version wrote no `take_over`: what [`Upgrades::take_over`](crate::Upgrades)
    /// answers until it is written.
    Unwritten,
}

impl TakeOverError {
    /// The old state has no field `field`, and this version cannot work it out: for a hand-written `take_over` that
    /// requires one.
    #[must_use]
    pub const fn missing(field: &'static str) -> Self {
        Self::Missing { field }
    }
}

impl From<DecodeError> for TakeOverError {
    fn from(error: DecodeError) -> Self {
        Self::Unreadable(error)
    }
}

impl fmt::Display for TakeOverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mismatch { field } => write!(f, "the old state holds `{field}` with another layout"),
            Self::Unreadable(error) => write!(f, "the old state does not read: {error}"),
            Self::Missing { field } => write!(f, "the old state has no `{field}`"),
            Self::Unwritten => f.write_str("the layout changed and no `take_over` is written"),
        }
    }
}

impl Error for TakeOverError {}
