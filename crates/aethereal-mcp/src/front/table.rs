//! The tools the root holds (ADR-0037).

use super::reply;
use crate::{Offered, ToolSheet, Tooling};
use aethereal_actor::{ErasedActorRef, ProtocolRef};
use aethereal_data::{Bound, Budget, SchemaNodes, is_object, json_schema};
use std::collections::{BTreeMap, BTreeSet};

/// The most bytes of JSON Schema one schema may write.
const SCHEMA_BYTES: usize = 16 * 1024 * 1024;
/// The longest name MCP asks of a tool.
const MOST_NAME: usize = 128;

/// A tool held.
#[derive(Bound)]
pub struct Tool {
    pub worker: ProtocolRef<Tooling>,
    pub input: SchemaNodes,
    pub output: SchemaNodes,
    /// Its entry in `tools/list`, written when it was offered.
    pub listed: String,
}

/// The tools held, by name, which is the order `tools/list` gives them in.
#[derive(Bound)]
pub struct Table {
    tools: BTreeMap<String, Tool>,
}

impl Table {
    /// No tools.
    pub const fn new() -> Self {
        Self { tools: BTreeMap::new() }
    }

    /// The tool named `name`: none when it is not held.
    pub fn tool(&self, name: &str) -> Option<&Tool> {
        self.tools.get(name)
    }

    /// Each tool's entry in `tools/list`, in order.
    pub fn listed(&self) -> impl Iterator<Item = &str> {
        self.tools.values().map(|tool| tool.listed.as_str())
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
        }
        let is_full = self.tools.len().saturating_add(sheets.len()) > room;
        if is_full {
            return Offered::Full;
        }
        let mut written = Vec::with_capacity(sheets.len());
        for sheet in sheets {
            let Some(tool) = written_tool(&sheet, worker) else {
                return Offered::Unwritable;
            };
            written.push((sheet.name, tool));
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

/// The tool `sheet` offers: none when one of its schemas has no JSON Schema.
fn written_tool(sheet: &ToolSheet, worker: ProtocolRef<Tooling>) -> Option<Tool> {
    let input = json_schema(&sheet.input, &mut Budget::new(SCHEMA_BYTES)).ok()?;
    let output = json_schema(&sheet.output, &mut Budget::new(SCHEMA_BYTES)).ok()?;
    let listed = reply::listed(&sheet.name, &sheet.description, &input, &output);

    Some(Tool { worker, input: sheet.input.clone(), output: sheet.output.clone(), listed })
}
