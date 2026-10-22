//! A program's configuration: plain data, the composite of its roots' sections (ADR-0028).
//!
//! Each root that has settings brings a section: a settings kind, under the short name the root gives it as it is
//! added (ADR-0029). Layers are overlays on one document, the first winning, merged field by field and decoded once
//! with the composite's schema, and each section is then taken by the root that reads it. A field is named by the
//! handle `#[settings]` emits beside it, and the flags a program's users type are a table built from what each kind
//! suggests and what the program changes.
//!
//! The handles and the settings declarations are always here, as the derives write them; the composite, its
//! overlays, its flags, and the values a program sets in code read and write JSON, and are behind the `json`
//! feature.

pub(crate) mod field;
mod settings;

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
#[cfg(feature = "json")]
mod reader;
#[cfg(all(test, feature = "json"))]
mod samples;
#[cfg(feature = "json")]
mod setting;

pub use field::Field;
pub use settings::{Declared, Settings, Suggested};

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
#[cfg(feature = "json")]
pub use reader::RootSettings;
#[cfg(feature = "json")]
pub use setting::Setting;
