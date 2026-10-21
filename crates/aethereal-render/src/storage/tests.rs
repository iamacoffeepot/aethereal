//! Storage's checks: as it is made, and as it is written.

use super::*;

#[test]
fn storage_is_whole_words_as_many_as_one_buffer_binds() {
    // Catches empty storage, which WebGPU binds as nothing, bytes no whole number of words, which it cannot write,
    // or more than one storage buffer binds.
    let made = check(&[0; 64], 64);
    let empty = check(&[], 64);
    let torn = check(&[0; 6], 64);
    let past = check(&[0; 68], 64);

    assert_eq!(made, Ok(64));
    assert_eq!(empty, Err(StorageRefused::Size { bytes: 0, most: 64 }));
    assert_eq!(torn, Err(StorageRefused::Size { bytes: 6, most: 64 }));
    assert_eq!(past, Err(StorageRefused::Size { bytes: 68, most: 64 }));
}

#[test]
fn a_write_lies_inside_the_storage_on_whole_words() {
    // Catches a write past the buffer, which WebGPU refuses on the queue, or one off a word.
    assert!(writes(64, 0, 64));
    assert!(writes(64, 48, 16));
    assert!(!writes(64, 52, 16));
    assert!(!writes(64, 2, 4));
    assert!(!writes(64, 0, 6));
}
