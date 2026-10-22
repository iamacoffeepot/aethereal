// Keeps the part of the light traced small for the glow that rises past a threshold, easing in over a soft knee, so
// the glow grows from the bright disk and the photon ring without an edge. A camera's lens and the eye both scatter a
// little of a bright source's light around it; this is that scatter's source.

@group(0) @binding(0) var light: texture_2d<f32>;

// The light past which a pixel glows, and the width of the knee it eases in over.
const THRESHOLD: f32 = 0.9;
const KNEE: f32 = 0.6;

@fragment
fn bright(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let seen = textureLoad(light, vec2<i32>(at.xy), 0).rgb;
    let peak = max(seen.r, max(seen.g, seen.b));
    let eased = clamp(peak - THRESHOLD + KNEE, 0.0, 2.0 * KNEE);
    let kept = max(eased * eased / (4.0 * KNEE), peak - THRESHOLD) / max(peak, 0.0001);
    return vec4(seen * kept, 1.0);
}
