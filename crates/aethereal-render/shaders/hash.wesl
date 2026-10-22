// The integer hash of a cell `(i, j, k)` and a seed to a fraction in [0, 1). It mixes as JavaScript's `Math.imul`
// mixes, on the same 32 bits, so a port of a generator written in JavaScript matches it bit for bit. The fraction is
// the top 24 bits, exact in a 32-bit float.
public fn hash(i: i32, j: i32, k: i32, seed: i32) -> f32 {
    var h = bitcast<u32>(i) * 374761393u + bitcast<u32>(j) * 668265263u + bitcast<u32>(k) * 1274126177u
        + bitcast<u32>(seed) * 1442695041u;
    h = (h ^ (h >> 13u)) * 1274126177u;
    h = h ^ (h >> 16u);
    return f32(h >> 8u) / 16777216.0;
}
