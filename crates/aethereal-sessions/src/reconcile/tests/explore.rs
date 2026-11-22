//! The explorer: every state the actions reach from a new session, breadth first, each known by the fingerprint of its
//! canonical form, with the invariants checked in each; then, over the states reached, the check that no fault or ask
//! is needed to settle: from every state, every path of calm actions reaches a quiet one, and none loops.
//!
//! From every state reached in full, the owner's stop and archive of each session are taken too, and the states they
//! lead to are explored by calm actions alone, so that from any reachable state each is shown to settle, whatever the
//! bounds allow the owner to ask besides.

use super::actions::{Action, Outcome, apply, effect, enabled};
use super::bounds::Bounds;
use super::checks;
use super::world::World;
use crate::reconcile::Asked;
use std::collections::{HashMap, VecDeque};
use std::fmt::Write as _;

/// What an exploration found.
#[derive(Debug, Clone, Copy)]
pub(super) struct Explored {
    pub states: usize,
    /// States one of whose calm actions would pass a bound, so whether they settle is not known.
    pub frontier: usize,
    pub quiet: usize,
}

/// How a state is explored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// By every action.
    Full,
    /// By calm actions alone: it was reached by an owner's stop or archive taken beyond the bounds' asks.
    Calm,
    /// By every action but the calm ones, which it was explored by already.
    Upgraded,
}

/// A state reached: its fingerprint, the state it was reached from, and how.
struct Node {
    fingerprint: u128,
    parent: u32,
    action: Action,
    mode: Mode,
    quiet: bool,
    frontier: bool,
}

/// Every state reached, and the calm actions between them.
struct Graph {
    index: HashMap<u128, u32>,
    nodes: Vec<Node>,
    /// The states each state's calm actions lead to.
    calm: Vec<Vec<u32>>,
    queue: VecDeque<(u32, World)>,
}

impl Graph {
    /// The state `world`, reached from `parent` by `action`, to be explored as `mode` says.
    fn reach(&mut self, world: World, parent: u32, action: Action, mode: Mode) -> u32 {
        let fingerprint = world.fingerprint();
        if let Some(&at) = self.index.get(&fingerprint) {
            let node = &mut self.nodes[at as usize];
            if node.mode == Mode::Calm && mode == Mode::Full {
                node.mode = Mode::Upgraded;
                self.queue.push_back((at, world));
            }
            return at;
        }
        let at = u32::try_from(self.nodes.len()).unwrap_or(u32::MAX);
        self.index.insert(fingerprint, at);
        self.nodes.push(Node { fingerprint, parent, action, mode, quiet: false, frontier: false });
        self.calm.push(Vec::new());
        self.queue.push_back((at, world));

        at
    }
}

/// How many kinds of violation an exploration reports.
const MOST_KINDS: usize = 16;

/// The kinds of violation found, each with the shortest path to it.
struct Found {
    kinds: Vec<(String, String)>,
}

impl Found {
    /// Keeps `trace` when `why` is a kind not found before; answers whether enough kinds are found to stop.
    fn note(&mut self, why: &str, trace: String) -> bool {
        let kind: String = why.lines().next().unwrap_or_default().chars().filter(|c| !c.is_ascii_digit()).collect();
        if !self.kinds.iter().any(|(seen, _)| *seen == kind) {
            self.kinds.push((kind, trace));
        }

        self.kinds.len() >= MOST_KINDS
    }

    fn report(&self) -> String {
        let traces: Vec<&str> = self.kinds.iter().map(|(_, trace)| trace.as_str()).collect();

        format!("{} kinds of violation:\n\n{}", self.kinds.len(), traces.join("\n"))
    }
}

/// Explores every state within `bounds`, or answers, for each kind of violation found, the shortest path to it.
pub(super) fn explore(bounds: &Bounds) -> Result<Explored, String> {
    let mut found = Found { kinds: Vec::new() };
    let mut graph = Graph { index: HashMap::new(), nodes: Vec::new(), calm: Vec::new(), queue: VecDeque::new() };
    let mut initial = World::new(bounds);
    initial.canonical(bounds.longest());
    graph.reach(initial, u32::MAX, Action::Tick, Mode::Full);

    while let Some((at, world)) = graph.queue.pop_front() {
        let index = at as usize;
        let mode = graph.nodes[index].mode;
        let actions = enabled(&world, bounds);
        if mode != Mode::Upgraded {
            if let Err(why) = checks::state(&world, bounds) {
                graph.nodes[index].frontier = true;
                if found.note(&why, trace(&graph.nodes, at, None, &why, bounds)) {
                    return Err(found.report());
                }
                continue;
            }
            let quiet = checks::quiet(&actions);
            graph.nodes[index].quiet = quiet;
            if quiet
                && let Err(why) = checks::settled(&world)
                && found.note(&why, trace(&graph.nodes, at, None, &why, bounds))
            {
                return Err(found.report());
            }
        }
        let quiet = graph.nodes[index].quiet;
        let fair = actions.iter().any(|action| action.fair());
        for action in actions {
            let calm = action.calm();
            let taken = match mode {
                Mode::Full => true,
                Mode::Calm => calm,
                Mode::Upgraded => !calm,
            };
            if !taken {
                continue;
            }
            match apply(&world, action, bounds) {
                Outcome::Violation(why) => {
                    graph.nodes[index].frontier |= calm;
                    if found.note(&why, trace(&graph.nodes, at, Some(action), &why, bounds)) {
                        return Err(found.report());
                    }
                }
                Outcome::Pruned => graph.nodes[index].frontier |= calm,
                Outcome::Next(mut next) => {
                    if action.ticks() {
                        next.canonical(bounds.longest());
                    }
                    let next_mode = if mode == Mode::Calm {
                        Mode::Calm
                    } else {
                        Mode::Full
                    };
                    let to = graph.reach(next, at, action, next_mode);
                    let waits = action == Action::Tick && to == at && fair;
                    if calm && !quiet && !waits {
                        graph.calm[index].push(to);
                    }
                }
            }
        }
        if mode == Mode::Calm || !bounds.sleeps {
            continue;
        }
        for session in 0..world.sessions.len() {
            let session = u8::try_from(session).unwrap_or(u8::MAX);
            for ask in [Asked::Stop, Asked::Archive] {
                let action = Action::Ask { session, ask };
                if let Outcome::Next(next) = apply(&world, action, bounds) {
                    graph.reach(next, at, action, Mode::Calm);
                }
            }
        }
    }

    if let Err(why) = settles(&graph, bounds) {
        found.note("loop", why);
    }
    if !found.kinds.is_empty() {
        return Err(found.report());
    }
    let frontier = graph.nodes.iter().filter(|node| node.frontier).count();
    let quiet = graph.nodes.iter().filter(|node| node.quiet).count();

    Ok(Explored { states: graph.nodes.len(), frontier, quiet })
}

/// Checks that every calm path from every state reaches a quiet one: none ends anywhere else, and none loops.
fn settles(graph: &Graph, bounds: &Bounds) -> Result<(), String> {
    let nodes = &graph.nodes;
    let calm = &graph.calm;
    let mut out: Vec<usize> = calm.iter().map(Vec::len).collect();
    let mut into: Vec<Vec<u32>> = vec![Vec::new(); nodes.len()];
    for (from, edges) in calm.iter().enumerate() {
        for &to in edges {
            into[to as usize].push(u32::try_from(from).unwrap_or(u32::MAX));
        }
    }
    let mut sinks: Vec<u32> = Vec::new();
    for (at, node) in nodes.iter().enumerate() {
        if out[at] > 0 {
            continue;
        }
        let at = u32::try_from(at).unwrap_or(u32::MAX);
        if !node.quiet && !node.frontier {
            return Err(trace(nodes, at, None, "stuck: no calm action moves this state, and it is not quiet", bounds));
        }
        sinks.push(at);
    }
    while let Some(sink) = sinks.pop() {
        for &from in &into[sink as usize] {
            out[from as usize] -= 1;
            if out[from as usize] == 0 {
                sinks.push(from);
            }
        }
    }
    let Some(start) = out.iter().position(|&left| left > 0) else {
        return Ok(());
    };
    let mut path: Vec<u32> = Vec::new();
    let mut at = u32::try_from(start).unwrap_or(u32::MAX);
    while !path.contains(&at) {
        path.push(at);
        let Some(&next) = calm[at as usize].iter().find(|&&to| out[to as usize] > 0) else {
            break;
        };
        at = next;
    }
    let looped = path.iter().position(|&node| node == at).unwrap_or(0);
    let cycle = &path[looped..];
    let mut why = String::from("a loop of calm actions never settles; it goes round:\n");
    for (step, &from) in cycle.iter().enumerate() {
        let to = cycle.get(step + 1).copied().unwrap_or(cycle[0]);
        let action = edge(nodes, from, to, bounds);
        let _ = writeln!(why, "      {action:?}");
    }

    Err(trace(nodes, cycle[0], None, &why, bounds))
}

/// The calm action that takes state `from` to state `to`.
fn edge(nodes: &[Node], from: u32, to: u32, bounds: &Bounds) -> Option<Action> {
    let world = replay(nodes, from, bounds);
    let target = nodes[to as usize].fingerprint;

    enabled(&world, bounds).into_iter().filter(|action| action.calm()).find(|&action| {
        match apply(&world, action, bounds) {
            Outcome::Next(mut next) => {
                if action.ticks() {
                    next.canonical(bounds.longest());
                }
                next.fingerprint() == target
            }
            Outcome::Pruned | Outcome::Violation(_) => false,
        }
    })
}

/// The actions from the first state to state `at`.
fn path(nodes: &[Node], at: u32) -> Vec<Action> {
    let mut path = Vec::new();
    let mut node = at;
    while node != 0 && node != u32::MAX {
        path.push(nodes[node as usize].action);
        node = nodes[node as usize].parent;
    }
    path.reverse();

    path
}

/// Takes the world from the first state along `actions`.
fn step(world: &World, action: Action, bounds: &Bounds) -> World {
    match apply(world, action, bounds) {
        Outcome::Next(mut next) => {
            if action.ticks() {
                next.canonical(bounds.longest());
            }
            next
        }
        Outcome::Pruned | Outcome::Violation(_) => world.clone(),
    }
}

/// State `at`, rebuilt from the first.
fn replay(nodes: &[Node], at: u32, bounds: &Bounds) -> World {
    let mut world = World::new(bounds);
    world.canonical(bounds.longest());
    for action in path(nodes, at) {
        world = step(&world, action, bounds);
    }

    world
}

/// The path to state `at`, then `action` when it broke the invariant, one action per line, each of the hub's with the
/// effect it performed.
fn trace(nodes: &[Node], at: u32, action: Option<Action>, why: &str, bounds: &Bounds) -> String {
    let mut actions = path(nodes, at);
    actions.extend(action);
    let mut world = World::new(bounds);
    world.canonical(bounds.longest());
    let mut text = format!("{why}\nafter {} steps:\n", actions.len());
    for (number, action) in actions.iter().enumerate() {
        let done = match action {
            Action::Hub { session } | Action::Torn { session } => {
                format!("  => {:?}", effect(&world, usize::from(*session), bounds))
            }
            _ => String::new(),
        };
        let _ = writeln!(text, "{:>4}. {action:?}{done}", number + 1);
        world = step(&world, *action, bounds);
    }
    for (index, session) in world.sessions.iter().enumerate() {
        let _ = writeln!(text, "      session {index}: {:?}", session.record);
    }

    text
}
