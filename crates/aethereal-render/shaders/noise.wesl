import package::hash::hash;

// Value noise at `(x, z)` in stream `k` of the seed: the hashes of the four whole cells around the point, blended by
// the smooth step of its place in the cell, from -1 to 1.
public fn noise(x: f32, z: f32, k: i32, seed: i32) -> f32 {
    let fi = floor(x);
    let fj = floor(z);
    let i = i32(fi);
    let j = i32(fj);
    let fx = x - fi;
    let fz = z - fj;
    let u = fx * fx * (3.0 - 2.0 * fx);
    let v = fz * fz * (3.0 - 2.0 * fz);
    let near = mix(hash(i, j, k, seed), hash(i + 1, j, k, seed), u);
    let far = mix(hash(i, j + 1, k, seed), hash(i + 1, j + 1, k, seed), u);
    return mix(near, far, v) * 2.0 - 1.0;
}

// Two octaves of the noise: the first at `(x, z)` in stream `k`, the second at half its weight, at twice the
// frequency, moved off by `(17.3, -9.1)`, in stream `k + 1`; their sum over 1.5, so it too runs from -1 to 1.
public fn fbm(x: f32, z: f32, k: i32, seed: i32) -> f32 {
    return (noise(x, z, k, seed) + 0.5 * noise(2.0 * x + 17.3, 2.0 * z - 9.1, k + 1, seed)) / 1.5;
}
