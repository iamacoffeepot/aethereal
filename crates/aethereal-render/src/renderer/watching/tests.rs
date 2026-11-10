//! What the renderer counts for its subscribers between frames: nothing while it has none.

use super::*;

#[test]
fn nothing_is_counted_while_no_one_is_sent_statistics_and_each_frame_counts_from_the_last() {
    // Catches bytes written while no one watched counted, so a frame's statistics once someone subscribes tell of
    // writes from before, a frame recorded unwatched counting its draws, and bytes counted into one frame counted
    // again into the next.
    let mut watch = Watch::Unwatched;
    watch.uploaded(4096);
    assert_eq!(watch.tally(), Tally::Off);
    assert!(matches!(watch, Watch::Unwatched));

    let mut watch = Watch::Watched { subscribers: Subscribers::new(), uploaded_bytes: 0 };
    watch.uploaded(64);
    watch.uploaded(16);
    let first = watch.tally();
    let second = watch.tally();

    let counted = |uploaded_bytes| FrameDrawn { draws: 0, triangles: 0, uploaded_bytes };
    assert_eq!((first, second), (Tally::On(counted(80)), Tally::On(counted(0))));
}
