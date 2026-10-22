// Lays the glow over the stars, maps the light down to the screen's range with a filmic curve, darkens the corners a
// little, and dithers the result so the faint glow does not band. `shown` writes to a window whose format encodes
// sRGB itself; `encoded` writes to one that does not, and encodes it.

@group(0) @binding(0) var stars: texture_2d<f32>;
@group(1) @binding(0) var glow: texture_2d<f32>;

// How much the glow adds, and the exposure the whole is mapped at.
const GLOW: f32 = 1.8;
const EXPOSURE: f32 = 1.0;
// How dark the corners fall, and the faint blue of the space behind the disc.
const VIGNETTE: f32 = 0.38;
const SPACE: vec3<f32> = vec3(0.004, 0.006, 0.014);

// A filmic curve from light of any brightness down to the screen's range (Narkowicz's fit of ACES).
fn filmic(light: vec3<f32>) -> vec3<f32> {
    let mapped = (light * (2.51 * light + 0.03)) / (light * (2.43 * light + 0.59) + 0.14);
    return clamp(mapped, vec3(0.0), vec3(1.0));
}

// Noise of up to half a step of a byte, different at each pixel.
fn dither(at: vec2<f32>) -> vec3<f32> {
    let noise = fract(sin(dot(at, vec2(12.9898, 78.233))) * 43758.5453);
    return vec3((noise - 0.5) / 255.0);
}

// The glow at `uv`, blended between its four nearest texels: it is smaller than the window, and read in its place.
fn glow_at(uv: vec2<f32>) -> vec3<f32> {
    let size = vec2<f32>(textureDimensions(glow));
    let at = uv * size - 0.5;
    let base = vec2<i32>(floor(at));
    let between = fract(at);
    let last = vec2<i32>(size) - 1;
    let low = mix(glow_texel(base, last), glow_texel(base + vec2(1, 0), last), between.x);
    let high = mix(glow_texel(base + vec2(0, 1), last), glow_texel(base + vec2(1, 1), last), between.x);
    return mix(low, high, between.y);
}

// The glow's texel at `at`, held within `last`, its last texel.
fn glow_texel(at: vec2<i32>, last: vec2<i32>) -> vec3<f32> {
    return textureLoad(glow, clamp(at, vec2(0), last), 0).rgb;
}

// The picture at `at`, `uv` across it, in linear light from zero to one.
fn picture(at: vec4<f32>, uv: vec2<f32>) -> vec3<f32> {
    let light = textureLoad(stars, vec2<i32>(at.xy), 0).rgb;
    let stored = glow_at(uv);
    // The glow was kept in range by mapping it from zero up to one and storing its square root; this maps it back.
    let mapped = stored * stored;
    let glowing = mapped / max(1.0 - mapped, vec3(0.004));
    let off_centre = uv - 0.5;
    let space = SPACE * (1.0 - 1.4 * dot(off_centre, off_centre));
    let mapped_light = filmic((light + glowing * GLOW + space) * EXPOSURE);
    let vignette = 1.0 - VIGNETTE * smoothstep(0.15, 0.75, length(off_centre * vec2(1.0, 0.8)));
    return mapped_light * vignette;
}

// The sRGB encoding of linear `light`.
fn encode(light: vec3<f32>) -> vec3<f32> {
    let low = light * 12.92;
    let high = 1.055 * pow(light, vec3(1.0 / 2.4)) - 0.055;
    return select(high, low, light <= vec3(0.0031308));
}

@fragment
fn shown(@builtin(position) at: vec4<f32>, @location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    return vec4(picture(at, uv) + dither(at.xy) * 0.08, 1.0);
}

@fragment
fn encoded(@builtin(position) at: vec4<f32>, @location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    return vec4(encode(picture(at, uv)) + dither(at.xy), 1.0);
}
