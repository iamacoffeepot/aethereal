//! Storage's checks: as it is made, and as it is written.

use super::*;

#[test]
fn storage_is_whole_words_as_many_as_one_buffer_binds() {
    // Catches empty storage, which WebGPU binds as nothing, bytes no whole number of words, which it cannot write,
    // or more than one storage buffer binds.
    let made = check(64, 64);
    let empty = check(0, 64);
    let torn = check(6, 64);
    let past = check(68, 64);

    assert_eq!(made, Ok(64));
    assert_eq!(empty, Err(StorageRefused::Size { bytes: 0, most: 64 }));
    assert_eq!(torn, Err(StorageRefused::Size { bytes: 6, most: 64 }));
    assert_eq!(past, Err(StorageRefused::Size { bytes: 68, most: 64 }));
}

#[test]
fn storage_past_what_a_word_addresses_is_refused_naming_that_most() {
    // Catches a device that binds more than 4 GiB, as one asking the adapter's own limits may: storage past what its
    // `u32` offsets reach is refused, and the refusal names that most, never the device's larger one.
    let most = u64::from(u32::MAX) - 3;
    let binds = 8 << 30;

    assert_eq!(check(most, binds), Ok(u32::MAX - 3));
    assert_eq!(check(4 << 30, binds), Err(StorageRefused::Size { bytes: 4 << 30, most }));
}

#[test]
fn a_write_lies_inside_the_storage_on_whole_words() {
    // Catches a write past the buffer, which WebGPU refuses on the queue, or one off a word.
    assert!(fits(64, 0, 64));
    assert!(fits(64, 48, 16));
    assert!(!fits(64, 52, 16));
    assert!(!fits(64, 2, 4));
    assert!(!fits(64, 0, 6));
}
