//! The reconcile, explored: every interleaving of the owner's asks, the hub's effects and crashes, Docker's faults,
//! Claude Code's steps, and the clock, within bounds, with the invariants checked in every state reached and, over all
//! of them, the check that every state settles with no fault or ask.
//!
//! Each test explores one slice of the whole, small enough for a debug build: lines, the hub's crashes, faults, each of
//! the owner's asks, stops from every state, and two sessions under the most awake. `the_slices_together` explores
//! several at once, and is run by hand.

mod actions;
mod bounds;
mod checks;
mod claude;
mod explore;
mod world;

use bounds::{Bounds, HIBERNATE, INTERRUPT, STOP, WAKE};
use explore::{Explored, explore};

/// Explores `bounds`, failing with the shortest path to each kind of violation found.
fn holds(bounds: &Bounds) -> Explored {
    explore(bounds).unwrap_or_else(|report| panic!("{report}"))
}

#[test]
fn two_lines_reach_claude_once_each_in_order_whatever_ends_their_runs() {
    holds(&Bounds { lines: 2, ..Bounds::LINE });
}

#[test]
fn a_hub_that_stops_between_any_two_writes_or_in_the_middle_of_one_carries_on() {
    holds(&Bounds { hub_crashes: 2, runs: 3, ..Bounds::LINE });
}

#[test]
fn every_fault_of_docker_claude_or_the_bootstrap_is_rescued_or_held() {
    holds(&Bounds { faults: 1, hub_crashes: 1, ..Bounds::LINE });
}

#[test]
fn a_hibernate_and_a_wake_at_any_moment_settle() {
    holds(&Bounds { asks: HIBERNATE | WAKE, ..Bounds::LINE });
}

#[test]
fn a_stop_and_a_wake_at_any_moment_settle() {
    holds(&Bounds { asks: STOP | WAKE, ..Bounds::LINE });
}

#[test]
fn an_interrupt_or_a_background_task_at_any_moment_settles_across_a_hub_crash() {
    holds(&Bounds { asks: INTERRUPT, tasks: 1, hub_crashes: 1, runs: 3, ..Bounds::LINE });
}

#[test]
fn from_every_state_a_stop_or_an_archive_settles() {
    holds(&Bounds { sleeps: true, faults: 1, ..Bounds::LINE });
}

#[test]
fn two_sessions_never_wake_past_the_most_awake() {
    holds(&Bounds { sessions: 2, ..Bounds::LINE });
}

#[test]
#[ignore = "explores eleven million states, in minutes in release and some 6 GB: run by hand"]
fn the_slices_together() {
    let explored = holds(&Bounds { lines: 2, faults: 1, ..Bounds::LINE });

    #[allow(clippy::print_stderr, reason = "the larger bound reports how far it went, which --nocapture shows")]
    {
        eprintln!("explored states={} quiet={} frontier={}", explored.states, explored.quiet, explored.frontier);
    }
}
