//! Where generated code says it stands (ADR-0056: Expanded by a macro): at the markup that wrote it, so rustc's error on
//! it points there, while it still resolves as the macro's own code.

use proc_macro2::Span;

/// A span that resolves names as the macro's own code does, placed at `span` in the markup: an error on what it marks
/// points at the markup, and a lint that skips code a macro wrote skips it.
pub fn at(span: Span) -> Span {
    Span::call_site().located_at(span)
}

/// A span for a name the macro makes for itself, a local of the code it writes, which no reference in the markup can
/// name or shadow.
pub fn hidden() -> Span {
    Span::mixed_site()
}
