//! An overlay: the fields one layer sets, at their paths, under the layer's name (ADR-0028).

use super::document::{Json, Leaf, Step, path_of};
use super::form::Form;
use super::place::Place;
use super::{ConfigError, ConfigFault, Flag, Parts};
use crate::JsonPath;
use alloc::string::String;
use alloc::vec::Vec;

/// One value a layer sets, at its path from the configuration's root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Entry {
    /// The path, from the configuration's root.
    pub(super) path: Vec<Step>,
    /// The value set there.
    pub(super) value: Json,
}

/// The fields one layer sets, at their paths, and the layer's name (ADR-0028).
///
/// Overlays are merged field by field, the first winning, over the defaults, and the result is decoded once with
/// the composite's schema ([`Parts::configure`]). Within one overlay a later entry wins over an earlier one at the
/// same place, as a flag given twice on a command line takes the last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overlay {
    layer: String,
    entries: Vec<Entry>,
}

impl Overlay {
    /// An overlay that sets nothing yet, for the layer named `layer`: `command line`, `environment`, `page`.
    #[must_use]
    pub fn new(layer: impl Into<String>) -> Self {
        Self { layer: layer.into(), entries: Vec::new() }
    }

    /// The name of the layer.
    #[must_use]
    pub fn layer(&self) -> &str {
        &self.layer
    }

    /// The entries, in the order they were set.
    pub(super) fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Sets `flag`'s field to `text`, read by the flag's form.
    pub fn read(&mut self, flag: &Flag, text: &str) -> Result<(), ConfigError> {
        let path = flag.steps();
        let value = flag.form().read(text);
        self.push(path, value)
    }

    /// Sets a switch's field on, as the flag alone does, or off, as `--no-<name>` does.
    pub fn switch(&mut self, flag: &Flag, on: bool) -> Result<(), ConfigError> {
        let path = flag.steps();
        self.push(path, Ok(Json::Leaf(Leaf::Boolean(on))))
    }

    /// Sets the field `--set path=value` names, its value read as the flag of that field's type would read it, and
    /// as JSON for a structure.
    pub fn set(&mut self, parts: &Parts, text: &str) -> Result<(), ConfigError> {
        let Some((path_text, value_text)) = text.split_once('=') else {
            return Err(self.refused(JsonPath::of(Vec::new()), ConfigFault::NoValue));
        };
        let path = parse_path(path_text).map_err(|fault| self.refused(JsonPath::of(Vec::new()), fault))?;

        let schema = parts.schema();
        let place = path.iter().fold(Place::root(&schema), Place::step);
        let value = match place {
            Place::Schema(at) => Form::of(at).read(value_text),
            Place::Opaque => Json::read(value_text),
            Place::Unknown => Ok(Json::Leaf(Leaf::String(String::from(value_text)))),
        };

        self.push(path, value)
    }

    /// Adds the entry `value` sets at `path`, refusing a value its form did not read and one nested too deeply.
    fn push(&mut self, path: Vec<Step>, value: Result<Json, ConfigFault>) -> Result<(), ConfigError> {
        let value = match value {
            Ok(value) => value,
            Err(fault) => return Err(self.refused(path_of(&path), fault)),
        };
        let deep = value.too_deep_at(&path);
        if deep {
            return Err(self.refused(path_of(&path), ConfigFault::TooDeep));
        }

        self.entries.push(Entry { path, value });
        Ok(())
    }

    /// The refusal of what this layer set at `path`.
    pub(super) fn refused(&self, path: JsonPath, fault: ConfigFault) -> ConfigError {
        ConfigError::Layer { layer: self.layer.clone(), path, fault }
    }
}

/// The steps of a path as `--set` writes it: names joined by `.`, each followed by any indices, `windows[0].size`.
fn parse_path(text: &str) -> Result<Vec<Step>, ConfigFault> {
    let mut steps = Vec::new();
    for segment in text.split('.') {
        let opened = segment.split_once('[');
        let (name, indices) = match opened {
            Some((name, rest)) => (name, rest.strip_suffix(']').ok_or(ConfigFault::NotAPath)?.split("][").collect()),
            None => (segment, Vec::new()),
        };
        let named = !name.is_empty();
        let clean = !name.contains(']');
        let valid = named && clean;
        if !valid {
            return Err(ConfigFault::NotAPath);
        }
        steps.push(Step::Field(String::from(name)));

        for digits in indices {
            let decimal = !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit());
            if !decimal {
                return Err(ConfigFault::NotAPath);
            }
            steps.push(Step::Index(digits.parse().map_err(|_| ConfigFault::NotAPath)?));
        }
    }

    Ok(steps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn paths_are_names_and_indices() {
        // Catches a path reader that drops an index, reads a name past its bracket, or takes a malformed path.
        let field = |name: &str| Step::Field(String::from(name));
        assert_eq!(parse_path("engine.workers"), Ok(vec![field("engine"), field("workers")]));
        assert_eq!(
            parse_path("windows[0][2].size"),
            Ok(vec![field("windows"), Step::Index(0), Step::Index(2), field("size")])
        );
        for bad in ["", "a..b", "a[", "a[]", "a[x]", "a]", "a[0]b", "[0]"] {
            assert_eq!(parse_path(bad), Err(ConfigFault::NotAPath), "{bad}");
        }
    }
}
