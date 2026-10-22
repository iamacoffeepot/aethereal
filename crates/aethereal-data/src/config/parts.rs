//! The composite: the sections a program is put together from, its schema, and the one decode of its layers
//! (ADR-0028).

use super::document::{Document, Json, path_of};
use super::place::Place;
use super::{ConfigError, ConfigFault, Declared, Overlay, RepeatedSection, Section};
use crate::{
    Bare, Budget, DecodeError, EncodeError, JsonError, JsonPath, JsonStep, KindName, Reader, SchemaNodes, Shape,
    Writer, from_json, to_json,
};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::any::{Any, TypeId};

/// The name of the layer the sections' defaults are, beneath every other.
const DEFAULTS: &str = "defaults";

/// The name of the composite's struct in its schema.
const COMPOSITE: &str = "configuration";

/// Reads one section's bytes as its type, the value's type erased.
type DecodeSection = fn(&mut Reader<'_, Bare>) -> Result<Box<dyn Any>, DecodeError>;

/// One section of the composite, its type erased.
struct Part {
    /// The section's name.
    name: &'static str,
    /// The section's kind's name.
    kind: KindName,
    /// The section's type, by which a handle and a decoded value find it.
    type_id: TypeId,
    /// The section's shape.
    shape: Shape,
    /// The section's named fields as its part declares them.
    fields: &'static [Declared],
    /// The bytes of the section's `Default`.
    defaults: fn() -> Result<Vec<u8>, EncodeError>,
    /// Reads the section's bytes as its type.
    decode: DecodeSection,
}

/// The sections a program's configuration is put together from: each part's, and the program's own (ADR-0028).
///
/// Its schema is the sections' schemas under their names, so it says exactly what this program, on this host, can
/// be configured with.
pub struct Parts {
    sections: Vec<Part>,
}

impl Parts {
    /// A composite of no sections.
    #[must_use]
    pub const fn new() -> Self {
        Self { sections: Vec::new() }
    }

    /// The composite with the section `S` added after those already in it, refusing a second section of one name.
    pub fn with<S: Section>(mut self) -> Result<Self, RepeatedSection> {
        let found = self.sections.iter().find(|part| part.name == S::SECTION);
        if let Some(first) = found {
            return Err(RepeatedSection { name: S::SECTION, first: first.kind.clone(), second: S::NAME });
        }

        self.sections.push(Part {
            name: S::SECTION,
            kind: S::NAME,
            type_id: TypeId::of::<S>(),
            shape: S::SHAPE,
            fields: S::FIELDS,
            defaults: defaults_of::<S>,
            decode: decode_of::<S>,
        });
        Ok(self)
    }

    /// The composite's schema: a struct with a field for each section, named for it, holding its kind's schema.
    #[must_use]
    pub fn schema(&self) -> SchemaNodes {
        let fields: Vec<(&str, Shape)> = self.sections.iter().map(|part| (part.name, part.shape)).collect();
        SchemaNodes::of_fields(COMPOSITE, &fields)
    }

    /// Each section's name and its named fields, in the order they were added.
    pub(super) fn declared(&self) -> impl Iterator<Item = (&'static str, &'static [Declared])> + '_ {
        self.sections.iter().map(|part| (part.name, part.fields))
    }

    /// Whether the section named `name` is of the type `S`.
    pub(super) fn holds<S: Section>(&self, name: &str) -> bool {
        let type_id = TypeId::of::<S>();
        self.sections.iter().any(|part| part.name == name && part.type_id == type_id)
    }

    /// The configuration `layers` set over the sections' defaults, the first winning, merged field by field and
    /// decoded once with the composite's schema; what the walks hold is charged to `budget`.
    ///
    /// A field that does not exist and a value of the wrong shape are refused, naming the field's path and the
    /// layer that set it.
    pub fn configure(&self, layers: &[Overlay], budget: &mut Budget) -> Result<Configuration, ConfigError> {
        let schema = self.schema();
        let mut document = self.defaults(layers.len(), budget)?;

        for (layer, overlay) in layers.iter().enumerate().rev() {
            for entry in overlay.entries() {
                let root = Place::root(&schema);
                document.set(&entry.path, entry.value.clone(), layer, root).map_err(|unplaced| {
                    let steps = entry.path.get(..unplaced.steps).unwrap_or_default();
                    overlay.refused(path_of(steps), unplaced.fault)
                })?;
            }
        }

        let bytes = from_json(&schema, &document.text(), budget).map_err(|error| {
            let layer = document.layer_at(error.path());
            let name = layers.get(layer).map_or(DEFAULTS, Overlay::layer);
            refused(name, &error)
        })?;

        self.decode(&bytes, budget)
    }

    /// The document of every section's `Default`, each value set by the layer at `layer`.
    fn defaults(&self, layer: usize, budget: &mut Budget) -> Result<Document, ConfigError> {
        let mut members = Vec::new();
        for part in &self.sections {
            let at_section =
                |error: JsonError| refused(DEFAULTS, &error.within(JsonStep::Field(String::from(part.name))));
            let bytes = (part.defaults)().map_err(JsonError::from).map_err(at_section)?;
            let schema = SchemaNodes::of(&part.shape);
            let text = to_json(&schema, &bytes, budget).map_err(at_section)?;
            let json = Json::read(&text).map_err(|fault| {
                let path = JsonPath::of(Vec::from([JsonStep::Field(String::from(part.name))]));
                ConfigError::Layer { layer: String::from(DEFAULTS), path, fault }
            })?;
            members.push((String::from(part.name), json));
        }

        Ok(Document::new(Json::Object(members), layer))
    }

    /// Each section's value, read from the composite's bytes as its type, in order.
    fn decode(&self, bytes: &[u8], budget: &mut Budget) -> Result<Configuration, ConfigError> {
        let mut reader = Reader::<Bare>::new(bytes, budget, ());
        let mut sections = Vec::new();
        for part in &self.sections {
            let value =
                (part.decode)(&mut reader).map_err(|error| ConfigError::Section { section: part.name, error })?;
            sections.push(Decoded { type_id: part.type_id, value });
        }

        let trailing = reader.input().len();
        let clean = trailing == 0;
        let last = self.sections.last();
        match last {
            Some(part) if !clean => {
                Err(ConfigError::Section { section: part.name, error: DecodeError::TrailingBytes { count: trailing } })
            }
            Some(_) | None => Ok(Configuration { sections }),
        }
    }
}

impl Default for Parts {
    fn default() -> Self {
        Self::new()
    }
}

/// The refusal of `error`, at its path, as what the layer `layer` set.
fn refused(layer: &str, error: &JsonError) -> ConfigError {
    ConfigError::Layer {
        layer: String::from(layer),
        path: error.path().clone(),
        fault: ConfigFault::Json(error.fault()),
    }
}

/// The bytes of `S`'s `Default`.
fn defaults_of<S: Section>() -> Result<Vec<u8>, EncodeError> {
    let mut out = Writer::<Bare>::new(());
    S::default().encode(&mut out)?;
    Ok(out.into_bytes())
}

/// Reads one `S` from `reader`.
fn decode_of<S: Section>(reader: &mut Reader<'_, Bare>) -> Result<Box<dyn Any>, DecodeError> {
    let value = S::decode(reader)?;
    Ok(Box::new(value))
}

/// One section's value, its type erased.
struct Decoded {
    type_id: TypeId,
    value: Box<dyn Any>,
}

/// A program's configuration, set up: each section's value (ADR-0028).
pub struct Configuration {
    sections: Vec<Decoded>,
}

impl Configuration {
    /// The value of the section `S`, or nothing when the composite has no such section.
    #[must_use]
    pub fn get<S: Section>(&self) -> Option<&S> {
        let type_id = TypeId::of::<S>();
        let found = self.sections.iter().find(|decoded| decoded.type_id == type_id)?;
        found.value.downcast_ref::<S>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::JsonFault;
    use crate::config::samples::{EngineConfig, OtherEngine, Window, WindowsConfig};
    use alloc::string::ToString;
    use core::error::Error;

    type Outcome = Result<(), Box<dyn Error>>;

    /// The engine's and the windows' sections.
    fn parts() -> Result<Parts, RepeatedSection> {
        Parts::new().with::<EngineConfig>()?.with::<WindowsConfig>()
    }

    /// An overlay of the layer `layer`, setting each `--set` text in order.
    fn overlay(parts: &Parts, layer: &str, sets: &[&str]) -> Result<Overlay, ConfigError> {
        let mut overlay = Overlay::new(layer);
        for text in sets {
            overlay.set(parts, text)?;
        }
        Ok(overlay)
    }

    /// The layer, the path as text, and the fault of a refusal by a layer.
    fn refusal(result: Result<Configuration, ConfigError>) -> Option<(String, String, ConfigFault)> {
        let Err(ConfigError::Layer { layer, path, fault }) = result else {
            return None;
        };
        Some((layer, path.to_string(), fault))
    }

    #[test]
    fn layers_merge_field_by_field_the_first_winning() -> Outcome {
        // Catches a merge that lets a later layer win, replaces a structure a layer set by some of its fields, or
        // drops what a lower layer set beside the field a higher one set.
        let parts = parts()?;
        let command_line = overlay(&parts, "command line", &["engine.workers=4"])?;
        let file = overlay(&parts, "file", &["engine={\"workers\":2,\"mail_bytes\":\"7\"}"])?;
        let mut budget = Budget::new(1 << 20);

        let configuration = parts.configure(&[command_line, file], &mut budget)?;

        let engine = configuration.get::<EngineConfig>();
        assert_eq!(engine, Some(&EngineConfig { workers: 4, mail_bytes: 7, fused: true }));
        Ok(())
    }

    #[test]
    fn a_sequence_is_set_whole_or_one_element_by_index() -> Outcome {
        // Catches a sequence merged element by element when set whole, and an index that sets the wrong element or
        // replaces the element's other fields.
        let parts = parts()?;
        let whole = "windows=[{\"title\":\"a\",\"size\":[1,2]},{\"title\":\"b\",\"size\":[3,4]}]";
        let command_line = overlay(&parts, "command line", &["windows[1].size=10x20"])?;
        let file = overlay(&parts, "file", &[whole])?;
        let mut budget = Budget::new(1 << 20);

        let configuration = parts.configure(&[command_line, file], &mut budget)?;

        let first = Window { title: String::from("a"), size: [1, 2] };
        let second = Window { title: String::from("b"), size: [10, 20] };
        let windows = configuration.get::<WindowsConfig>();
        assert_eq!(windows, Some(&WindowsConfig(Vec::from([first, second]))));
        Ok(())
    }

    #[test]
    fn an_index_past_the_end_names_its_path_and_layer() -> Outcome {
        // Catches an index past a sequence's end that grows it, or is refused without its place.
        let parts = parts()?;
        let command_line = overlay(&parts, "command line", &["windows[3].size=1x2"])?;
        let mut budget = Budget::new(1 << 20);

        let refused = refusal(parts.configure(&[command_line], &mut budget));

        let expected = (String::from("command line"), String::from("windows[3]"), ConfigFault::NoElement { length: 1 });
        assert_eq!(refused, Some(expected));
        Ok(())
    }

    #[test]
    fn an_unknown_field_names_its_path_and_layer() -> Outcome {
        // Catches an unknown field that is dropped, or blamed on the layer above or the defaults beneath.
        let parts = parts()?;
        let command_line = overlay(&parts, "command line", &["engine.workers=4"])?;
        let file = overlay(&parts, "file", &["engine.bogus=1"])?;
        let page = overlay(&parts, "page", &["engine={\"nonsense\":true}"])?;
        let mut budget = Budget::new(1 << 20);

        let from_file = refusal(parts.configure(&[command_line.clone(), file], &mut budget));
        let from_page = refusal(parts.configure(&[command_line, page], &mut budget));

        let unknown = ConfigFault::Json(JsonFault::UnknownField);
        assert_eq!(from_file, Some((String::from("file"), String::from("engine.bogus"), unknown)));
        assert_eq!(from_page, Some((String::from("page"), String::from("engine.nonsense"), unknown)));
        Ok(())
    }

    #[test]
    fn a_wrong_value_names_its_path_and_layer() -> Outcome {
        // Catches a refusal the one decode makes blamed on a layer other than the one that set the value.
        let parts = parts()?;
        let environment = overlay(&parts, "environment", &["engine.workers=-1"])?;
        let file = overlay(&parts, "file", &["windows[0]={\"title\":3,\"size\":[1,2]}"])?;
        let mut budget = Budget::new(1 << 20);

        let out_of_range = refusal(parts.configure(&[environment], &mut budget));
        let wrong_shape = refusal(parts.configure(&[Overlay::new("command line"), file], &mut budget));

        let range = ConfigFault::Json(JsonFault::OutOfRange);
        let string = ConfigFault::Json(JsonFault::Expected(crate::JsonForm::String));
        assert_eq!(out_of_range, Some((String::from("environment"), String::from("engine.workers"), range)));
        assert_eq!(wrong_shape, Some((String::from("file"), String::from("windows[0].title"), string)));
        Ok(())
    }

    #[test]
    fn two_sections_of_one_name_are_refused_naming_both() -> Outcome {
        // Catches a composite that lets the second section of a name replace or shadow the first.
        let engine = Parts::new().with::<EngineConfig>()?;

        let refused = engine.with::<OtherEngine>().err();

        let first = <EngineConfig as crate::Kind>::NAME;
        let second = <OtherEngine as crate::Kind>::NAME;
        assert_eq!(refused, Some(RepeatedSection { name: "engine", first, second }));
        Ok(())
    }
}
