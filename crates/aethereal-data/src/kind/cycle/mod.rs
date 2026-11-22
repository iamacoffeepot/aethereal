//! A kind described as plain data, and its id computed from the description when the program runs (ADR-0006).
//!
//! A kind in no cycle is described by its name and its flat schema, each kind its references name written as its id.
//! A member of a cycle is described with its whole cycle: each member's name and flat schema, a member named by its
//! place in the list, and which member it is. The list may be in any order: the id is computed as a constant computes
//! it, from the members' names and layouts alone.

#[cfg(test)]
mod tests;

use super::graph::{Graph, Membership};
use super::layout::{CYCLE, Members, nodes_layout_in};
use super::{KindId, KindName, KindPtr};
use crate::{Fnv, Plain, ProtocolNode, Referent, SchemaNode, SchemaNodes};
use alloc::vec::Vec;
use core::error::Error;
use core::{fmt, iter};

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

/// Why a description of a kind is refused (ADR-0006).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescriptionFault {
    /// A kind described alone names a member of a cycle by its place: a member arrives with its whole cycle.
    MemberAlone,
    /// A cycle has no members, or `this` is past them.
    NoSuchMember,
    /// A reference names a member past the list.
    MemberPastList,
    /// Its members are not one cycle: one does not reach another through references, or a lone member does not name
    /// itself.
    NotOneCycle,
    /// Two members have one name and one layout with members blanked, so no order tells them apart.
    Tied,
}

impl fmt::Display for DescriptionFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MemberAlone => f.write_str("a kind described alone names a member of a cycle"),
            Self::NoSuchMember => f.write_str("a described cycle has no member at the place it describes"),
            Self::MemberPastList => f.write_str("a reference names a member past the described cycle's list"),
            Self::NotOneCycle => f.write_str("a described cycle's members are not one cycle"),
            Self::Tied => f.write_str("two members of a described cycle have one name and one layout"),
        }
    }
}

impl Error for DescriptionFault {}

impl KindDescription {
    /// The description of `kind`: alone, or with its cycle, its members in position order.
    #[must_use]
    pub fn of(kind: KindPtr) -> Self {
        let graph = Graph::of(kind);
        let Membership::Member { component, position } = graph.membership(0) else {
            let node = kind.get();
            return Self::Alone { name: node.name.clone(), schema: SchemaNodes::of(node.shape) };
        };

        let mut ordered: Vec<(usize, KindPtr)> = (0..graph.len())
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
        #[expect(clippy::cast_possible_truncation, reason = "a graph holds at most 128 kinds, so a position fits")]
        let this = position as u32;

        Self::Cycle { members, this }
    }

    /// The id it describes.
    ///
    /// # Errors
    ///
    /// A [`DescriptionFault`] when it describes a kind alone that names a member of a cycle, or a cycle that is not
    /// one or whose members tie.
    pub fn id(&self) -> Result<KindId, DescriptionFault> {
        match self {
            Self::Alone { name, schema } => KindId::from_description(name, schema),
            Self::Cycle { members, this } => cycle_id(members, *this),
        }
    }
}

/// The id of the member at `this` of `members`, computed as a constant computes it.
fn cycle_id(members: &[CycleMember], this: u32) -> Result<KindId, DescriptionFault> {
    let count = members.len();
    if this as usize >= count {
        return Err(DescriptionFault::NoSuchMember);
    }
    let named: Vec<Vec<u32>> = members.iter().map(|member| named_members(&member.schema)).collect();
    let past = named.iter().flatten().any(|listed| *listed as usize >= count);
    if past {
        return Err(DescriptionFault::MemberPastList);
    }
    if !one_cycle(&named) {
        return Err(DescriptionFault::NotOneCycle);
    }

    let blanks: Vec<u64> = members.iter().map(|member| nodes_layout_in(&member.schema, Members::Blank)).collect();
    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by(|one, other| {
        let by_name = members[*one].name.as_str().as_bytes().cmp(members[*other].name.as_str().as_bytes());
        by_name.then(blanks[*one].cmp(&blanks[*other]))
    });
    let ties = |one: usize, other: usize| members[one].name == members[other].name && blanks[one] == blanks[other];
    let tied = order.windows(2).any(|pair| ties(pair[0], pair[1]));
    if tied {
        return Err(DescriptionFault::Tied);
    }

    let mut positions = alloc::vec![0; count];
    for (position, listed) in order.iter().enumerate() {
        positions[*listed] = position;
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
    let layout = Fnv::new().byte(CYCLE).hash(cycle).count(positions[this as usize]).finish();

    Ok(KindId::from_layout(&members[this as usize].name, layout))
}

/// The places of the members `schema`'s references name.
pub(super) fn named_members(schema: &SchemaNodes) -> Vec<u32> {
    let mut named = Vec::new();
    let nodes = schema.earlier().iter().chain(iter::once(schema.root()));
    for node in nodes {
        let SchemaNode::Reference { protocol, .. } = node else {
            continue;
        };
        let referents: Vec<Referent> = match protocol {
            ProtocolNode::Told(kinds) => kinds.clone(),
            ProtocolNode::Named(entries) => {
                entries.iter().flat_map(|entry| iter::once(entry.kind).chain(entry.answers.iter().copied())).collect()
            }
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
