//! A kind described as plain data, and its id computed from the description when the program runs (ADR-0006).
//!
//! A kind in no cycle is described by its name and its flat schema, each kind its references name written as its id.
//! A member of a cycle is described with its whole cycle: each member's name and flat schema, a member named by its
//! place in the list, and which member it is. The list may be in any order: the id is computed as a constant computes
//! it, from the members' names and layouts alone.

use super::graph::{Graph, Membership};
use super::layout::{CYCLE, Members, nodes_layout_in};
use super::{KindId, KindName, KindPtr};
use crate::{Fnv, Plain, ProtocolNode, Referent, SchemaNode, SchemaNodes};
use alloc::vec::Vec;

/// A kind described as plain data: what a guest says of a kind for a host to compute its id from (ADR-0006).
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
pub enum KindDescription {
    /// A kind in no cycle: its name and schema.
    Alone {
        /// Its name.
        name: KindName,
        /// Its schema, each kind its references name written as its id.
        schema: SchemaNodes,
    },
    /// A member of a cycle: the cycle's members, and which of them it is.
    Cycle {
        /// Each member, in any order; a reference names a member by its place here.
        members: Vec<CycleMember>,
        /// The place of the kind described.
        this: u32,
    },
}

/// One member of a described cycle.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
pub struct CycleMember {
    /// Its name.
    pub name: KindName,
    /// Its schema, a member its references name written as [`Referent::Member`].
    pub schema: SchemaNodes,
}

/// Why a described cycle is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleFault {
    /// It has no members, or `this` is past them.
    NoSuchMember,
    /// A reference names a member past the list.
    MemberPastList,
    /// Its members are not one cycle: one does not reach another through references, or a lone member does not name
    /// itself.
    NotOneCycle,
    /// Two members have one name and one layout with members blanked, so no order tells them apart.
    Tied,
}

impl KindDescription {
    /// The description of `kind`: alone, or with its cycle, its members in position order.
    #[must_use]
    pub fn of(kind: KindPtr) -> Self {
        let graph = Graph::of(kind);
        let Membership::Member { component, position } = graph.membership(0) else {
            let node = kind.get();
            return Self::Alone { name: node.name.clone(), schema: SchemaNodes::of(node.shape) };
        };

        let mut ordered: Vec<(u32, KindPtr)> = (0..graph.len())
            .filter_map(|index| match graph.membership(index) {
                Membership::Member { component: other, position } if other == component => {
                    Some((position, graph.kind(index)))
                }
                Membership::Member { .. } | Membership::Alone => None,
            })
            .collect();
        ordered.sort_by_key(|(position, _)| *position);
        let kinds: Vec<KindPtr> = ordered.iter().map(|(_, kind)| *kind).collect();
        let members = kinds
            .iter()
            .map(|kind| CycleMember {
                name: kind.get().name.clone(),
                schema: SchemaNodes::of_in(kind.get().shape, &kinds),
            })
            .collect();

        Self::Cycle { members, this: position }
    }

    /// The id it describes.
    ///
    /// # Errors
    ///
    /// A [`CycleFault`] when it describes a cycle that is not one, or whose members tie.
    pub fn id(&self) -> Result<KindId, CycleFault> {
        match self {
            Self::Alone { name, schema } => Ok(KindId::from_description(name, schema)),
            Self::Cycle { members, this } => cycle_id(members, *this),
        }
    }
}

/// The id of the member at `this` of `members`, computed as a constant computes it.
fn cycle_id(members: &[CycleMember], this: u32) -> Result<KindId, CycleFault> {
    let count = members.len();
    if this as usize >= count {
        return Err(CycleFault::NoSuchMember);
    }
    let named: Vec<Vec<u32>> = members.iter().map(|member| named_members(&member.schema)).collect();
    let past = named.iter().flatten().any(|listed| *listed as usize >= count);
    if past {
        return Err(CycleFault::MemberPastList);
    }
    if !one_cycle(&named) {
        return Err(CycleFault::NotOneCycle);
    }

    let blanks: Vec<u64> = members.iter().map(|member| nodes_layout_in(&member.schema, Members::Blank)).collect();
    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by(|one, other| {
        let names = members[*one].name.as_str().as_bytes().cmp(members[*other].name.as_str().as_bytes());
        names.then(blanks[*one].cmp(&blanks[*other]))
    });
    let tied = order.windows(2).any(|pair| {
        matches!(pair, [one, other] if members[*one].name == members[*other].name && blanks[*one] == blanks[*other])
    });
    if tied {
        return Err(CycleFault::Tied);
    }

    let mut positions = alloc::vec![0u32; count];
    for (position, listed) in order.iter().enumerate() {
        positions[*listed] = position as u32;
    }
    let cycle = order
        .iter()
        .fold(Fnv::new().byte(CYCLE).count(count), |hash, listed| {
            let member = &members[*listed];
            let name = member.name.as_str().as_bytes();
            let layout = nodes_layout_in(&member.schema, Members::Positioned(&positions));
            hash.count(name.len()).bytes(name).hash(layout)
        })
        .finish();
    let position = positions[this as usize];
    let layout = Fnv::new().byte(CYCLE).hash(cycle).count(position as usize).finish();

    Ok(KindId::from_layout(&members[this as usize].name, layout))
}

/// The places of the members `schema`'s references name.
fn named_members(schema: &SchemaNodes) -> Vec<u32> {
    let mut named = Vec::new();
    let nodes = schema.earlier().iter().chain(core::iter::once(schema.root()));
    for node in nodes {
        let SchemaNode::Reference { protocol, .. } = node else {
            continue;
        };
        let referents: Vec<Referent> = match protocol {
            ProtocolNode::Told(kinds) => kinds.clone(),
            ProtocolNode::Named(entries) => entries
                .iter()
                .flat_map(|entry| core::iter::once(entry.kind).chain(entry.answers.iter().copied()))
                .collect(),
        };
        named.extend(referents.iter().filter_map(|referent| match referent {
            Referent::Member(listed) => Some(*listed),
            Referent::Kind(_) => None,
        }));
    }

    named
}

/// Whether the members, each naming the members `named` lists, are one strongly connected cycle: each reaches every
/// other, and a lone member names itself.
fn one_cycle(named: &[Vec<u32>]) -> bool {
    let count = named.len();
    let forward: Vec<Vec<usize>> =
        named.iter().map(|names| names.iter().map(|listed| *listed as usize).collect()).collect();
    let mut backward = alloc::vec![Vec::new(); count];
    for (from, names) in forward.iter().enumerate() {
        for to in names {
            backward[*to].push(from);
        }
    }
    let lone_names_itself = count > 1 || forward[0].contains(&0);

    reaches_all(&forward) && reaches_all(&backward) && lone_names_itself
}

/// Whether the first member reaches every member along `edges`.
fn reaches_all(edges: &[Vec<usize>]) -> bool {
    let mut seen = alloc::vec![false; edges.len()];
    seen[0] = true;
    let mut stack = Vec::from([0]);
    while let Some(at) = stack.pop() {
        for next in &edges[at] {
            if !seen[*next] {
                seen[*next] = true;
                stack.push(*next);
            }
        }
    }

    seen.iter().all(|reached| *reached)
}
