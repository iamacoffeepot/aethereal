//! The routes, compiled to a trie of their patterns' segments, each route's value at the node its pattern ends on.

use super::pattern::{Pattern, Segment};
use super::walk::{Cursor, Next, Rest, Step};
use super::{Captures, Unroutable};
use crate::Method;

/// The routes, by method and pattern, each holding a value of its user's: a trie of their patterns' segments, its
/// nodes in one arena, so a request is one walk down it, with no recursion.
#[derive(Debug)]
pub struct Trie<R> {
    /// The nodes; the root, before any segment, is the first.
    nodes: Vec<Node<R>>,
}

/// Where the segments walked so far lead.
#[derive(Debug)]
struct Node<R> {
    /// The next segments, each to its node.
    children: Vec<Child>,
    /// The routes whose patterns end here, for distinct methods.
    routes: Vec<Route<R>>,
}

/// A segment, and the node it leads to.
#[derive(Debug)]
struct Child {
    edge: Edge,
    node: usize,
}

/// A segment of a pattern, as the trie keys it: a capture's name is its route's, so two captures are one edge.
#[derive(Debug)]
enum Edge {
    Literal(String),
    Capture,
}

/// One route, at the node its pattern ends on.
#[derive(Debug)]
struct Route<R> {
    method: Method,
    /// As it was given, to name the route it is in a refusal.
    pattern: String,
    /// Where each of its captures is among its segments, in order.
    captures: Vec<usize>,
    value: R,
}

/// What a method and a path matched.
#[derive(Debug, PartialEq, Eq)]
pub enum Routed<T> {
    /// The one route for the method.
    Found(T),
    /// No route matches the path: `404`.
    NotFound,
    /// Routes match the path, but none for the method: `405`, its `Allow` naming these methods, in their tokens'
    /// order.
    Unallowed(Vec<Method>),
}

/// The route a method and a path found: its value, and the slices of the path its captures matched, in the order of
/// its pattern.
#[derive(Debug, PartialEq, Eq)]
pub struct Found<'t, 'p, R> {
    pub route: &'t R,
    pub slices: Vec<&'p str>,
}

/// The root's place in the arena.
const ROOT: usize = 0;

impl<R> Default for Trie<R> {
    fn default() -> Self {
        Self { nodes: vec![Node::default()] }
    }
}

impl<R> Default for Node<R> {
    fn default() -> Self {
        Self { children: Vec::new(), routes: Vec::new() }
    }
}

impl<R> Trie<R> {
    /// The trie with a route for `method` on `pattern`, holding `value`, its captures read as `C`. Refused
    /// `Malformed` when the pattern is, `Uncaptured` when its `{names}` are not `C::NAMES`, and `Overlaps` when it
    /// matches a path another route for `method` does: the same pattern, or one with a literal or a capture where
    /// this one has a capture, or a capture where this one has a literal. Whether a set of routes is refused does not
    /// depend on their order.
    pub fn route<C: Captures>(mut self, method: Method, pattern: &str, value: R) -> Result<Self, Unroutable> {
        let read = Pattern::read(pattern)?;
        if !read.captures_are(C::NAMES) {
            return Err(Unroutable::Uncaptured { pattern: pattern.to_owned() });
        }

        let overlapped = self.routes_along(read.segments()).find(|route| route.method == method);
        if let Some(other) = overlapped {
            return Err(Unroutable::Overlaps { pattern: pattern.to_owned(), other: other.pattern.clone() });
        }

        let end = read.segments().iter().fold(ROOT, |node, segment| self.child(node, segment));
        let route = Route { method, pattern: pattern.to_owned(), captures: read.capture_positions(), value };
        self.nodes[end].routes.push(route);

        Ok(self)
    }

    /// The route `method` and `path` match, as sent: the path is not normalised, so `//` is an empty segment, and a
    /// percent-encoded byte is its three characters. Routes never overlap, so at most one is found.
    #[must_use]
    pub fn find<'p>(&self, method: Method, path: &'p str) -> Routed<Found<'_, 'p, R>> {
        let Some(segments) = path.strip_prefix('/') else {
            return Routed::NotFound;
        };

        let rest = Rest::Segments(segments);
        self.routes_along(rest).find(|route| route.method == method).map_or_else(
            || self.unallowed(rest),
            |route| Routed::Found(Found { route: &route.value, slices: route.slices(segments) }),
        )
    }

    /// What a path no route for its method matches is: the methods of the routes it does match, or not found.
    fn unallowed<'p>(&self, rest: Rest<'_>) -> Routed<Found<'_, 'p, R>> {
        let mut methods: Vec<Method> = self.routes_along(rest).map(|route| route.method).collect();
        methods.sort_unstable_by_key(|method| method.token());

        if methods.is_empty() {
            Routed::NotFound
        } else {
            Routed::Unallowed(methods)
        }
    }

    /// The routes at every node a walk of `from` ends on: those whose patterns match every path it does.
    fn routes_along<C: Cursor>(&self, from: C) -> impl Iterator<Item = &Route<R>> {
        Ends { nodes: &self.nodes, stack: vec![(ROOT, from)] }.flat_map(|node| &node.routes)
    }

    /// The node `segment` leads to from `from`, added when there is none.
    fn child(&mut self, from: usize, segment: &Segment<'_>) -> usize {
        let existing = self.nodes[from].children.iter().find(|child| child.edge.is(segment)).map(|child| child.node);

        existing.unwrap_or_else(|| {
            let node = self.nodes.len();
            self.nodes.push(Node::default());
            self.nodes[from].children.push(Child { edge: Edge::of(segment), node });
            node
        })
    }
}

impl<T> Routed<T> {
    /// What `read` makes of the route found, such as its captures read as their type: a route it makes nothing of is
    /// not found, as though no route matched the path (`404`).
    #[must_use]
    pub fn read<U>(self, read: impl FnOnce(T) -> Option<U>) -> Routed<U> {
        match self {
            Self::Found(found) => read(found).map_or(Routed::NotFound, Routed::Found),
            Self::NotFound => Routed::NotFound,
            Self::Unallowed(methods) => Routed::Unallowed(methods),
        }
    }
}

impl<R> Route<R> {
    /// The slices of `segments`, a path after its first `/`, that this route's captures matched.
    fn slices<'p>(&self, segments: &'p str) -> Vec<&'p str> {
        segments.split('/').enumerate().filter(|(at, _)| self.captures.contains(at)).map(|(_, slice)| slice).collect()
    }
}

impl Edge {
    fn of(segment: &Segment<'_>) -> Self {
        match *segment {
            Segment::Literal(text) => Self::Literal(text.to_owned()),
            Segment::Capture(_) => Self::Capture,
        }
    }

    /// Whether this is the edge `segment` is keyed as.
    fn is(&self, segment: &Segment<'_>) -> bool {
        match (self, *segment) {
            (Self::Literal(literal), Segment::Literal(text)) => literal == text,
            (Self::Capture, Segment::Capture(_)) => true,
            (Self::Literal(_), Segment::Capture(_)) | (Self::Capture, Segment::Literal(_)) => false,
        }
    }

    /// Whether a segment that `step` takes may pass this edge: a literal takes its own text, and a capture any
    /// segment that is not empty.
    fn meets(&self, step: Step<'_>) -> bool {
        match (self, step) {
            (Self::Literal(literal), Step::Text(text)) => literal == text,
            (Self::Literal(literal), Step::Any) => !literal.is_empty(),
            (Self::Capture, Step::Text(text)) => !text.is_empty(),
            (Self::Capture, Step::Any) => true,
        }
    }
}

/// The nodes a walk ends on, each segment taking every edge it meets: a work stack of nodes and where the cursor is
/// at each, bounded by the trie's nodes, as a walk reaches each node by one way at most.
struct Ends<'t, R, C> {
    nodes: &'t [Node<R>],
    stack: Vec<(usize, C)>,
}

impl<'t, R, C: Cursor> Iterator for Ends<'t, R, C> {
    type Item = &'t Node<R>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some((at, cursor)) = self.stack.pop() {
            let node = &self.nodes[at];
            match cursor.next() {
                Next::Segment(step, after) => {
                    let met =
                        node.children.iter().filter(|child| child.edge.meets(step)).map(|child| (child.node, after));
                    self.stack.extend(met);
                }
                Next::End => return Some(node),
            }
        }

        None
    }
}
