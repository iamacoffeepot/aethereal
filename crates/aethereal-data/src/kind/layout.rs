//! The layout hash over both forms of a schema (ADR-0006).

use super::graph::{Graph, Slot, kind_id};
use crate::schema::MOST_LISTED;
use crate::{
    Answered, Field, FieldNode, Fields, FieldsNode, Float, Fnv, Integer, KindId, KindPtr, ProtocolNode, ProtocolShape,
    Referent, SchemaNode, SchemaNodes, Shape, Variant,
};
use alloc::vec::Vec;

// The tag bytes of ADR-0006's table: the first byte each shape hashes.
const BOOLEAN: u8 = 0;
const INTEGER: u8 = 1;
const FLOAT: u8 = 2;
const STRING: u8 = 3;
const BYTES: u8 = 4;
const OPTION: u8 = 5;
const SEQUENCE: u8 = 6;
const ARRAY: u8 = 7;
const MAP: u8 = 8;
const STRUCT: u8 = 9;
const TUPLE: u8 = 10;
const ENUM: u8 = 11;
const JSON: u8 = 12;
const REFERENCE: u8 = 13;
/// The tag a cycle of kinds is hashed under, and each member's layout under it (ADR-0006).
pub(super) const CYCLE: u8 = 14;

// The byte after a reference's written shape: whether its protocol is a tuple of told kinds or a named protocol.
const TOLD: u8 = 0;
const NAMED: u8 = 1;

/// The tag byte of an integer width: `0` to `7` for `U8`, `U16`, `U32`, `U64`, `I8`, `I16`, `I32`, `I64`.
pub const fn integer_code(kind: Integer) -> u8 {
    match kind {
        Integer::U8 => 0,
        Integer::U16 => 1,
        Integer::U32 => 2,
        Integer::U64 => 3,
        Integer::I8 => 4,
        Integer::I16 => 5,
        Integer::I32 => 6,
        Integer::I64 => 7,
    }
}

/// The tag byte of a float width: `0` for `F32`, `1` for `F64`.
pub const fn float_code(kind: Float) -> u8 {
    match kind {
        Float::F32 => 0,
        Float::F64 => 1,
    }
}

/// The layout hash of a constant shape whose references name kinds by their ids, each computed from its own graph:
/// the layout of a shape that is no member of a cycle, such as a state's.
#[must_use]
pub const fn shape_layout(shape: &Shape) -> u64 {
    layout(&mut Walk::direct(), shape)
}

/// What a reference's kind is written as in a layout.
#[derive(Clone, Copy)]
enum Target {
    /// Its id: a kind outside the cycle being hashed, or any kind where no cycle is.
    Known(KindId),
    /// A member of the cycle being hashed: its key, its place in the graph while members are blank and its position
    /// once they are given positions.
    Inner(u32),
}

/// Where a layout being written finds what each reference's kinds are, and how it writes them.
pub(super) struct Walk<'g> {
    targets: Targets<'g>,
    mode: Mode,
}

/// Where a walk finds its references' kinds.
enum Targets<'g> {
    /// Each kind's id, computed from its own graph.
    Direct,
    /// The graph's edges of the kind being written, from `cursor` on, in the order the walk meets them.
    Graph { graph: &'g Graph, cursor: usize },
}

/// How a layout writes the kinds its references name (ADR-0006).
#[derive(Clone, Copy)]
pub(crate) enum Mode {
    /// No kind is a member of the layout's own cycle: each is its id, as for a kind in no cycle.
    Acyclic,
    /// A member of a cycle, before positions are given: a member it names is counted but not told apart.
    Blank,
    /// A member of a cycle, once positions are given: a member it names is its position.
    Positioned,
}

impl<'g> Walk<'g> {
    /// A walk that computes each kind's id from its own graph.
    const fn direct() -> Self {
        Self { targets: Targets::Direct, mode: Mode::Acyclic }
    }

    /// A walk over the edges of a kind of `graph`, from `cursor`, written as `mode` says.
    pub(super) const fn over(graph: &'g Graph, cursor: usize, mode: Mode) -> Self {
        Self { targets: Targets::Graph { graph, cursor }, mode }
    }

    /// What the next kind a reference names is written as.
    const fn next(&mut self, kind: KindPtr) -> Target {
        match &mut self.targets {
            Targets::Direct => Target::Known(kind_id(kind)),
            Targets::Graph { graph, cursor } => {
                let to = graph.edge(*cursor);
                *cursor += 1;
                match graph.slot(to) {
                    Slot::Known(id) => Target::Known(id),
                    Slot::Blank => Target::Inner(to as u32),
                    Slot::Member(position) => Target::Inner(position),
                    Slot::Unresolved => panic!("a kind's layout names a kind whose id is not yet computed"),
                }
            }
        }
    }
}

/// The layout hash of `shape`, its references' kinds found by `walk`. A `Shape` is a finite tree fixed by a type up to
/// its references, which the walk does not follow, so the recursion is bounded by the type.
pub(super) const fn layout(walk: &mut Walk<'_>, shape: &Shape) -> u64 {
    match *shape {
        Shape::Boolean => Fnv::new().byte(BOOLEAN).finish(),
        Shape::Integer(kind) => Fnv::new().byte(INTEGER).byte(integer_code(kind)).finish(),
        Shape::Float(kind) => Fnv::new().byte(FLOAT).byte(float_code(kind)).finish(),
        Shape::String => Fnv::new().byte(STRING).finish(),
        Shape::Bytes => Fnv::new().byte(BYTES).finish(),
        Shape::Json => Fnv::new().byte(JSON).finish(),
        Shape::Option(inner) => {
            let inner = layout(walk, inner);
            Fnv::new().byte(OPTION).hash(inner).finish()
        }
        Shape::Sequence(inner) => {
            let inner = layout(walk, inner);
            Fnv::new().byte(SEQUENCE).hash(inner).finish()
        }
        Shape::Array { element, length } => {
            let element = layout(walk, element);
            Fnv::new().byte(ARRAY).hash(element).number(length).finish()
        }
        Shape::Map { key, value } => {
            let key = layout(walk, key);
            let value = layout(walk, value);
            Fnv::new().byte(MAP).hash(key).hash(value).finish()
        }
        Shape::Struct { fields, .. } => fold_fields(walk, Fnv::new().byte(STRUCT), fields).finish(),
        Shape::Tuple { elements } => fold_shapes(walk, Fnv::new().byte(TUPLE), elements).finish(),
        Shape::Enum { variants, .. } => fold_variants(walk, Fnv::new().byte(ENUM), variants).finish(),
        Shape::Reference { written, protocol } => {
            let written = layout(walk, written);
            fold_protocol(walk, Fnv::new().byte(REFERENCE).hash(written), protocol).finish()
        }
    }
}

/// The most kinds one entry of a named protocol is answered with: a handler answers with one kind or none.
const MOST_ANSWERS: usize = 4;

/// A set of the kinds a reference names, as a walk finds them: ids ascending, then inner keys ascending, each once.
#[derive(Clone, Copy)]
struct KindSet {
    known: [u64; MOST_LISTED],
    known_len: usize,
    inner: [u32; MOST_LISTED],
    inner_len: usize,
}

impl KindSet {
    const EMPTY: Self = Self { known: [0; MOST_LISTED], known_len: 0, inner: [0; MOST_LISTED], inner_len: 0 };

    /// The next `count` kinds of `kinds`, found by `walk`.
    const fn take(walk: &mut Walk<'_>, kinds: &[KindPtr]) -> Self {
        let mut set = Self::EMPTY;
        let mut index = 0;
        while index < kinds.len() {
            set.add(walk.next(kinds[index]));
            index += 1;
        }

        set
    }

    /// Adds `target` in its place, unless it is held.
    const fn add(&mut self, target: Target) {
        match target {
            Target::Known(id) => self.known_len = insert_u64(&mut self.known, self.known_len, id.bits()),
            Target::Inner(key) => self.inner_len = insert_u32(&mut self.inner, self.inner_len, key),
        }
    }

    /// Whether it holds `target`.
    const fn holds(&self, target: Target) -> bool {
        match target {
            Target::Known(id) => holds_u64(&self.known, self.known_len, id.bits()),
            Target::Inner(key) => holds_u32(&self.inner, self.inner_len, key),
        }
    }

    /// The set folded onto `start` as `mode` writes it: the count and ids, as for a kind in no cycle, then in a cycle
    /// the count of members, and once they are given positions each one's position.
    const fn fold(&self, start: Fnv, mode: Mode) -> Fnv {
        let mut hash = start.count(self.known_len);
        let mut index = 0;
        while index < self.known_len {
            hash = hash.hash(self.known[index]);
            index += 1;
        }
        match mode {
            Mode::Acyclic => {
                assert!(self.inner_len == 0, "a kind in no cycle names a member of a cycle by position");
                hash
            }
            Mode::Blank => hash.count(self.inner_len),
            Mode::Positioned => {
                hash = hash.count(self.inner_len);
                let mut index = 0;
                while index < self.inner_len {
                    hash = hash.number(self.inner[index]);
                    index += 1;
                }
                hash
            }
        }
    }
}

/// A reference's protocol folded onto `start`: its tag, then its kinds as a set, and for a named protocol each kind's
/// answers (ADR-0016). Where no cycle is, this is the fold of a protocol whose kinds are ids.
const fn fold_protocol(walk: &mut Walk<'_>, start: Fnv, protocol: ProtocolShape) -> Fnv {
    match protocol {
        ProtocolShape::Told(kinds) => {
            let mode = walk.mode;
            KindSet::take(walk, kinds).fold(start.byte(TOLD), mode)
        }
        ProtocolShape::Named(entries) => fold_named(walk, start.byte(NAMED), entries),
    }
}

/// A named protocol's entries folded onto `start`: the kinds outside the cycle ascending, each with its answers, then
/// in a cycle the members, by position once they have one, or by their answers while they are blank.
const fn fold_named(walk: &mut Walk<'_>, start: Fnv, entries: &[Answered]) -> Fnv {
    let mode = walk.mode;
    let mut kinds = KindSet::EMPTY;
    let mut known_answers = [KindSet::EMPTY; MOST_LISTED];
    let mut inner_answers = [0u64; MOST_LISTED];
    let mut index = 0;
    while index < entries.len() {
        let kind = walk.next(entries[index].kind);
        let answers = KindSet::take(walk, entries[index].answers);
        assert!(entries[index].answers.len() <= MOST_ANSWERS, "a kind is answered with one kind or none");
        // A kind listed twice keeps the answers of its first entry.
        if !kinds.holds(kind) {
            kinds.add(kind);
            match kind {
                Target::Known(id) => {
                    let at = place_u64(&kinds.known, kinds.known_len, id.bits());
                    shift_targets(&mut known_answers, kinds.known_len, at);
                    known_answers[at] = answers;
                }
                Target::Inner(key) => {
                    let at = place_u32(&kinds.inner, kinds.inner_len, key);
                    shift_u64(&mut inner_answers, kinds.inner_len, at);
                    inner_answers[at] = answers.fold(Fnv::new(), mode).finish();
                }
            }
        }
        index += 1;
    }

    let mut hash = start.count(kinds.known_len);
    let mut index = 0;
    while index < kinds.known_len {
        hash = hash.hash(kinds.known[index]);
        hash = match mode {
            Mode::Acyclic => known_answers[index].fold(hash, mode),
            Mode::Blank | Mode::Positioned => hash.hash(known_answers[index].fold(Fnv::new(), mode).finish()),
        };
        index += 1;
    }
    match mode {
        Mode::Acyclic => {
            assert!(kinds.inner_len == 0, "a kind in no cycle names a member of a cycle by position");
            hash
        }
        Mode::Positioned => {
            hash = hash.count(kinds.inner_len);
            let mut index = 0;
            while index < kinds.inner_len {
                hash = hash.number(kinds.inner[index]).hash(inner_answers[index]);
                index += 1;
            }
            hash
        }
        Mode::Blank => {
            // Blank members are not told apart, so their entries are folded in the order of their answers.
            let mut answers = inner_answers;
            sort_u64(&mut answers, kinds.inner_len);
            hash = hash.count(kinds.inner_len);
            let mut index = 0;
            while index < kinds.inner_len {
                hash = hash.hash(answers[index]);
                index += 1;
            }
            hash
        }
    }
}

/// The count, then each field's layout hash in declared order, folded onto `start`.
const fn fold_fields(walk: &mut Walk<'_>, start: Fnv, fields: &[Field]) -> Fnv {
    let mut hash = start.count(fields.len());
    let mut index = 0;
    while index < fields.len() {
        hash = hash.hash(layout(walk, &fields[index].shape));
        index += 1;
    }

    hash
}

/// The count, then each element's layout hash in order, folded onto `start`.
const fn fold_shapes(walk: &mut Walk<'_>, start: Fnv, shapes: &[Shape]) -> Fnv {
    let mut hash = start.count(shapes.len());
    let mut index = 0;
    while index < shapes.len() {
        hash = hash.hash(layout(walk, &shapes[index]));
        index += 1;
    }

    hash
}

/// The count, then per variant its selector and its fields, folded onto `start`.
const fn fold_variants(walk: &mut Walk<'_>, start: Fnv, variants: &[Variant]) -> Fnv {
    let mut hash = start.count(variants.len());
    let mut index = 0;
    while index < variants.len() {
        let selected = hash.number(variants[index].selector);
        hash = match variants[index].fields {
            Fields::Named(fields) => fold_fields(walk, selected, fields),
            Fields::Unnamed(shapes) => fold_shapes(walk, selected, shapes),
        };
        index += 1;
    }

    hash
}

/// The place `value` goes in the ascending `held[..len]`.
const fn place_u64(held: &[u64], len: usize, value: u64) -> usize {
    let mut at = 0;
    while at < len && held[at] < value {
        at += 1;
    }

    at
}

/// The place `value` goes in the ascending `held[..len]`.
const fn place_u32(held: &[u32], len: usize, value: u32) -> usize {
    let mut at = 0;
    while at < len && held[at] < value {
        at += 1;
    }

    at
}

/// Whether the ascending `held[..len]` holds `value`.
const fn holds_u64(held: &[u64], len: usize, value: u64) -> bool {
    let at = place_u64(held, len, value);
    at < len && held[at] == value
}

/// Whether the ascending `held[..len]` holds `value`.
const fn holds_u32(held: &[u32], len: usize, value: u32) -> bool {
    let at = place_u32(held, len, value);
    at < len && held[at] == value
}

/// Inserts `value` into the ascending `held[..len]` unless held, answering with the new length.
const fn insert_u64(held: &mut [u64; MOST_LISTED], len: usize, value: u64) -> usize {
    let at = place_u64(held, len, value);
    if at < len && held[at] == value {
        return len;
    }
    assert!(len < MOST_LISTED, "a protocol lists at most 64 kinds");
    shift_u64(held, len, at);
    held[at] = value;

    len + 1
}

/// Inserts `value` into the ascending `held[..len]` unless held, answering with the new length.
const fn insert_u32(held: &mut [u32; MOST_LISTED], len: usize, value: u32) -> usize {
    let at = place_u32(held, len, value);
    if at < len && held[at] == value {
        return len;
    }
    assert!(len < MOST_LISTED, "a protocol lists at most 64 kinds");
    let mut index = len;
    while index > at {
        held[index] = held[index - 1];
        index -= 1;
    }
    held[at] = value;

    len + 1
}

/// Moves `held[at..len]` one place up, the one at `len - 1` written past it. `len` is the length after an insert.
const fn shift_u64(held: &mut [u64; MOST_LISTED], len: usize, at: usize) {
    let mut index = len;
    while index > at {
        if index < MOST_LISTED {
            held[index] = held[index - 1];
        }
        index -= 1;
    }
}

/// Moves `held[at..len - 1]` one place up, making room at `at`: `len` is the length after the insert.
const fn shift_targets(held: &mut [KindSet; MOST_LISTED], len: usize, at: usize) {
    let mut index = len - 1;
    while index > at {
        held[index] = held[index - 1];
        index -= 1;
    }
}

/// Sorts `values[..len]` ascending.
const fn sort_u64(values: &mut [u64; MOST_LISTED], len: usize) {
    let mut index = 1;
    while index < len {
        let mut at = index;
        while at > 0 && values[at - 1] > values[at] {
            let lower = values[at - 1];
            values[at - 1] = values[at];
            values[at] = lower;
            at -= 1;
        }
        index += 1;
    }
}

/// The layout hash of a flat schema in no cycle: one step per node, in stored order.
pub(super) fn nodes_layout(schema: &SchemaNodes) -> u64 {
    nodes_layout_in(schema, Members::Acyclic)
}

/// How a flat layout writes the members of a cycle its references name, as [`Mode`] says for a constant one.
#[derive(Clone, Copy)]
pub(super) enum Members<'m> {
    /// It names none.
    Acyclic,
    /// Counted, not told apart.
    Blank,
    /// By position: the position of the member at each place of the list the description gives.
    Positioned(&'m [u32]),
}

impl Members<'_> {
    /// The mode a constant layout is written in to fold as this one does.
    const fn mode(self) -> Mode {
        match self {
            Self::Acyclic => Mode::Acyclic,
            Self::Blank => Mode::Blank,
            Self::Positioned(_) => Mode::Positioned,
        }
    }

    /// The key of the member at `listed`: its position, or, while members are blank, its place in the list.
    fn key(self, listed: u32) -> u32 {
        match self {
            Self::Positioned(positions) => positions.get(listed as usize).copied().unwrap_or(u32::MAX),
            Self::Acyclic | Self::Blank => listed,
        }
    }
}

/// The layout hash of a flat schema, members of a cycle written as `members` says: one step per node, in stored order.
pub(super) fn nodes_layout_in(schema: &SchemaNodes, members: Members<'_>) -> u64 {
    let mut hashes = Vec::new();
    for node in schema.earlier() {
        let hash = node_layout(node, &hashes, members);
        hashes.push(hash);
    }

    node_layout(schema.root(), &hashes, members)
}

/// The layout hash of one flat node, given the hash of every node before it.
fn node_layout(node: &SchemaNode, earlier: &[u64], members: Members<'_>) -> u64 {
    match node {
        SchemaNode::Boolean => Fnv::new().byte(BOOLEAN).finish(),
        SchemaNode::Integer(kind) => Fnv::new().byte(INTEGER).byte(integer_code(*kind)).finish(),
        SchemaNode::Float(kind) => Fnv::new().byte(FLOAT).byte(float_code(*kind)).finish(),
        SchemaNode::String => Fnv::new().byte(STRING).finish(),
        SchemaNode::Bytes => Fnv::new().byte(BYTES).finish(),
        SchemaNode::Json => Fnv::new().byte(JSON).finish(),
        SchemaNode::Option(child) => Fnv::new().byte(OPTION).hash(hash_at(earlier, *child)).finish(),
        SchemaNode::Sequence(child) => Fnv::new().byte(SEQUENCE).hash(hash_at(earlier, *child)).finish(),
        SchemaNode::Array { element, length } => {
            Fnv::new().byte(ARRAY).hash(hash_at(earlier, *element)).number(*length).finish()
        }
        SchemaNode::Map { key, value } => {
            Fnv::new().byte(MAP).hash(hash_at(earlier, *key)).hash(hash_at(earlier, *value)).finish()
        }
        SchemaNode::Struct { fields, .. } => fold_field_nodes(Fnv::new().byte(STRUCT), fields, earlier).finish(),
        SchemaNode::Tuple { elements } => fold_indices(Fnv::new().byte(TUPLE), elements, earlier).finish(),
        SchemaNode::Enum { variants, .. } => {
            let start = Fnv::new().byte(ENUM).count(variants.len());
            let folded = variants.iter().fold(start, |hash, variant| {
                let selected = hash.number(variant.selector);
                fold_variant_fields(selected, &variant.fields, earlier)
            });

            folded.finish()
        }
        SchemaNode::Reference { written, protocol } => {
            fold_protocol_node(Fnv::new().byte(REFERENCE).hash(hash_at(earlier, *written)), protocol, members).finish()
        }
    }
}

/// A flat protocol folded onto `start` as [`fold_protocol`] folds a constant one.
fn fold_protocol_node(start: Fnv, protocol: &ProtocolNode, members: Members<'_>) -> Fnv {
    let mode = members.mode();
    match protocol {
        ProtocolNode::Told(kinds) => KindSet::of_referents(kinds, members).fold(start.byte(TOLD), mode),
        ProtocolNode::Named(entries) => {
            let mut known = Vec::new();
            let mut inner = Vec::new();
            for entry in entries {
                let answers = KindSet::of_referents(&entry.answers, members);
                match entry.kind {
                    Referent::Kind(id) => known.push((id.bits(), answers)),
                    Referent::Member(listed) => {
                        inner.push((members.key(listed), answers.fold(Fnv::new(), mode).finish()))
                    }
                }
            }
            known.sort_by_key(|(id, _)| *id);
            inner.sort_unstable();
            let mut hash = start.byte(NAMED).count(known.len());
            for (id, answers) in &known {
                hash = hash.hash(*id);
                hash = match mode {
                    Mode::Acyclic => answers.fold(hash, mode),
                    Mode::Blank | Mode::Positioned => hash.hash(answers.fold(Fnv::new(), mode).finish()),
                };
            }
            match members {
                Members::Acyclic => hash,
                Members::Positioned(_) => inner
                    .iter()
                    .fold(hash.count(inner.len()), |hash, (position, answers)| hash.number(*position).hash(*answers)),
                Members::Blank => {
                    let mut answers: Vec<u64> = inner.iter().map(|(_, answers)| *answers).collect();
                    answers.sort_unstable();
                    answers.iter().fold(hash.count(inner.len()), |hash, answer| hash.hash(*answer))
                }
            }
        }
    }
}

impl KindSet {
    /// The set flat `referents` name, members keyed as `members` says.
    fn of_referents(referents: &[Referent], members: Members<'_>) -> Self {
        let mut set = Self::EMPTY;
        for referent in referents {
            let target = match *referent {
                Referent::Kind(id) => Target::Known(id),
                Referent::Member(listed) => Target::Inner(members.key(listed)),
            };
            set.add(target);
        }

        set
    }
}

/// The count, then each flat field's layout hash in order, folded onto `start`.
fn fold_field_nodes(start: Fnv, fields: &[FieldNode], earlier: &[u64]) -> Fnv {
    fields.iter().fold(start.count(fields.len()), |hash, field| hash.hash(hash_at(earlier, field.shape)))
}

/// The count, then each named node's layout hash in order, folded onto `start`.
fn fold_indices(start: Fnv, indices: &[u32], earlier: &[u64]) -> Fnv {
    indices.iter().fold(start.count(indices.len()), |hash, index| hash.hash(hash_at(earlier, *index)))
}

/// One flat variant's fields folded onto `start`, named or not.
fn fold_variant_fields(start: Fnv, fields: &FieldsNode, earlier: &[u64]) -> Fnv {
    match fields {
        FieldsNode::Named(named) => fold_field_nodes(start, named, earlier),
        FieldsNode::Unnamed(indices) => fold_indices(start, indices, earlier),
    }
}

/// The hash of the node at `index`.
///
/// A `SchemaNodes` names only earlier nodes, so the fallback below cannot be
/// reached but keeps the lookup total without panicking, as `node_index` does.
fn hash_at(earlier: &[u64], index: u32) -> u64 {
    earlier.get(index as usize).copied().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::super::samples::{Command, Ledger, NoteOn, Path, doubling};
    use super::*;
    use crate::Schema;
    use alloc::collections::BTreeMap;
    use alloc::vec::Vec;

    #[test]
    fn pinned_layout_hashes_match() {
        // Catches a changed tag, code, or fold order.
        // Tripwire: layout hashes computed from ADR-0006's text by a separate program.
        assert_eq!(shape_layout(&NoteOn::SHAPE), 0xcfad_3ea2_0cdb_75b6);
        assert_eq!(shape_layout(&Path::SHAPE), 0x952c_c7a2_c575_79ed);
        assert_eq!(shape_layout(&Command::SHAPE), 0x6244_b875_ee15_b97e);
        assert_eq!(shape_layout(&Ledger::SHAPE), 0x79e1_fedc_d5e5_d3e5);
    }

    #[test]
    fn flat_hashes_equal_tree_hashes() {
        // Catches a flat arm that folds what its tree arm does not.
        assert_eq!(nodes_layout(&SchemaNodes::of(&NoteOn::SHAPE)), shape_layout(&NoteOn::SHAPE));
        assert_eq!(nodes_layout(&SchemaNodes::of(&Path::SHAPE)), shape_layout(&Path::SHAPE));
        assert_eq!(nodes_layout(&SchemaNodes::of(&Command::SHAPE)), shape_layout(&Command::SHAPE));
        assert_eq!(nodes_layout(&SchemaNodes::of(&Ledger::SHAPE)), shape_layout(&Ledger::SHAPE));
    }

    #[test]
    fn json_and_string_share_no_layout() {
        // Catches a JSON value hashed with the string tag: their bytes agree, but a kind that changes a field from one
        // to the other is a different kind (ADR-0038).
        let json = shape_layout(&Shape::Json);
        let string = shape_layout(&Shape::String);
        assert_ne!(json, string);
        assert_eq!(nodes_layout(&SchemaNodes::of(&Shape::Json)), json);
    }

    #[test]
    fn struct_and_tuple_share_no_layout() {
        // Catches a hash that merges the struct and tuple tags.
        assert_ne!(shape_layout(&<(u8, u8)>::SHAPE), shape_layout(&NoteOn::SHAPE));
    }

    #[test]
    fn lengths_orders_and_widths_change_the_layout() {
        // Catches a hash that drops a length, an order, a signedness, or a constructor.
        assert_ne!(shape_layout(&<[u8; 3]>::SHAPE), shape_layout(&<[u8; 4]>::SHAPE));
        assert_ne!(shape_layout(&<BTreeMap<u8, u16>>::SHAPE), shape_layout(&<BTreeMap<u16, u8>>::SHAPE));
        assert_ne!(shape_layout(&<(u8, u16)>::SHAPE), shape_layout(&<(u16, u8)>::SHAPE));
        assert_ne!(shape_layout(&<u32>::SHAPE), shape_layout(&<i32>::SHAPE));
        assert_ne!(shape_layout(&<Option<u8>>::SHAPE), shape_layout(&<Vec<u16>>::SHAPE));
    }

    #[test]
    fn shared_flat_schema_hashes_in_proportion() {
        // Catches a walk that unfolds sharing: 64 levels unfold to 2^64 leaves and never return.
        // Tripwire: the flat hash of 64 shared levels over a boolean, computed separately.
        assert_eq!(nodes_layout(&doubling(64, "level", SchemaNode::Boolean)), 0xfb4d_4758_de94_1bc6);
    }
}
