//! The one document the layers are merged into, each value marked with the layer that set it (ADR-0028).

use super::ConfigFault;
use super::place::Place;
use crate::json::escape;
use crate::json::layout::DEPTH_LIMIT;
use crate::json::reader;
use crate::json::tree::{Tree, Value};
use crate::{Budget, JsonPath, JsonStep};
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// The deepest a document nests: a schema's depth, below the configuration's root.
const DOCUMENT_DEPTH: usize = DEPTH_LIMIT as usize + 1;

/// One step of a path a layer sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Step {
    /// A member of an object: a field, a section, or a map's key.
    Field(String),
    /// An element of a sequence, by its index.
    Index(usize),
}

impl Step {
    /// The step as a path into a JSON value names it.
    fn json(&self) -> JsonStep {
        match self {
            Self::Field(name) => JsonStep::Field(name.clone()),
            Self::Index(index) => JsonStep::Index(*index),
        }
    }
}

/// The JSON path of `steps`.
pub(super) fn path_of(steps: &[Step]) -> JsonPath {
    JsonPath::of(steps.iter().map(Step::json).collect())
}

/// A JSON value that holds no other: what ends a branch of a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Leaf {
    /// `null`.
    Null,
    /// `true` or `false`.
    Boolean(bool),
    /// A number, as its text reads.
    Number(String),
    /// A string, escapes resolved.
    String(String),
}

impl Leaf {
    /// Appends the leaf as JSON text.
    fn write(&self, out: &mut String) {
        match self {
            Self::Null => out.push_str("null"),
            Self::Boolean(true) => out.push_str("true"),
            Self::Boolean(false) => out.push_str("false"),
            Self::Number(text) => out.push_str(text),
            Self::String(text) => escape::write(text, out),
        }
    }
}

/// A JSON value a layer sets, owned, nesting at most a document's depth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Json {
    /// A value that holds no other.
    Leaf(Leaf),
    /// An array's elements.
    Array(Vec<Self>),
    /// An object's members, each name once.
    Object(Vec<(String, Self)>),
}

impl Json {
    /// The value JSON text states, refusing text that is not JSON and a value nested deeper than a document.
    ///
    /// The text is in hand, so what reading it holds grows with its length and needs no budget of its own.
    pub(super) fn read(text: &str) -> Result<Self, ConfigFault> {
        let mut budget = Budget::new(usize::MAX);
        let tree = reader::read(text, &mut budget).map_err(|error| ConfigFault::Json(error.fault()))?;
        Self::of(&tree, tree.root(), 0)
    }

    /// The owned form of `value` in `tree`, `level` containers below the value read.
    ///
    /// It recurses once per container, and refuses past a document's depth, so the depth is capped.
    fn of(tree: &Tree<'_>, value: &Value<'_>, level: usize) -> Result<Self, ConfigFault> {
        let deep = level > DOCUMENT_DEPTH;
        if deep {
            return Err(ConfigFault::TooDeep);
        }

        let json = match value {
            Value::Null => Self::Leaf(Leaf::Null),
            Value::Boolean(bit) => Self::Leaf(Leaf::Boolean(*bit)),
            Value::Number(text) => Self::Leaf(Leaf::Number(String::from(*text))),
            Value::String(text) => Self::Leaf(Leaf::String(text.clone())),
            Value::Array(span) => {
                let mut elements = Vec::new();
                for element in tree.elements(*span) {
                    elements.push(Self::of(tree, element, level + 1)?);
                }
                Self::Array(elements)
            }
            Value::Object(span) => {
                let mut members = Vec::new();
                for member in tree.members(*span) {
                    let value = Self::of(tree, &member.value, level + 1)?;
                    members.push((member.name.clone(), value));
                }
                Self::Object(members)
            }
        };

        Ok(json)
    }

    /// How many containers nest in the value: none for a leaf.
    pub(super) fn depth(&self) -> usize {
        match self {
            Self::Leaf(_) => 0,
            Self::Array(elements) => 1 + elements.iter().map(Self::depth).max().unwrap_or(0),
            Self::Object(members) => 1 + members.iter().map(|(_, value)| value.depth()).max().unwrap_or(0),
        }
    }

    /// Whether a value this deep set at `steps` would nest deeper than a document.
    pub(super) fn too_deep_at(&self, steps: &[Step]) -> bool {
        let depth = steps.len().saturating_add(self.depth());
        depth > DOCUMENT_DEPTH
    }
}

/// What a node of the document holds.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Held {
    /// A value that holds no other.
    Leaf(Leaf),
    /// An array's elements.
    Array(Vec<Node>),
    /// An object's members by name.
    Object(BTreeMap<String, Node>),
}

/// One value of the document and the layer that set it, by its place in the layers, the first at `0`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Node {
    layer: usize,
    held: Held,
}

impl Node {
    /// `json`, every value in it set by `layer`.
    fn placed(json: Json, layer: usize) -> Self {
        let held = match json {
            Json::Leaf(leaf) => Held::Leaf(leaf),
            Json::Array(elements) => {
                Held::Array(elements.into_iter().map(|element| Self::placed(element, layer)).collect())
            }
            Json::Object(members) => {
                Held::Object(members.into_iter().map(|(name, value)| (name, Self::placed(value, layer))).collect())
            }
        };

        Self { layer, held }
    }

    /// Merges `json`, set by `layer`, over this value: an object a structure's fields are written in by its members,
    /// anything else whole.
    fn merge(&mut self, json: Json, layer: usize, place: Place<'_>) {
        let by_fields = place.by_fields();
        match (&mut self.held, json) {
            (Held::Object(members), Json::Object(given)) if by_fields => {
                for (name, value) in given {
                    let within = place.field(&name);
                    match members.get_mut(&name) {
                        Some(member) => member.merge(value, layer, within),
                        None => {
                            members.insert(name, Self::placed(value, layer));
                        }
                    }
                }
            }
            (_, whole) => {
                *self = Self::placed(whole, layer);
            }
        }
    }

    /// Appends the value as JSON text.
    fn write(&self, out: &mut String) {
        match &self.held {
            Held::Leaf(leaf) => leaf.write(out),
            Held::Array(elements) => {
                out.push('[');
                for (position, element) in elements.iter().enumerate() {
                    let later = position > 0;
                    if later {
                        out.push(',');
                    }
                    element.write(out);
                }
                out.push(']');
            }
            Held::Object(members) => {
                out.push('{');
                for (position, (name, member)) in members.iter().enumerate() {
                    let later = position > 0;
                    if later {
                        out.push(',');
                    }
                    escape::write(name, out);
                    out.push(':');
                    member.write(out);
                }
                out.push('}');
            }
        }
    }
}

/// A place a layer set that no document has, and why: how many steps of the path it took to find out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Unplaced {
    /// The steps up to and including the one that has no place.
    pub(super) steps: usize,
    /// Why it has none.
    pub(super) fault: ConfigFault,
}

/// The layers merged into one document.
pub(super) struct Document {
    root: Node,
}

impl Document {
    /// A document of `json`, every value set by `layer`.
    pub(super) fn new(json: Json, layer: usize) -> Self {
        Self { root: Node::placed(json, layer) }
    }

    /// Sets `json` at `steps`, by `layer`, over what is there: through an object by its member, made where none is,
    /// and through a sequence by an element it has.
    pub(super) fn set(&mut self, steps: &[Step], json: Json, layer: usize, root: Place<'_>) -> Result<(), Unplaced> {
        let mut node = &mut self.root;
        let mut place = root;
        for (position, step) in steps.iter().enumerate() {
            match step {
                Step::Field(name) => {
                    let is_object = matches!(node.held, Held::Object(_));
                    if !is_object {
                        *node = Node { layer, held: Held::Object(BTreeMap::new()) };
                    }
                    let Held::Object(members) = &mut node.held else {
                        return Err(Unplaced { steps: position + 1, fault: ConfigFault::NotASequence });
                    };
                    node = members.entry(name.clone()).or_insert(Node { layer, held: Held::Leaf(Leaf::Null) });
                    place = place.field(name);
                }
                Step::Index(index) => {
                    let Held::Array(elements) = &mut node.held else {
                        return Err(Unplaced { steps: position + 1, fault: ConfigFault::NotASequence });
                    };
                    let length = elements.len();
                    let Some(element) = elements.get_mut(*index) else {
                        return Err(Unplaced { steps: position + 1, fault: ConfigFault::NoElement { length } });
                    };
                    node = element;
                    place = place.index(*index);
                }
            }
        }

        node.merge(json, layer, place);
        Ok(())
    }

    /// The document as JSON text.
    pub(super) fn text(&self) -> String {
        let mut out = String::new();
        self.root.write(&mut out);
        out
    }

    /// The layer that set the deepest value `path` reaches: the value it names, or the nearest one holding it.
    pub(super) fn layer_at(&self, path: &JsonPath) -> usize {
        let mut node = &self.root;
        for step in path.steps() {
            let next = match (step, &node.held) {
                (JsonStep::Field(name) | JsonStep::Key(name) | JsonStep::Variant(name), Held::Object(members)) => {
                    members.get(name)
                }
                (JsonStep::Index(index), Held::Array(elements)) => elements.get(*index),
                _ => None,
            };
            let Some(next) = next else {
                break;
            };
            node = next;
        }

        node.layer
    }
}
