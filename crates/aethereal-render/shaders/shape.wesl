// The smooth step from `a` to `b` at `x`: 0 at or below `a`, 1 at or above `b`, and the cubic `t²(3 - 2t)` between.
public fn smooth_step(a: f32, b: f32, x: f32) -> f32 {
    let t = clamp((x - a) / (b - a), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// The smaller of `a` and `b`, rounded where they are within `k` of each other: `min(a, b) - h²k / 4`, with `h` how far
// within `k` they are, as a fraction of `k`. It is at most `k / 4` lower, where they are equal.
public fn smooth_min(a: f32, b: f32, k: f32) -> f32 {
    let h = max(k - abs(a - b), 0.0) / k;
    return min(a, b) - h * h * k / 4.0;
}
