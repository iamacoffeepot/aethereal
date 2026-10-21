//! The order frames run in: each after the frames it reads, and what reads round is found.

use super::*;

#[test]
fn a_frame_runs_after_every_frame_it_reads() {
    // Catches frames run in the order they were made, or read before they are drawn: a minimap read by a frame made
    // before it would show what it drew a `Render` ago.
    let reads = [(1, 3), (3, 4), (2, 4)];

    assert_eq!(ordered(&[1, 2, 3, 4], &reads), [4, 2, 3, 1]);
}

#[test]
fn frames_that_read_nothing_of_each_other_run_in_the_order_of_their_ids() {
    // Catches an order that hangs on how the frames were listed, which would change from one `Render` to the next.
    assert_eq!(ordered(&[3, 1, 2], &[]), [1, 2, 3]);
    assert_eq!(ordered(&[2, 1], &[(2, 1), (5, 2)]), [1, 2]);
}

#[test]
fn reading_round_is_found_through_every_frame_between() {
    // Catches a placement let through that has a frame read what reads it, through frames between or at once, or
    // itself: the frames would each read what the other drew a `Render` ago.
    let reads = [(1, 2), (2, 3), (4, 1)];

    assert!(reaches(&reads, 1, 3));
    assert!(reaches(&reads, 4, 3));
    assert!(reaches(&reads, 2, 2));
    assert!(!reaches(&reads, 3, 1));
    assert!(!reaches(&reads, 1, 4));
}
