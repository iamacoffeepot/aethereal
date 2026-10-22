//! What a root reads its settings as: a settings kind under its section, or nothing (ADR-0029).

use super::{ConfigError, Configuration, Parts, RepeatedSection, Settings};

/// What a root's bootstrap reads its settings as (ADR-0029): a settings kind, under the section the root names, or
/// `()`, for a root that has no settings and brings no section.
pub trait RootSettings: Sized + 'static {
    /// Whether it brings a section: false only for `()`.
    const SECTIONED: bool;

    /// `parts` with its section added under `name`, its defaults `defaults`, when it brings one.
    ///
    /// # Errors
    ///
    /// [`RepeatedSection`] when `parts` already holds a section of that name.
    fn compose(parts: Parts, name: &'static str, defaults: &Self) -> Result<Parts, RepeatedSection>;

    /// Its section `name`, taken from `configuration`.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Untaken`] when no section of that name, read as this kind, is left.
    fn take(configuration: &mut Configuration, name: &'static str) -> Result<Self, ConfigError>;
}

impl<S: Settings> RootSettings for S {
    const SECTIONED: bool = true;

    fn compose(parts: Parts, name: &'static str, defaults: &Self) -> Result<Parts, RepeatedSection> {
        parts.with_defaults(name, defaults)
    }

    fn take(configuration: &mut Configuration, name: &'static str) -> Result<Self, ConfigError> {
        configuration.take::<S>(name)
    }
}

impl RootSettings for () {
    const SECTIONED: bool = false;

    fn compose(parts: Parts, _: &'static str, &(): &Self) -> Result<Parts, RepeatedSection> {
        Ok(parts)
    }

    fn take(_: &mut Configuration, _: &'static str) -> Result<Self, ConfigError> {
        Ok(())
    }
}
