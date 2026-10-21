//! Timing: a pass's timestamps, read as the GPU resolves them, into nanoseconds.

use super::*;

/// `ticks`, as the GPU resolves them: eight bytes each, little-endian.
fn resolved(ticks: &[u64]) -> Vec<u8> {
    ticks.iter().flat_map(|tick| tick.to_le_bytes()).collect()
}

#[test]
fn a_pass_takes_from_its_beginning_to_its_end_in_the_gpus_ticks() {
    // Catches stamps read in the other order of bytes, a pass's ends read the wrong way round, the period left out,
    // or passes paired across: two passes, one of 1500 ticks and one of 40, at two nanoseconds a tick.
    let stamps = resolved(&[1_000_000_000_000, 1_000_000_001_500, 7, 47]);

    assert_eq!(passes(&stamps, 2.0), [3000, 80]);
}

#[test]
fn a_pass_whose_end_reads_before_its_beginning_took_nothing() {
    // Catches a counter reset between a pass's stamps answered as a pass of nearly 2^64 ticks.
    assert_eq!(passes(&resolved(&[500, 100]), 1.0), [0]);
}
