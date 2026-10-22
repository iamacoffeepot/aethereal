// A Gaussian blur, in two passes: `across` blurs along each row, and `down` along each column of what `across` left,
// each over twenty-seven texels. `down` writes the frame's end, a texture of bytes, so it keeps the glow in range by
// mapping it from zero up to one and spends its steps where the eye sees them, on the faint glow, by storing the
// square root, dithered so the steps do not band; `composite` maps it back.

@group(0) @binding(0) var light: texture_2d<f32>;

// The texels read on each side of the middle, and the kernel's deviation, in texels.
const REACH: i32 = 13;
const DEVIATION: f32 = 5.5;

// The light at `at`, held to the edge of the target.
fn texel(at: vec2<i32>) -> vec3<f32> {
    let last = vec2<i32>(textureDimensions(light)) - 1;
    return textureLoad(light, clamp(at, vec2(0), last), 0).rgb;
}

// The light blurred about `at` along `direction`, a texel's step.
fn blurred(at: vec2<i32>, direction: vec2<i32>) -> vec3<f32> {
    var sum = vec3(0.0);
    var weights = 0.0;
    for (var offset = -REACH; offset <= REACH; offset++) {
        let weight = exp(-f32(offset * offset) / (2.0 * DEVIATION * DEVIATION));
        sum += texel(at + direction * offset) * weight;
        weights += weight;
    }
    return sum / weights;
}

@fragment
fn across(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4(blurred(vec2<i32>(at.xy), vec2(1, 0)), 1.0);
}

@fragment
fn down(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let glow = blurred(vec2<i32>(at.xy), vec2(0, 1));
    let noise = fract(sin(dot(at.xy, vec2(12.9898, 78.233))) * 43758.5453) - 0.5;
    return vec4(sqrt(glow / (1.0 + glow)) + noise / 255.0, 1.0);
}
