//! A route's captures, typed (ADR-0035).

use aethereal_data::Plain;

/// A route's captures, typed: read from the slices of the path its `{names}` matched.
///
/// A route's pattern names its captures `NAMES`, in this order, or it is refused
/// [`Uncaptured`](crate::Unroutable::Uncaptured) as the router is built.
pub trait Captures: Plain + Clone {
    /// The names a route's pattern captures, in the order they appear in it.
    const NAMES: &'static [&'static str];

    /// The captures `slices` hold, one for each of `NAMES` and in its order, as sent: none when one does not read as
    /// its type, which the router answers `404`, as no route matches it.
    fn read(slices: &[&str]) -> Option<Self>;
}

/// A route with no captures.
impl Captures for () {
    const NAMES: &'static [&'static str] = &[];

    fn read(_: &[&str]) -> Option<Self> {
        Some(())
    }
}
