//! The composite: the sections a program is put together from, its schema, and the one decode of its layers
//! (ADR-0028).

use super::document::{Document, Json, path_of};
use super::place::Place;
use super::{ConfigError, ConfigFault, Declared, Overlay, RepeatedSection, Settings};
use crate::{
    Bare, Budget, DecodeError, Encode, EncodeError, JsonError, JsonPath, JsonStep, KindName, Reader, Schema,
    SchemaNodes, Shape, Writer, from_json, to_json,
};
use alloc::boxed::Box;
use alloc::format;
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
    /// The type the section is read as, by which a handle and its reader find it.
    type_id: TypeId,
    /// The section's shape.
    shape: Shape,
    /// The section's named fields as its settings kind declares them.
    fields: &'static [Declared],
    /// The bytes of the section's defaults, or why they could not be written.
    defaults: Result<Vec<u8>, EncodeError>,
    /// Reads the section's bytes as its type.
    decode: DecodeSection,
}

/// The sections a program's configuration is put together from: the host's, and each root's that has settings
/// (ADR-0028).
///
/// Its schema is the sections' schemas under their names, so it says exactly what this program, on this host, can
/// be configured with.
///
/// ```
/// use aethereal_data::config::Parts;
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
/// let mut configuration = parts.configure(&[], &mut Budget::new(1 << 20))?;
/// let show = configuration.take::<Show>("show")?;
/// assert_eq!(show.seconds, 0);
/// assert!(configuration.take::<Show>("show").is_err());
/// # Ok::<(), Box<dyn core::error::Error>>(())
/// ```
pub struct Parts {
    sections: Vec<Part>,
}

impl Parts {
    /// A composite of no sections.
    #[must_use]
    pub const fn new() -> Self {
        Self { sections: Vec::new() }
    }

    /// The composite with the section `name`, read as `S`, its defaults `S`'s `Default`, added after those already in
    /// it: the name is the one the root that reads it gives (ADR-0029).
    ///
    /// # Errors
    ///
    /// [`RepeatedSection`] when a section of that name is already in it, naming both kinds.
    pub fn with<S: Settings + Default>(self, name: &'static str) -> Result<Self, RepeatedSection> {
        self.with_defaults(name, &S::default())
    }

    /// The composite with the section `name`, read as `S`, its defaults `defaults`, added after those already in it:
    /// beneath every layer, what the section holds where no layer sets it (ADR-0028).
    ///
    /// # Errors
    ///
    /// [`RepeatedSection`] when a section of that name is already in it, naming both kinds.
    pub fn with_defaults<S: Settings>(mut self, name: &'static str, defaults: &S) -> Result<Self, RepeatedSection> {
        let found = self.sections.iter().find(|part| part.name == name);
        if let Some(first) = found {
            return Err(RepeatedSection { name, first: first.kind.clone(), second: S::NAME });
        }

        self.sections.push(Part {
            name,
            kind: S::NAME,
            type_id: TypeId::of::<S>(),
            shape: S::SHAPE,
            fields: S::FIELDS,
            defaults: encoded(defaults),
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

    /// The composite described as JSON, `{"schema": …, "defaults": …}`: its schema as the flat nodes ADR-0003 gives
    /// every schema, and the document of every section's defaults, each in the form ADR-0007 gives every kind. What
    /// the walks hold is charged to `budget`.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Layer`] when a section's defaults could not be written, naming the layer `defaults`.
    pub fn describe(&self, budget: &mut Budget) -> Result<String, ConfigError> {
        let mut out = Writer::<Bare>::new(());
        let at_root = |error: JsonError| refused(DEFAULTS, &error);
        self.schema().encode(&mut out).map_err(JsonError::from).map_err(at_root)?;
        let schema = to_json(&SchemaNodes::of(&SchemaNodes::SHAPE), &out.into_bytes(), budget).map_err(at_root)?;
        let defaults = self.defaults(0, budget)?.text();

        Ok(format!("{{\"schema\":{schema},\"defaults\":{defaults}}}"))
    }

    /// Each section's name and its named fields, in the order they were added.
    pub(super) fn declared(&self) -> impl Iterator<Item = (&'static str, &'static [Declared])> + '_ {
        self.sections.iter().map(|part| (part.name, part.fields))
    }

    /// The names of the sections read as `S`, in the order they were added.
    pub(super) fn read_as<S: Settings>(&self) -> Vec<&'static str> {
        let type_id = TypeId::of::<S>();
        self.sections.iter().filter(|part| part.type_id == type_id).map(|part| part.name).collect()
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

    /// The document of every section's defaults, each value set by the layer at `layer`.
    fn defaults(&self, layer: usize, budget: &mut Budget) -> Result<Document, ConfigError> {
        let mut members = Vec::new();
        for part in &self.sections {
            let at_section =
                |error: JsonError| refused(DEFAULTS, &error.within(JsonStep::Field(String::from(part.name))));
            let bytes = part.defaults.as_ref().map_err(|error| at_section(JsonError::from(*error)))?;
            let schema = SchemaNodes::of(&part.shape);
            let text = to_json(&schema, bytes, budget).map_err(at_section)?;
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
            sections.push(Decoded { name: part.name, type_id: part.type_id, value });
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

/// The bytes of `value`.
fn encoded<S: Settings>(value: &S) -> Result<Vec<u8>, EncodeError> {
    let mut out = Writer::<Bare>::new(());
    value.encode(&mut out)?;
    Ok(out.into_bytes())
}

/// Reads one `S` from `reader`.
fn decode_of<S: Settings>(reader: &mut Reader<'_, Bare>) -> Result<Box<dyn Any>, DecodeError> {
    let value = S::decode(reader)?;
    Ok(Box::new(value))
}

/// One section's value, its type erased.
struct Decoded {
    /// The section's name.
    name: &'static str,
    /// The type it was read as.
    type_id: TypeId,
    /// Its value, of that type.
    value: Box<dyn Any>,
}

impl Decoded {
    /// Whether this is the section `name`, read as the type of `type_id`.
    fn is(&self, name: &str, type_id: TypeId) -> bool {
        let same_name = self.name == name;
        let same_type = self.type_id == type_id;
        same_name && same_type
    }
}

/// A program's configuration, set up: each section's value, until the root that reads it takes it (ADR-0028,
/// ADR-0029).
pub struct Configuration {
    sections: Vec<Decoded>,
}

impl Configuration {
    /// The section `name`, read as `S`, taken out: the bootstrap of the root that gave the name is its one reader
    /// (ADR-0029).
    ///
    /// # Errors
    ///
    /// [`ConfigError::Untaken`] when no section of that name, read as `S`, is left: none was composed, or it was taken
    /// already.
    pub fn take<S: Settings>(&mut self, name: &'static str) -> Result<S, ConfigError> {
        let type_id = TypeId::of::<S>();
        let untaken = || ConfigError::Untaken { section: name, kind: S::NAME };
        let position = self.sections.iter().position(|decoded| decoded.is(name, type_id)).ok_or_else(untaken)?;
        let value = self.sections.swap_remove(position).value.downcast::<S>().map_err(|_| untaken())?;

        Ok(*value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::JsonFault;
    use crate::config::samples::{EngineSettings, PoolSettings, Window, WindowsSettings};
    use alloc::string::ToString;
    use core::error::Error;

    type Outcome = Result<(), Box<dyn Error>>;

    /// The engine's and the windows' sections.
    fn parts() -> Result<Parts, RepeatedSection> {
        Parts::new().with::<EngineSettings>("engine")?.with::<WindowsSettings>("windows")
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

        let mut configuration = parts.configure(&[command_line, file], &mut budget)?;

        let engine = configuration.take::<EngineSettings>("engine")?;
        assert_eq!(engine, EngineSettings { workers: 4, mail_bytes: 7, fused: true });
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

        let mut configuration = parts.configure(&[command_line, file], &mut budget)?;

        let first = Window { title: String::from("a"), size: [1, 2] };
        let second = Window { title: String::from("b"), size: [10, 20] };
        let windows = configuration.take::<WindowsSettings>("windows")?;
        assert_eq!(windows, WindowsSettings(Vec::from([first, second])));
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
        let engine = Parts::new().with::<EngineSettings>("engine")?;

        let refused = engine.with::<PoolSettings>("engine").err();

        let first = <EngineSettings as crate::Kind>::NAME;
        let second = <PoolSettings as crate::Kind>::NAME;
        assert_eq!(refused, Some(RepeatedSection { name: "engine", first, second }));
        Ok(())
    }

    #[test]
    fn a_section_is_taken_once_by_its_name_and_type() -> Outcome {
        // Catches a take that finds a section by its type alone, handing one root another's section of the same
        // kind, or by its name alone, and one that leaves the section to be taken again.
        let parts = parts()?.with::<EngineSettings>("spare")?;
        let file = overlay(&parts, "file", &["spare.workers=9"])?;
        let mut budget = Budget::new(1 << 20);
        let mut configuration = parts.configure(&[file], &mut budget)?;

        let spare = configuration.take::<EngineSettings>("spare")?;
        let again = configuration.take::<EngineSettings>("spare").err();
        let misread = configuration.take::<PoolSettings>("engine").err();
        let engine = configuration.take::<EngineSettings>("engine")?;

        assert_eq!(spare.workers, 9);
        assert_eq!(engine.workers, 1);
        let spare_kind = <EngineSettings as crate::Kind>::NAME;
        assert_eq!(again, Some(ConfigError::Untaken { section: "spare", kind: spare_kind }));
        let pool_kind = <PoolSettings as crate::Kind>::NAME;
        assert_eq!(misread, Some(ConfigError::Untaken { section: "engine", kind: pool_kind }));
        Ok(())
    }
}
