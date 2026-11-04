//! `record_clip`'s own arithmetic: each frame's delta, which must sum to the clip's length exactly, and what is
//! refused before any frame is drawn.

use super::*;

#[test]
fn the_deltas_of_a_clip_s_frames_sum_to_its_length_exactly() {
    // Catches a delta of `1/fps` rounded once, whose rounding a frame drifts the clip short, and deltas that do not
    // sum to the time the frames cover at a rate that does not divide a second.
    let minute: u64 = (0..MOST_FRAMES).map(|frame| delta_nanos(frame, 60)).sum();
    assert_eq!(minute, 60 * SECOND_NANOS);

    let sevenths: Vec<u64> = (0..7).map(|frame| delta_nanos(frame, 7)).collect();
    assert_eq!(sevenths, [142_857_142, 142_857_143, 142_857_143, 142_857_143, 142_857_143, 142_857_143, 142_857_143]);
    assert_eq!(sevenths.iter().sum::<u64>(), SECOND_NANOS);
}

#[test]
fn a_clip_of_no_frames_too_many_or_at_no_rate_is_refused_before_it_is_drawn() {
    // Catches a clip of no frames held forever, one past the most the tool bounds a disk's use by, and a rate of zero,
    // which divides by it.
    assert_eq!(unrecordable(0, 30, 512), Some("`frames` is 1 to 3600"));
    assert_eq!(unrecordable(MOST_FRAMES + 1, 30, 512), Some("`frames` is 1 to 3600"));
    assert_eq!(unrecordable(30, 0, 512), Some("`frames_per_second` is 1 to 240"));
    assert_eq!(unrecordable(30, 241, 512), Some("`frames_per_second` is 1 to 240"));
    assert_eq!(unrecordable(30, 30, 0), Some("`longest` is at least 1"));
    assert_eq!(unrecordable(MOST_FRAMES, 240, 1), None);
}
