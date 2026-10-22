// Keeps the light of the stars drawn small for the glow that rises past a threshold, easing in over a soft knee so
// the glow grows from the bright parts without an edge.

@group(0) @binding(0) var stars: texture_2d<f32>;

// The light past which a pixel glows, and the width of the knee it eases in over.
const THRESHOLD: f32 = 0.2;
const KNEE: f32 = 0.3;

@fragment
fn bright(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let light = textureLoad(stars, vec2<i32>(at.xy), 0).rgb;
    let peak = max(light.r, max(light.g, light.b));
    let eased = clamp(peak - THRESHOLD + KNEE, 0.0, 2.0 * KNEE);
    let kept = max(eased * eased / (4.0 * KNEE), peak - THRESHOLD) / max(peak, 0.0001);
    return vec4(light * kept, 1.0);
}
