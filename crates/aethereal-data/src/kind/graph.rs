//! The ids of kinds that reach each other through the protocols their references name, computed as the program is
//! built (ADR-0006).
//!
//! The kinds a kind's references reach, and theirs in turn, are a graph. A kind's id is computed from the whole graph
//! it reaches: the graph is found from its node, broken into its strongly connected components, and each component
//! hashed after every component it reaches. A component of one kind with no edge to itself is hashed as a kind always
//! was, each kind its references name written as its id. A component of more kinds, or of one that names itself, is a
//! cycle, hashed as one unit: each member's layout written with the members it names as positions, the members in the
//! order of their names, then their layouts with members blanked; the cycle hashed once; and each member's id the
//! cycle's hash with its position.
//!
//! Nothing here recurses over the graph: it is walked with explicit stacks, and a graph past [`MOST_KINDS`] kinds or
//! [`MOST_EDGES`] edges stops the build.

use super::layout::{CYCLE, Mode, Walk, layout};
use super::shape::{KindPtr, Site};
use super::{KindId, KindName};
use crate::{Fields, Fnv, ProtocolShape, Shape};

/// The most kinds one kind's graph may reach, itself included.
pub const MOST_KINDS: usize = 128;

/// The most edges one kind's graph may hold: one for each kind a reference lists, repeats included.
pub const MOST_EDGES: usize = 1024;

/// What the hash knows of one kind of the graph.
#[derive(Clone, Copy)]
pub(crate) enum Slot {
    /// Its id is not yet computed: its component is not yet reached.
    Unresolved,
    /// Its id.
    Known(KindId),
    /// A member of the cycle being hashed, before positions are given.
    Blank,
    /// A member of the cycle being hashed, at its position.
    Member(u32),
}

/// Whether a kind of the graph is a member of a cycle, and where.
#[derive(Clone, Copy)]
pub(crate) enum Membership {
    /// It is a component of its own.
    Alone,
    /// It is a member of the cycle numbered `component`, at `position`.
    Member { component: u32, position: u32 },
}

/// The kinds one kind reaches, the edges between them, and what is known of each.
pub(crate) struct Graph {
    kinds: [KindPtr; MOST_KINDS],
    sites: [u64; MOST_KINDS],
    len: usize,
    edges: [u16; MOST_EDGES],
    edge_len: usize,
    first_edge: [u16; MOST_KINDS],
    end_edge: [u16; MOST_KINDS],
    slots: [Slot; MOST_KINDS],
    membership: [Membership; MOST_KINDS],
}

impl Graph {
    /// The graph `root` reaches, root first, with every id computed.
    pub(crate) const fn of(root: KindPtr) -> Self {
        let mut graph = Self {
            kinds: [KindPtr::UNSET; MOST_KINDS],
            sites: [0; MOST_KINDS],
            len: 0,
            edges: [0; MOST_EDGES],
            edge_len: 0,
            first_edge: [0; MOST_KINDS],
            end_edge: [0; MOST_KINDS],
            slots: [Slot::Unresolved; MOST_KINDS],
            membership: [Membership::Alone; MOST_KINDS],
        };
        graph.add(root);
        graph.discover();
        let components = Components::of(&graph);
        graph.resolve(&components);

        graph
    }

    /// The kind at `index`.
    pub(crate) const fn kind(&self, index: usize) -> KindPtr {
        self.kinds[index]
    }

    /// How many kinds it holds.
    pub(crate) const fn len(&self) -> usize {
        self.len
    }

    /// The kind edge number `edge` reaches.
    pub(crate) const fn edge(&self, edge: usize) -> usize {
        self.edges[edge] as usize
    }

    /// What is known of the kind at `index`.
    pub(crate) const fn slot(&self, index: usize) -> Slot {
        self.slots[index]
    }

    /// Whether the kind at `index` is a member of a cycle.
    pub(crate) const fn membership(&self, index: usize) -> Membership {
        self.membership[index]
    }

    /// Expands each kind in the order it was added, adding the kinds its references list and an edge to each.
    const fn discover(&mut self) {
        let mut next = 0;
        while next < self.len {
            self.first_edge[next] = self.edge_len as u16;
            let shape = self.kinds[next].get().shape;
            self.expand(shape);
            self.end_edge[next] = self.edge_len as u16;
            next += 1;
        }
    }

    /// Adds an edge to each kind `shape`'s references list, in the order a layout's walk meets them. A `Shape` is a
    /// finite tree up to its references, which are not followed here, so the recursion is bounded by the type.
    const fn expand(&mut self, shape: &Shape) {
        match *shape {
            Shape::Boolean | Shape::Integer(_) | Shape::Float(_) | Shape::String | Shape::Bytes | Shape::Json => {}
            Shape::Option(inner) | Shape::Sequence(inner) | Shape::Array { element: inner, .. } => self.expand(inner),
            Shape::Map { key, value } => {
                self.expand(key);
                self.expand(value);
            }
            Shape::Struct { fields, .. } => {
                let mut index = 0;
                while index < fields.len() {
                    self.expand(&fields[index].shape);
                    index += 1;
                }
            }
            Shape::Tuple { elements } => self.expand_all(elements),
            Shape::Enum { variants, .. } => {
                let mut index = 0;
                while index < variants.len() {
                    match variants[index].fields {
                        Fields::Named(fields) => {
                            let mut field = 0;
                            while field < fields.len() {
                                self.expand(&fields[field].shape);
                                field += 1;
                            }
                        }
                        Fields::Unnamed(shapes) => self.expand_all(shapes),
                    }
                    index += 1;
                }
            }
            Shape::Reference { written, protocol } => {
                self.expand(written);
                match protocol {
                    ProtocolShape::Told(kinds) => self.edges_to(kinds),
                    ProtocolShape::Named(entries) => {
                        let mut index = 0;
                        while index < entries.len() {
                            self.edge_to(entries[index].kind);
                            self.edges_to(entries[index].answers);
                            index += 1;
                        }
                    }
                }
            }
        }
    }

    /// Expands each of `shapes`.
    const fn expand_all(&mut self, shapes: &[Shape]) {
        let mut index = 0;
        while index < shapes.len() {
            self.expand(&shapes[index]);
            index += 1;
        }
    }

    /// Adds an edge to each of `kinds`.
    const fn edges_to(&mut self, kinds: &[KindPtr]) {
        let mut index = 0;
        while index < kinds.len() {
            self.edge_to(kinds[index]);
            index += 1;
        }
    }

    /// Adds an edge to `kind`, adding it first unless it is held.
    const fn edge_to(&mut self, kind: KindPtr) {
        assert!(self.edge_len < MOST_EDGES, "a kind's graph of references holds more than 1024 edges");
        let to = match self.find(kind) {
            Some(index) => index,
            None => self.add(kind),
        };
        self.edges[self.edge_len] = to as u16;
        self.edge_len += 1;
    }

    /// The place of `kind`, when it is held: a non-generic kind is found by its site, a generic one never is.
    const fn find(&self, kind: KindPtr) -> Option<usize> {
        let Site::Defined { at } = kind.get().site else {
            return None;
        };
        let hash = Fnv::new().bytes(at.as_bytes()).finish();
        let mut index = 0;
        while index < self.len {
            let found = match self.kinds[index].get().site {
                Site::Defined { at: other } => self.sites[index] == hash && same_text(at, other),
                Site::Unsited => false,
            };
            if found {
                return Some(index);
            }
            index += 1;
        }

        None
    }

    /// Adds `kind`, answering with its place.
    const fn add(&mut self, kind: KindPtr) -> usize {
        assert!(self.len < MOST_KINDS, "a kind's graph of references reaches more than 128 kinds");
        let index = self.len;
        self.kinds[index] = kind;
        self.sites[index] = match kind.get().site {
            Site::Defined { at } => Fnv::new().bytes(at.as_bytes()).finish(),
            Site::Unsited => 0,
        };
        self.len += 1;

        index
    }

    /// Computes every id, a component after every component it reaches.
    const fn resolve(&mut self, components: &Components) {
        let mut component = 0;
        while component < components.count {
            let mut members = [0u16; MOST_KINDS];
            let mut count = 0;
            let mut index = 0;
            while index < self.len {
                if components.of[index] as usize == component {
                    members[count] = index as u16;
                    count += 1;
                }
                index += 1;
            }
            let cyclic = count > 1 || self.names_itself(members[0] as usize);
            if cyclic {
                self.resolve_cycle(component as u32, &members, count);
            } else {
                let only = members[0] as usize;
                let shape = self.kinds[only].get().shape;
                let layout = layout(&mut Walk::over(self, self.first_edge[only] as usize, Mode::Acyclic), shape);
                self.slots[only] = Slot::Known(KindId::from_layout(&self.kinds[only].get().name, layout));
            }
            component += 1;
        }
    }

    /// Whether the kind at `index` has an edge to itself.
    const fn names_itself(&self, index: usize) -> bool {
        let mut edge = self.first_edge[index] as usize;
        while edge < self.end_edge[index] as usize {
            if self.edges[edge] as usize == index {
                return true;
            }
            edge += 1;
        }

        false
    }

    /// Hashes the cycle of the `count` kinds `members` as one unit.
    const fn resolve_cycle(&mut self, component: u32, members: &[u16; MOST_KINDS], count: usize) {
        let mut index = 0;
        while index < count {
            let member = members[index] as usize;
            assert!(
                matches!(self.kinds[member].get().site, Site::Defined { .. }),
                "a cycle of kinds passes through a generic kind or one whose `Kind` is written by hand, which has no \
                 static node: a cycle's members are non-generic derived kinds"
            );
            self.slots[member] = Slot::Blank;
            index += 1;
        }

        // Each member's layout with the members it names blanked: what orders members of one name.
        let mut blanks = [0u64; MOST_KINDS];
        let mut index = 0;
        while index < count {
            blanks[index] = self.layout_of(members[index] as usize, Mode::Blank);
            index += 1;
        }

        // The members in order of name, then of blank layout: insertion sort over places in `members`.
        let mut order = [0u16; MOST_KINDS];
        let mut index = 0;
        while index < count {
            order[index] = index as u16;
            index += 1;
        }
        let mut index = 1;
        while index < count {
            let mut at = index;
            while at > 0 {
                let before = self.member_order(members, &blanks, order[at - 1] as usize, order[at] as usize);
                assert!(
                    !matches!(before, core::cmp::Ordering::Equal),
                    "two kinds of one name and one layout are members of one cycle: no order tells them apart"
                );
                if matches!(before, core::cmp::Ordering::Less) {
                    break;
                }
                let lower = order[at - 1];
                order[at - 1] = order[at];
                order[at] = lower;
                at -= 1;
            }
            index += 1;
        }

        let mut position = 0;
        while position < count {
            let member = members[order[position] as usize] as usize;
            self.slots[member] = Slot::Member(position as u32);
            self.membership[member] = Membership::Member { component, position: position as u32 };
            position += 1;
        }

        // The cycle's hash: its count, then each member in position order, its name and its layout.
        let mut cycle = Fnv::new().byte(CYCLE).count(count);
        let mut position = 0;
        while position < count {
            let member = members[order[position] as usize] as usize;
            let name = self.kinds[member].get().name.as_str().as_bytes();
            cycle = cycle.count(name.len()).bytes(name).hash(self.layout_of(member, Mode::Positioned));
            position += 1;
        }
        let cycle = cycle.finish();

        let mut position = 0;
        while position < count {
            let member = members[order[position] as usize] as usize;
            let layout = Fnv::new().byte(CYCLE).hash(cycle).count(position).finish();
            self.slots[member] = Slot::Known(KindId::from_layout(&self.kinds[member].get().name, layout));
            position += 1;
        }
    }

    /// The layout of the kind at `index`, written as `mode` says.
    const fn layout_of(&self, index: usize, mode: Mode) -> u64 {
        layout(&mut Walk::over(self, self.first_edge[index] as usize, mode), self.kinds[index].get().shape)
    }

    /// The order of the members at places `one` and `other` of `members`: by name, then by blank layout.
    const fn member_order(
        &self,
        members: &[u16; MOST_KINDS],
        blanks: &[u64; MOST_KINDS],
        one: usize,
        other: usize,
    ) -> core::cmp::Ordering {
        let name = &self.kinds[members[one] as usize].get().name;
        let other_name = &self.kinds[members[other] as usize].get().name;
        match compare_names(name, other_name) {
            core::cmp::Ordering::Equal => {
                if blanks[one] < blanks[other] {
                    core::cmp::Ordering::Less
                } else if blanks[one] > blanks[other] {
                    core::cmp::Ordering::Greater
                } else {
                    core::cmp::Ordering::Equal
                }
            }
            order => order,
        }
    }
}

/// The strongly connected components of a graph, found by Tarjan's algorithm over an explicit stack, each numbered in
/// the order it completes: a component after every component it reaches.
struct Components {
    of: [u16; MOST_KINDS],
    count: usize,
}

impl Components {
    /// The components of `graph`, every kind of which its root reaches.
    const fn of(graph: &Graph) -> Self {
        const UNVISITED: u16 = u16::MAX;
        let mut found = Self { of: [0; MOST_KINDS], count: 0 };
        let mut number = [UNVISITED; MOST_KINDS];
        let mut low = [0u16; MOST_KINDS];
        let mut on_stack = [false; MOST_KINDS];
        let mut stack = [0u16; MOST_KINDS];
        let mut frames = [(0u16, 0u16); MOST_KINDS];
        let mut next_number = 1u16;

        // The root first, as every kind is reached from it.
        number[0] = 0;
        low[0] = 0;
        on_stack[0] = true;
        frames[0] = (0, graph.first_edge[0]);
        let mut stack_len = 1;
        let mut frames_len = 1;

        while frames_len > 0 {
            let (kind, edge) = frames[frames_len - 1];
            let kind = kind as usize;
            if edge < graph.end_edge[kind] {
                frames[frames_len - 1].1 = edge + 1;
                let to = graph.edges[edge as usize] as usize;
                if number[to] == UNVISITED {
                    number[to] = next_number;
                    low[to] = next_number;
                    next_number += 1;
                    stack[stack_len] = to as u16;
                    stack_len += 1;
                    on_stack[to] = true;
                    frames[frames_len] = (to as u16, graph.first_edge[to]);
                    frames_len += 1;
                } else if on_stack[to] && number[to] < low[kind] {
                    low[kind] = number[to];
                }
            } else {
                frames_len -= 1;
                if low[kind] == number[kind] {
                    loop {
                        stack_len -= 1;
                        let member = stack[stack_len] as usize;
                        on_stack[member] = false;
                        found.of[member] = found.count as u16;
                        if member == kind {
                            break;
                        }
                    }
                    found.count += 1;
                }
                if frames_len > 0 {
                    let parent = frames[frames_len - 1].0 as usize;
                    if low[kind] < low[parent] {
                        low[parent] = low[kind];
                    }
                }
            }
        }

        found
    }
}

/// The id of `kind`, computed from the graph it reaches (ADR-0006).
#[must_use]
pub const fn kind_id(kind: KindPtr) -> KindId {
    match Graph::of(kind).slots[0] {
        Slot::Known(id) => id,
        Slot::Unresolved | Slot::Blank | Slot::Member(_) => panic!("a kind's graph leaves its root unresolved"),
    }
}

/// The order of two names, byte by byte.
const fn compare_names(one: &KindName, other: &KindName) -> core::cmp::Ordering {
    let (one, other) = (one.as_str().as_bytes(), other.as_str().as_bytes());
    let mut index = 0;
    while index < one.len() && index < other.len() {
        if one[index] < other[index] {
            return core::cmp::Ordering::Less;
        }
        if one[index] > other[index] {
            return core::cmp::Ordering::Greater;
        }
        index += 1;
    }
    if one.len() < other.len() {
        core::cmp::Ordering::Less
    } else if one.len() > other.len() {
        core::cmp::Ordering::Greater
    } else {
        core::cmp::Ordering::Equal
    }
}

/// Whether two texts are one.
const fn same_text(one: &str, other: &str) -> bool {
    let (one, other) = (one.as_bytes(), other.as_bytes());
    if one.len() != other.len() {
        return false;
    }
    let mut index = 0;
    while index < one.len() {
        if one[index] != other[index] {
            return false;
        }
        index += 1;
    }

    true
}
