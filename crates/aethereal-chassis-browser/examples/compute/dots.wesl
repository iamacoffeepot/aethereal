// Draws the quad, small, at each particle it reads from the field, by its instance, coloured by its place in the field
// and shaded by the shared two-octave noise where it is, so a dot's shade changes as it moves.
import aethereal_render::noise::fbm;

struct Particle { at: vec2<f32>, speed: vec2<f32> }

@group(0) @binding(0) var<storage, read> particles: array<Particle>;

struct Between {
    @builtin(position) at: vec4<f32>,
    @location(0) colour: vec4<f32>,
}

@vertex
fn place(@location(0) corner: vec2<f32>, @builtin(instance_index) at: u32) -> Between {
    let hue = f32(at) / f32(arrayLength(&particles));
    let here = particles[at].at;
    let tint = 0.6 + 0.4 * fbm(here.x * 3.0, here.y * 3.0, 0, 7);
    return Between(vec4(corner * 0.015 + here, 0.0, 1.0), vec4(hue * tint, 0.7 * tint, (1.0 - hue) * tint, 1.0));
}

@fragment
fn shade(between: Between) -> @location(0) vec4<f32> {
    return between.colour;
}
