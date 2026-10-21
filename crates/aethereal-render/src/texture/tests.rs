//! A texture's check: its size and its bytes.

use super::*;

#[test]
fn a_texture_is_made_of_the_bytes_its_size_and_format_take() {
    // Catches a check that counts pixels for bytes, or a format's bytes wrong: the upload would read past the
    // pixels, or leave rows unwritten.
    let rgba = check(3, 2, TextureFormat::Rgba8, &[0; 24], 8192);
    let srgb = check(3, 2, TextureFormat::Rgba8Srgb, &[0; 24], 8192);
    let grey = check(3, 2, TextureFormat::R8, &[0; 6], 8192);
    let short = check(3, 2, TextureFormat::Rgba8, &[0; 6], 8192);

    assert_eq!([rgba, srgb, grey], [Ok(()), Ok(()), Ok(())]);
    assert_eq!(short, Err(TextureRefused::Pixels { bytes: 6, expected: 24 }));
}

#[test]
fn a_side_of_nothing_or_past_the_gpus_longest_is_refused() {
    // Catches a check that lets an empty texture through, which WebGPU refuses, or one longer than the GPU makes.
    let empty = check(0, 2, TextureFormat::R8, &[], 8192);
    let longest = check(8192, 1, TextureFormat::R8, &vec![0; 8192], 8192);
    let past = check(1, 8193, TextureFormat::R8, &vec![0; 8193], 8192);

    assert_eq!(empty, Err(TextureRefused::Size { width: 0, height: 2, most: 8192 }));
    assert_eq!(longest, Ok(()));
    assert_eq!(past, Err(TextureRefused::Size { width: 1, height: 8193, most: 8192 }));
}
