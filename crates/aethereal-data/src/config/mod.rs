//! A program's configuration: plain data, the composite of its parts' sections (ADR-0028).
//!
//! Each part a chassis composes brings a section, a config kind under a short name, and the program brings one of
//! its own. Layers are overlays on one document, the first winning, merged field by field and decoded once with the
//! composite's schema. A field is named by the handle `#[section]` emits beside it, and the flags a program's users
//! type are a table built from what each part suggests and what the program changes.
//!
//! The handles and the section declarations are always here, as the derives write them; the composite, its
//! overlays, and its flags read and write JSON, and are behind the `json` feature.

pub(crate) mod field;
mod section;

#[cfg(feature = "json")]
mod document;
#[cfg(feature = "json")]
mod error;
#[cfg(feature = "json")]
mod flags;
#[cfg(feature = "json")]
mod form;
#[cfg(feature = "json")]
mod overlay;
#[cfg(feature = "json")]
mod parts;
#[cfg(feature = "json")]
mod place;
#[cfg(all(test, feature = "json"))]
mod samples;

pub use field::Field;
pub use section::{Declared, Section, Suggested};

#[cfg(feature = "json")]
pub use error::{ConfigError, ConfigFault, FlagError, FlagName, RepeatedSection};
#[cfg(feature = "json")]
pub use flags::{Flag, FlagTable, Flags, Named};
#[cfg(feature = "json")]
pub use form::{Form, NumberForm};
#[cfg(feature = "json")]
pub use overlay::Overlay;
#[cfg(feature = "json")]
pub use parts::{Configuration, Parts};
