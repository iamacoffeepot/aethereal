use crate::{Clock, Engine};
use aethereal_actor::{Against, Time};
use std::num::NonZeroUsize;

/// A clock that never moves.
struct Stopped;

impl Clock for Stopped {
    fn nanos(&self) -> u64 {
        0
    }
}

#[test]
fn the_blob_figure_tracks_puts_and_frees_against_the_budget() {
    // Catches a figure that counts what was put but not what was freed, reads the budget's room for what is held, or
    // reads one engine's store for another's.
    let parts = NonZeroUsize::new(4).unwrap_or(NonZeroUsize::MIN);
    let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0).ready();
    let Ok(kept) = engine.put_blob(vec![1, 2, 3]) else {
        panic!("three bytes fit");
    };
    let Ok(dropped) = engine.put_blob(vec![4; 5]) else {
        panic!("five bytes fit");
    };
    let both = engine.engine_stats().blobs;

    drop(dropped);
    engine.step(Time::ZERO, &Stopped);
    let one = engine.engine_stats().blobs;

    assert_eq!((both, one), (Against { bytes: 8, most: 1 << 20 }, Against { bytes: 3, most: 1 << 20 }));
    assert!(engine.blob(&kept).is_some());
}
