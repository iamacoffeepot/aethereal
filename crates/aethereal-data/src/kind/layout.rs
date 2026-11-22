//! The layout hash over both forms of a schema (ADR-0006).

use super::graph::{Graph, Slot};
use super::kind_id;
use crate::schema::MOST_LISTED;
use crate::{
    Answered, AnsweredNode, Field, FieldNode, Fields, FieldsNode, Float, Fnv, Integer, KindPtr, ProtocolNode,
    ProtocolShape, Referent, SchemaNode, SchemaNodes, Shape, Variant,
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
/// the layout of a shape that is no member of a cycle.
#[cfg(test)]
#[must_use]
pub const fn shape_layout(shape: &Shape) -> u64 {
    layout(&mut Walk::direct(), shape)
}

/// What a reference's kind is written as in a layout.
#[derive(Clone, Copy)]
enum Target {
    /// Its id's 64 bits: a kind outside the cycle being hashed, or any kind where no cycle is.
    Known(u64),
    /// A member of the cycle being hashed, by its key: its place in the graph while members are blank, and its
    /// position once they are ordered.
    Member(usize),
}

/// Where a layout being written finds what each reference's kinds are, and how it writes them.
pub(super) struct Walk<'g> {
    targets: Targets<'g>,
    mode: Mode,
}

/// Where a walk finds its references' kinds.
enum Targets<'g> {
    /// Each kind's id, computed from its own graph.
    #[cfg_attr(not(test), expect(dead_code, reason = "only the tests' layouts of a lone shape walk directly"))]
    Direct,
    /// The graph's edges of the kind being written, from `cursor` on, in the order the walk meets them.
    Graph { graph: &'g Graph, cursor: usize },
}

/// How a layout writes the kinds its references name (ADR-0006).
#[derive(Clone, Copy)]
pub(super) enum Mode {
    /// The kind is in no cycle: each kind is its id.
    Acyclic,
    /// The kind is a member of a cycle whose members are not yet ordered: a member it names is counted but not told
    /// apart.
    Blank,
    /// The kind is a member of an ordered cycle: a member it names is its position.
    Positioned,
}

impl<'g> Walk<'g> {
    /// A walk that computes each kind's id from its own graph.
    #[cfg(test)]
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
            Targets::Direct => Target::Known(kind_id(kind).bits()),
            Targets::Graph { graph, cursor } => {
                let to = graph.edge(*cursor);
                *cursor += 1;
                match graph.slot(to) {
                    Slot::Known(id) => Target::Known(id.bits()),
                    Slot::Blank => Target::Member(to),
                    Slot::Member(position) => Target::Member(position),
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
            let start = Fnv::new().byte(REFERENCE).hash(written);
            match protocol {
                ProtocolShape::Told(kinds) => fold_told(walk, start.byte(TOLD), kinds),
                ProtocolShape::Named(entries) => fold_named(walk, start.byte(NAMED), entries),
            }
            .finish()
        }
    }
}

/// A set of kinds folded onto `start` as ADR-0006 writes one: the count of those outside the cycle and their ids
/// ascending, then, in a member of a cycle, the count of the members it names and, once they are ordered, their
/// positions ascending. `ids` and `members` are each ascending, each kind once.
const fn fold_set(start: Fnv, ids: &[u64], members: &[usize], mode: Mode) -> Fnv {
    let mut hash = start.count(ids.len());
    let mut index = 0;
    while index < ids.len() {
        hash = hash.hash(ids[index]);
        index += 1;
    }

    match mode {
        Mode::Acyclic => {
            assert!(members.is_empty(), "a kind in no cycle names a member of a cycle");
            hash
        }
        Mode::Blank => hash.count(members.len()),
        Mode::Positioned => {
            hash = hash.count(members.len());
            let mut index = 0;
            while index < members.len() {
                hash = hash.count(members[index]);
                index += 1;
            }
            hash
        }
    }
}

/// The kinds a reference names, as a walk finds them: ids ascending, then members' keys ascending, each once.
struct KindSet {
    ids: [u64; MOST_LISTED],
    id_count: usize,
    members: [usize; MOST_LISTED],
    member_count: usize,
}

impl KindSet {
    /// No kinds.
    const EMPTY: Self = Self { ids: [0; MOST_LISTED], id_count: 0, members: [0; MOST_LISTED], member_count: 0 };

    /// Whether it holds `target`.
    const fn holds(&self, target: Target) -> bool {
        match target {
            Target::Known(id) => {
                let at = place_of(self.ids.split_at(self.id_count).0, id);
                at < self.id_count && self.ids[at] == id
            }
            Target::Member(key) => {
                let at = place_of_key(self.members.split_at(self.member_count).0, key);
                at < self.member_count && self.members[at] == key
            }
        }
    }

    /// Adds `target`, which it does not hold, in its place, answering with that place among the ids or the members.
    const fn add(&mut self, target: Target) -> usize {
        assert!(self.id_count + self.member_count < MOST_LISTED, "a protocol lists at most 64 kinds");
        match target {
            Target::Known(id) => {
                let at = place_of(self.ids.split_at(self.id_count).0, id);
                shift_up(&mut self.ids, self.id_count, at);
                self.ids[at] = id;
                self.id_count += 1;
                at
            }
            Target::Member(key) => {
                let at = place_of_key(self.members.split_at(self.member_count).0, key);
                shift_up(&mut self.members, self.member_count, at);
                self.members[at] = key;
                self.member_count += 1;
                at
            }
        }
    }

    /// The set folded onto `start`, as [`fold_set`] writes one.
    const fn fold(&self, start: Fnv, mode: Mode) -> Fnv {
        fold_set(start, self.ids.split_at(self.id_count).0, self.members.split_at(self.member_count).0, mode)
    }
}

/// A told list's kinds folded onto `start`, as a set (ADR-0016).
const fn fold_told(walk: &mut Walk<'_>, start: Fnv, kinds: &[KindPtr]) -> Fnv {
    let mut set = KindSet::EMPTY;
    let mut index = 0;
    while index < kinds.len() {
        let target = walk.next(kinds[index]);
        if !set.holds(target) {
            set.add(target);
        }
        index += 1;
    }

    set.fold(start, walk.mode)
}

/// What one entry of a named protocol is answered with, as a walk finds it: a handler answers with one kind or none.
#[derive(Clone, Copy)]
enum Answer {
    /// It is a tell.
    Nothing,
    /// It is answered with this kind.
    One(Target),
}

impl Answer {
    /// The answer of `entry`, its kind found by `walk` after the entry's own.
    const fn of(walk: &mut Walk<'_>, entry: &Answered) -> Self {
        match entry.answer() {
            Some(answer) => Self::One(walk.next(answer)),
            None => Self::Nothing,
        }
    }

    /// The answer folded onto `start` as a set of kinds is: one kind or none.
    const fn fold(self, start: Fnv, mode: Mode) -> Fnv {
        match self {
            Self::Nothing => fold_set(start, &[], &[], mode),
            Self::One(Target::Known(id)) => fold_set(start, &[id], &[], mode),
            Self::One(Target::Member(key)) => fold_set(start, &[], &[key], mode),
        }
    }
}

/// A named protocol's entries folded onto `start` (ADR-0006, ADR-0016): the count of the kinds outside the cycle, and
/// each one's id ascending followed by its answers as a set. Then, in a member of a cycle, the count of the members it
/// takes, and once they are ordered each one's position ascending followed by the hash of its answers; while they are
/// not, the hashes of their answers ascending. A kind listed twice keeps the answer of its first entry.
const fn fold_named(walk: &mut Walk<'_>, start: Fnv, entries: &[Answered]) -> Fnv {
    let mode = walk.mode;
    let mut kinds = KindSet::EMPTY;
    let mut answers = [Answer::Nothing; MOST_LISTED];
    let mut member_answers = [0; MOST_LISTED];
    let mut index = 0;
    while index < entries.len() {
        let kind = walk.next(entries[index].kind);
        let answer = Answer::of(walk, &entries[index]);
        if !kinds.holds(kind) {
            let at = kinds.add(kind);
            match kind {
                Target::Known(_) => {
                    shift_up(&mut answers, kinds.id_count - 1, at);
                    answers[at] = answer;
                }
                Target::Member(_) => {
                    shift_up(&mut member_answers, kinds.member_count - 1, at);
                    member_answers[at] = answer.fold(Fnv::new(), mode).finish();
                }
            }
        }
        index += 1;
    }

    let mut hash = start.count(kinds.id_count);
    let mut index = 0;
    while index < kinds.id_count {
        hash = answers[index].fold(hash.hash(kinds.ids[index]), mode);
        index += 1;
    }

    let member_count = kinds.member_count;
    match mode {
        Mode::Acyclic => {
            assert!(member_count == 0, "a kind in no cycle names a member of a cycle");
            hash
        }
        Mode::Positioned => {
            hash = hash.count(member_count);
            let mut index = 0;
            while index < member_count {
                hash = hash.count(kinds.members[index]).hash(member_answers[index]);
                index += 1;
            }
            hash
        }
        Mode::Blank => {
            // Members not yet ordered are not told apart, so their entries are written in the order of their answers.
            sort_ascending(member_answers.split_at_mut(member_count).0);
            hash = hash.count(member_count);
            let mut index = 0;
            while index < member_count {
                hash = hash.hash(member_answers[index]);
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

/// The place `value` goes in the ascending `held`.
const fn place_of(held: &[u64], value: u64) -> usize {
    let mut at = 0;
    while at < held.len() && held[at] < value {
        at += 1;
    }

    at
}

/// The place `key` goes in the ascending `held`.
const fn place_of_key(held: &[usize], key: usize) -> usize {
    let mut at = 0;
    while at < held.len() && held[at] < key {
        at += 1;
    }

    at
}

/// Moves `values[at..len]` one place up, making room at `at` for the value that will make them `len + 1`.
const fn shift_up<T: Copy>(values: &mut [T; MOST_LISTED], len: usize, at: usize) {
    let mut index = len;
    while index > at {
        values[index] = values[index - 1];
        index -= 1;
    }
}

/// Sorts `values` ascending.
const fn sort_ascending(values: &mut [u64]) {
    let mut index = 1;
    while index < values.len() {
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
///
/// A schema in no cycle names no member of one; [`KindId::from_description`](super::KindId::from_description) refuses
/// a schema that does before it is hashed.
pub(super) fn nodes_layout(schema: &SchemaNodes) -> u64 {
    nodes_layout_in(schema, Members::Acyclic)
}

/// How a flat layout writes the members of a cycle its references name, as [`Mode`] says for a constant one.
#[derive(Clone, Copy)]
pub(super) enum Members<'m> {
    /// It is in no cycle, and names no member of one.
    Acyclic,
    /// Counted, not told apart.
    Blank,
    /// By position: the position of the member at each place of the list a description gives.
    Positioned(&'m [usize]),
}

impl Members<'_> {
    /// The key of the member at `listed`: its position, or, while members are blank, its place in the list.
    ///
    /// A described cycle's places are checked to be in the list before it is hashed, so the fallback below is never
    /// reached; it keeps the lookup total without a panic.
    fn key(self, listed: u32) -> usize {
        let listed = listed as usize;
        match self {
            Self::Positioned(positions) => positions.get(listed).copied().unwrap_or(usize::MAX),
            Self::Acyclic | Self::Blank => listed,
        }
    }

    /// The members part of a set folded onto `start`, as [`fold_set`] writes it: nothing in no cycle, the count while
    /// members are blank, and the count and the positions ascending once they are ordered. `keys` are ascending.
    fn fold(self, start: Fnv, keys: &[usize]) -> Fnv {
        match self {
            Self::Acyclic => start,
            Self::Blank => start.count(keys.len()),
            Self::Positioned(_) => keys.iter().fold(start.count(keys.len()), |hash, key| hash.count(*key)),
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
            let start = Fnv::new().byte(REFERENCE).hash(hash_at(earlier, *written));
            match protocol {
                ProtocolNode::Told(kinds) => fold_referents(start.byte(TOLD), kinds, members),
                ProtocolNode::Named(entries) => fold_named_nodes(start.byte(NAMED), entries, members),
            }
            .finish()
        }
    }
}

/// Flat `referents` folded onto `start` as [`fold_set`] folds a constant set, each member keyed as `members` says.
fn fold_referents(start: Fnv, referents: &[Referent], members: Members<'_>) -> Fnv {
    let mut ids = Vec::new();
    let mut keys = Vec::new();
    for referent in referents {
        match *referent {
            Referent::Kind(id) => ids.push(id.bits()),
            Referent::Member(listed) => keys.push(members.key(listed)),
        }
    }
    ids.sort_unstable();
    ids.dedup();
    keys.sort_unstable();
    keys.dedup();

    let hash = ids.iter().fold(start.count(ids.len()), |hash, id| hash.hash(*id));
    members.fold(hash, &keys)
}

/// A flat named protocol's entries folded onto `start` as [`fold_named`] folds a constant one.
fn fold_named_nodes(start: Fnv, entries: &[AnsweredNode], members: Members<'_>) -> Fnv {
    let mut outside = Vec::new();
    let mut inside = Vec::new();
    for entry in entries {
        match entry.kind {
            Referent::Kind(id) => outside.push((id.bits(), entry.answers.as_slice())),
            Referent::Member(listed) => {
                let answers = fold_referents(Fnv::new(), &entry.answers, members).finish();
                inside.push((members.key(listed), answers));
            }
        }
    }
    // Stable sorts, so a kind listed twice keeps the answers of its first entry.
    outside.sort_by_key(|(id, _)| *id);
    outside.dedup_by_key(|(id, _)| *id);
    inside.sort_by_key(|(key, _)| *key);
    inside.dedup_by_key(|(key, _)| *key);

    let hash = outside
        .iter()
        .fold(start.count(outside.len()), |hash, (id, answers)| fold_referents(hash.hash(*id), answers, members));
    match members {
        Members::Acyclic => hash,
        Members::Positioned(_) => inside
            .iter()
            .fold(hash.count(inside.len()), |hash, (position, answers)| hash.count(*position).hash(*answers)),
        Members::Blank => {
            let mut answers: Vec<u64> = inside.iter().map(|(_, answers)| *answers).collect();
            answers.sort_unstable();
            answers.iter().fold(hash.count(answers.len()), |hash, answers| hash.hash(*answers))
        }
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
