// Draws each star as a small soft sprite where the simulation left it, seen from above the disc and tilted away, its
// colour by its radius, its speed, and how near it lies to an arm: a warm core, cool blue arms, and the stars thrown
// fast by the pointer burning white. Sprites add their light, so crowds glow.

struct Star {
    position: vec2<f32>,
    velocity: vec2<f32>,
    orbit: f32,
    angle: f32,
    lift: f32,
    shine: f32,
}

struct View {
    // Where a step along the disc's two axes, and one up from its plane, lands on the screen.
    across: vec2<f32>,
    along: vec2<f32>,
    up: vec2<f32>,
    // The clip units of one of the screen's units, across and up.
    scale: vec2<f32>,
    // Half a sprite, in clip units across and up.
    sprite: vec2<f32>,
    // The light each sprite gives.
    gain: f32,
    unused: f32,
}

struct Between {
    @builtin(position) at: vec4<f32>,
    @location(0) corner: vec2<f32>,
    @location(1) colour: vec3<f32>,
}

@group(0) @binding(0) var<storage, read> stars: array<Star>;
@group(0) @binding(1) var<uniform> view: View;

// How many arms the disc has, as the simulation draws them.
const ARMS: f32 = 3.0;

// The old, warm light of the core.
const CORE: vec3<f32> = vec3(1.0, 0.58, 0.28);
// The pale light between the core and the arms.
const DISC: vec3<f32> = vec3(1.0, 0.86, 0.72);
// The young blue light of the arms.
const ARM: vec3<f32> = vec3(0.36, 0.56, 1.0);
// The pink of the arms' glowing clouds.
const CLOUD: vec3<f32> = vec3(1.0, 0.36, 0.55);
// The light of a star thrown fast.
const FAST: vec3<f32> = vec3(0.75, 0.88, 1.0);

@vertex
fn place(@location(0) corner: vec2<f32>, @builtin(instance_index) at: u32) -> Between {
    let star = stars[at];
    let radius = length(star.position);
    let speed = length(star.velocity);
    // One at an arm's crest, zero between the arms.
    let crest = pow(0.5 + 0.5 * cos(ARMS * star.angle), 3.0);
    let outward = smoothstep(0.05, 0.5, radius);
    var colour = mix(CORE, DISC, smoothstep(0.05, 0.3, radius));
    colour = mix(colour, ARM, outward * (0.35 + 0.65 * crest));
    let cloudy = step(0.93, fract(star.shine * 31.7)) * crest * outward;
    colour = mix(colour, CLOUD, cloudy * 0.8);
    let thrown = smoothstep(0.25, 1.1, speed);
    colour = mix(colour, FAST, thrown) * (1.0 + 1.5 * thrown);
    // The few bright stars are larger, and the arms' young stars brighter.
    let bright = 0.35 + 0.65 * star.shine * star.shine;
    let light = bright * (0.55 + 0.6 * crest * outward + 0.4 * cloudy);
    let size = 0.75 + 0.6 * star.shine;

    let screen = star.position.x * view.across + star.position.y * view.along + star.lift * view.up;
    let at_clip = screen * view.scale + corner * view.sprite * size;
    return Between(vec4(at_clip, 0.0, 1.0), corner, colour * light * view.gain);
}

@fragment
fn shade(between: Between) -> @location(0) vec4<f32> {
    let near = dot(between.corner, between.corner);
    let soft = exp(-near * 4.0) * max(1.0 - near, 0.0);
    return vec4(between.colour * soft, 1.0);
}
