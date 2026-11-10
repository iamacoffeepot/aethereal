//! A clip's encoder and the MP4 it writes, read back by a player's parser and decoded by `OpenH264`'s decoder.

use super::{ClipEncoder, Unencodable};
use mp4::{Mp4Reader, TrackType};
use openh264::decoder::Decoder;
use openh264::formats::YUVSource;
use std::io::Cursor;

/// A frame `width` by `height` of one grey, `level`, with a white square in its top left so it is not flat.
fn frame(width: u32, height: u32, level: u8) -> Vec<u8> {
    let (width, height) = (width as usize, height as usize);
    let mut rgb = vec![level; width * height * 3];
    for row in rgb.chunks_exact_mut(width * 3).take(height / 4) {
        row[..width / 4 * 3].fill(u8::MAX);
    }

    rgb
}

/// The MP4 of a clip of `levels.len()` frames at `frames_per_second`, each `width` by `height`, frame `i` grey
/// `levels[i]`.
fn clip(frames_per_second: u32, width: u32, height: u32, levels: &[u8]) -> Result<Vec<u8>, Unencodable> {
    let mut encoder = ClipEncoder::new(frames_per_second, width, height)?;
    for (at, &level) in (0..).zip(levels) {
        encoder.encode(at, &frame(width, height, level))?;
    }

    encoder.finish()
}

/// The mean of a decoded picture's luma, from its top right quarter onward, away from the square.
fn mean_luma(picture: &impl YUVSource) -> u64 {
    let (width, height) = picture.dimensions();
    let stride = picture.strides().0;
    let rows = picture.y().chunks(stride).take(height).skip(height / 2);
    let (sum, count) = rows.fold((0u64, 0u64), |(sum, count), row| {
        let row = &row[width / 2..width];
        (sum + row.iter().map(|&byte| u64::from(byte)).sum::<u64>(), count + row.len() as u64)
    });

    sum / count.max(1)
}

#[test]
fn a_clip_s_mp4_holds_one_track_of_its_frames_at_its_rate_and_size_and_each_decodes_in_order() {
    // Catches an MP4 a player cannot parse (a box's size or field out of place), a track of the wrong size, rate or
    // count, an `avcC` whose parameter sets do not decode the frames, a frame's NAL units written without their
    // lengths, and frames written in another order than encoded, which the rising greys would show.
    let levels = [16, 60, 100, 140, 180, 220];
    let bytes = clip(30, 64, 48, &levels).expect("the clip encodes");

    let size = bytes.len() as u64;
    let mut reader = Mp4Reader::read_header(Cursor::new(bytes), size).expect("a player parses it");
    assert_eq!(reader.tracks().len(), 1);
    let track = reader.tracks().values().next().expect("one track");
    let id = track.track_id();
    assert_eq!(track.track_type().expect("its type"), TrackType::Video);
    assert_eq!((track.width(), track.height()), (64, 48));
    assert_eq!(track.sample_count(), 6);
    assert_eq!(track.timescale(), 30);
    assert!((track.frame_rate() - 30.0).abs() < 1e-9, "{}", track.frame_rate());
    assert_eq!(reader.duration().as_millis(), 200);

    let mut stream = Vec::new();
    for set in [track.sequence_parameter_set().expect("an SPS"), track.picture_parameter_set().expect("a PPS")] {
        stream.extend_from_slice(&[0, 0, 0, 1]);
        stream.extend_from_slice(set);
    }
    let mut decoder = Decoder::new().expect("a decoder");
    let mut lumas = Vec::new();
    for at in 1..=6 {
        let sample = reader.read_sample(id, at).expect("a sample").expect("the sample is there");
        assert_eq!(sample.is_sync, at == 1, "only the first frame is one a player may start at");
        let mut rest = &sample.bytes[..];
        while let [a, b, c, d, tail @ ..] = rest {
            let length = u32::from_be_bytes([*a, *b, *c, *d]) as usize;
            stream.extend_from_slice(&[0, 0, 0, 1]);
            stream.extend_from_slice(&tail[..length]);
            rest = &tail[length..];
        }
        let picture = decoder.decode(&stream).expect("the frame decodes");
        stream.clear();
        if let Some(picture) = picture {
            assert_eq!(picture.dimensions(), (64, 48));
            lumas.push(mean_luma(&picture));
        }
    }
    for picture in decoder.flush_remaining().expect("the decoder flushes") {
        lumas.push(mean_luma(&picture));
    }

    assert_eq!(lumas.len(), 6, "{lumas:?}");
    assert!(lumas.is_sorted(), "the frames decode in the order encoded: {lumas:?}");
}

#[test]
fn a_frame_is_encoded_only_as_the_clip_s_next() {
    // Catches a frame encoded out of its place, or twice, which would play the clip out of order or stretch it: the
    // encoder is given one frame at a time, and refuses any but the next.
    let mut encoder = ClipEncoder::new(60, 16, 16).expect("an encoder");
    let rgb = frame(16, 16, 128);

    assert_eq!(encoder.encode(1, &rgb), Err(Unencodable::OutOfOrder { frame: 1, next: 0 }));
    assert_eq!(encoder.encode(0, &rgb), Ok(()));
    assert_eq!(encoder.encode(0, &rgb), Err(Unencodable::OutOfOrder { frame: 0, next: 1 }));
    assert_eq!(encoder.encode(1, &rgb), Ok(()));
}

#[test]
fn a_clip_of_no_frames_is_refused_rather_than_written_undecodable() {
    // Catches an MP4 written with an empty `avcC`, which no player decodes.
    let encoder = ClipEncoder::new(60, 16, 16).expect("an encoder");

    assert_eq!(encoder.finish().err(), Some(Unencodable::NoParameterSets));
}
