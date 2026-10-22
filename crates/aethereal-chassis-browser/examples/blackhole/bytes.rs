//! The bytes of what is put as a blob: words, and floats.

/// The bytes of `values`, as a blob is put.
pub fn words(values: impl IntoIterator<Item = u32>) -> Vec<u8> {
    values.into_iter().flat_map(u32::to_ne_bytes).collect()
}

/// The bytes of `values`, floats, as a blob is put.
pub fn floats(values: impl IntoIterator<Item = f32>) -> Vec<u8> {
    words(values.into_iter().map(f32::to_bits))
}
