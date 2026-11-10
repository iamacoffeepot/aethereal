//! The tools a holder holds, and what it checks of each as it is offered (ADR-0037, ADR-0038).

use crate::{CommandLine, Offered, Output, ToolFlag, ToolSheet, Tooling};
use aethereal_actor::{ErasedActorRef, ProtocolRef};
use aethereal_data::{Bound, Budget, SchemaNodes, is_object, json_schema};
use std::collections::{BTreeMap, BTreeSet};

/// The most bytes of JSON Schema one schema may write.
const SCHEMA_BYTES: usize = 16 * 1024 * 1024;
/// The longest name MCP asks of a tool.
const MOST_NAME: usize = 128;

/// A tool held.
#[derive(Bound)]
pub struct HeldTool {
    pub worker: ProtocolRef<Tooling>,
    pub description: String,
    pub input: SchemaNodes,
    /// The flags its command line is typed with, checked against `input` when it was offered (ADR-0053).
    pub flags: Vec<ToolFlag>,
    pub output: HeldOutput,
    /// `input` as JSON Schema, written when it was offered.
    pub input_schema: String,
}

/// What a held tool answers with, as its sheet says, with what is listed of it (ADR-0043).
#[derive(Bound)]
pub enum HeldOutput {
    /// A value of `schema`, which is listed as `json_schema`, written when it was offered.
    Value { schema: SchemaNodes, json_schema: String },
    /// An image, which has no schema to list.
    Image,
    /// A value of the schema or an image: no schema is listed, as an image has none.
    ValueOrImage(SchemaNodes),
}

impl HeldOutput {
    /// What the tool's sheet says it answers with.
    #[must_use]
    pub fn sheet(&self) -> Output {
        match self {
            Self::Value { schema, .. } => Output::Value(schema.clone()),
            Self::Image => Output::Image,
            Self::ValueOrImage(schema) => Output::ValueOrImage(schema.clone()),
        }
    }
}

/// The tools held, by name, in the order of their names.
#[derive(Bound, Default)]
pub struct Table {
    tools: BTreeMap<String, HeldTool>,
}

impl Table {
    /// No tools.
    #[must_use]
    pub const fn new() -> Self {
        Self { tools: BTreeMap::new() }
    }

    /// The tool named `name`: none when it is not held.
    #[must_use]
    pub fn tool(&self, name: &str) -> Option<&HeldTool> {
        self.tools.get(name)
    }

    /// Each tool held, with its name, in order.
    pub fn tools(&self) -> impl Iterator<Item = (&str, &HeldTool)> {
        self.tools.iter().map(|(name, held)| (name.as_str(), held))
    }

    /// Each tool held as its worker offered it, in the order of their names.
    #[must_use]
    pub fn sheets(&self) -> Vec<ToolSheet> {
        self.tools
            .iter()
            .map(|(name, held)| ToolSheet {
                name: name.clone(),
                description: held.description.clone(),
                input: held.input.clone(),
                flags: held.flags.clone(),
                output: held.output.sheet(),
            })
            .collect()
    }

    /// How many tools are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Whether no tool is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Holds all of `sheets` for `worker`, or none, with `room` the most tools to hold, and says which.
    pub fn take(&mut self, sheets: Vec<ToolSheet>, worker: ProtocolRef<Tooling>, room: usize) -> Offered {
        let mut names = BTreeSet::new();
        for sheet in &sheets {
            let is_fresh = names.insert(sheet.name.as_str());
            let is_held = self.tools.contains_key(&sheet.name);
            if !is_fresh || is_held {
                return Offered::Taken;
            }
            if !is_well_named(&sheet.name) {
                return Offered::BadName;
            }
            if !is_object(&sheet.input) {
                return Offered::NotAnObject;
            }
            let fits = CommandLine::of(sheet).is_ok();
            if !fits {
                return Offered::BadFlags;
            }
        }
        let is_full = self.tools.len().saturating_add(sheets.len()) > room;
        if is_full {
            return Offered::Full;
        }
        let mut written = Vec::with_capacity(sheets.len());
        for sheet in sheets {
            let Some(tool) = written_tool(sheet, worker) else {
                return Offered::Unwritable;
            };
            written.push(tool);
        }
        self.tools.extend(written);

        Offered::Yes
    }

    /// Stops holding the tool named `name`, when `worker` holds it.
    pub fn withdraw(&mut self, name: &str, worker: ProtocolRef<Tooling>) {
        let held = self.tools.get(name).is_some_and(|tool| tool.worker.id() == worker.id());
        if held {
            self.tools.remove(name);
        }
    }

    /// Stops holding every tool of a worker that has ended.
    pub fn ended(&mut self, worker: ErasedActorRef) {
        let gone = worker.id();
        self.tools.retain(|_, tool| tool.worker.id() != gone);
    }
}

/// Whether `name` is what MCP asks of a tool's name.
fn is_well_named(name: &str) -> bool {
    let is_sized = !name.is_empty() && name.len() <= MOST_NAME;
    let is_plain = name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'));

    is_sized && is_plain
}

/// The tool `sheet` offers: none when a schema it lists has no JSON Schema.
fn written_tool(sheet: ToolSheet, worker: ProtocolRef<Tooling>) -> Option<(String, HeldTool)> {
    let input_schema = json_schema(&sheet.input, &mut Budget::new(SCHEMA_BYTES)).ok()?;
    let ToolSheet { name, description, input, flags, output } = sheet;
    let output = match output {
        Output::Value(schema) => {
            let json_schema = json_schema(&schema, &mut Budget::new(SCHEMA_BYTES)).ok()?;
            HeldOutput::Value { schema, json_schema }
        }
        Output::Image => HeldOutput::Image,
        Output::ValueOrImage(schema) => HeldOutput::ValueOrImage(schema),
    };

    Some((name, HeldTool { worker, description, input, flags, output, input_schema }))
}
