//! `record_clip`'s own arithmetic: each frame's delta, which must sum to the clip's length exactly, what is refused
//! before any frame is drawn, and each frame's size rounded down to even.

use super::*;
use aethereal_tools::{CommandLine, FlagsFault};

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
    // Catches a clip of no frames held forever, one past the most the tool bounds its memory by, a rate of zero, which
    // divides by it, and a `longest` that rounds to no pixels at all.
    assert_eq!(unrecordable(0, 30, 512), Some("`frames` is 1 to 3600"));
    assert_eq!(unrecordable(MOST_FRAMES + 1, 30, 512), Some("`frames` is 1 to 3600"));
    assert_eq!(unrecordable(30, 0, 512), Some("`frames_per_second` is 1 to 240"));
    assert_eq!(unrecordable(30, 241, 512), Some("`frames_per_second` is 1 to 240"));
    assert_eq!(unrecordable(30, 30, 1), Some("`longest` is at least 2"));
    assert_eq!(unrecordable(MOST_FRAMES, 240, 2), None);
}

#[test]
fn every_sheet_s_flags_fit_its_input() {
    // Catches a `#[flag]` an `Offer` refuses (`Offered::BadFlags`): the tool would be missing from a running engine,
    // and nothing would answer when it is called.
    let sheets = RecordTool::sheets();
    let refused: Vec<(&str, FlagsFault)> = sheets
        .iter()
        .filter_map(|sheet| CommandLine::of(sheet).err().map(|fault| (sheet.name.as_str(), fault)))
        .collect();

    assert!(refused.is_empty(), "{refused:?}");
}

#[test]
fn each_side_of_a_clip_s_frame_is_rounded_down_to_even_trimmed_when_unshrunk() {
    // Catches an odd side handed to the encoder, which YUV 4:2:0 cannot hold; a frame of its own size resampled for one
    // pixel, which blurs every line of text in it; a shrunk frame cut rather than shrunk to the even size; and a frame
    // rounded to no pixels encoded.
    let odd = Region { x: 3, y: 5, width: 1921, height: 1081 };
    let trimmed = Region { x: 3, y: 5, width: 1920, height: 1080 };
    assert_eq!(clip_cut(odd, u32::MAX), Ok(Cut { region: trimmed, size: ClipSize { width: 1920, height: 1080 } }));

    let wide = Region { x: 0, y: 0, width: 1000, height: 502 };
    assert_eq!(clip_cut(wide, 500), Ok(Cut { region: wide, size: ClipSize { width: 500, height: 250 } }));

    let even = Region { x: 0, y: 0, width: 800, height: 600 };
    assert_eq!(clip_cut(even, u32::MAX), Ok(Cut { region: even, size: ClipSize { width: 800, height: 600 } }));

    let thin = Region { x: 0, y: 0, width: 1, height: 400 };
    assert!(clip_cut(thin, u32::MAX).is_err());
}
