// Moves each star a step along the disc. Every star has a home: a place on a closed curve about the centre, its
// orbit, swept round by the arms' slow turn, which the star travels at its own pace. The curves of neighbouring
// orbits are turned a little further the further out they lie, and stars linger where their curve swings wide, so
// the crowding traces spiral arms that stay as the stars stream through them. A star is pulled home by a spring and
// damped toward its home's motion, so it follows its orbit while nothing disturbs it, and the pointer, while it is
// held, pulls and swirls the stars near it, which stream back into the arms once it lets go.

struct Star {
    position: vec2<f32>,
    velocity: vec2<f32>,
    // The size of its orbit.
    orbit: f32,
    // Where it is along its orbit, as an angle in the arms' turning frame.
    angle: f32,
    // Its height above the disc's plane.
    lift: f32,
    // How bright it is, from zero to one, which also picks how tightly it is held home.
    shine: f32,
}

struct Step {
    // The pointer, in the disc's plane.
    pointer: vec2<f32>,
    // How hard the pointer pulls: zero while it is up.
    pull: f32,
    // The seconds this step covers.
    delta: f32,
    // How far the arms have turned by the end of the step, in radians, and how fast they turn, in radians a second.
    turned: f32,
    turning: f32,
    unused: vec2<f32>,
}

@group(0) @binding(0) var<storage, read_write> stars: array<Star>;
@group(0) @binding(1) var<uniform> step: Step;

// A whole turn, in radians.
const TAU: f32 = 6.2831853;
// How many arms the disc has.
const ARMS: f32 = 3.0;
// How far an orbit's curve is turned for each step outward, in radians for each doubling of its size.
const WIND: f32 = 1.7;
// How far an orbit's curve swings from a circle at most.
const SWING: f32 = 0.15;
// The speed of a star far from the centre, in the disc's units a second.
const CRUISE: f32 = 0.16;
// The size of the core within which stars slow toward the centre.
const CORE: f32 = 0.09;
// How hard a star's spring pulls it home, at its tightest, per second squared.
const STIFFNESS: f32 = 10.0;
// How much of the damping that would stop a spring's swing outright the star feels.
const DAMPING: f32 = 0.55;
// How hard the pointer pulls, and how much it swirls.
const PULL: f32 = 2.4;
const SWIRL: f32 = 1.3;
// The softening and the reach of the pointer's pull.
const SOFTENING: f32 = 0.035;
const REACH: f32 = 0.55;
// The most a star is pushed, per second squared.
const MOST: f32 = 40.0;
// The steps each frame's time is cut into.
const SUBSTEPS: u32 = 2u;

// The angular speed of a star on an orbit of size `orbit`, about the centre.
fn spin(orbit: f32) -> f32 {
    let speed = CRUISE * (1.0 - exp(-orbit / CORE));
    return speed / max(orbit, 0.004);
}

// How far the curve of an orbit of size `orbit` swings: its inner orbits round, the core a ball.
fn swing(orbit: f32) -> f32 {
    return SWING * smoothstep(0.06, 0.28, orbit);
}

// The place on the curve of an orbit of size `orbit` at `angle` along it, with the arms turned by `turned`.
fn home(orbit: f32, angle: f32, turned: f32) -> vec2<f32> {
    let reach = orbit * (1.0 + swing(orbit) * cos(ARMS * angle));
    let along = angle + WIND * log2(max(orbit, 0.02)) + turned;
    return reach * vec2(cos(along), sin(along));
}

// How fast a star moves along its curve, in radians of angle a second: as fast as its orbit spins less the arms' turn,
// and slower where the curve swings wide, as a planet is slowest at its farthest.
fn travel(orbit: f32, angle: f32) -> f32 {
    let wide = 1.0 + swing(orbit) * cos(ARMS * angle);
    return (spin(orbit) - step.turning) / (wide * wide);
}

// What the held pointer does to a star at `position`: a pull toward it, and a swirl about it, both fading with distance.
fn pointer(position: vec2<f32>) -> vec2<f32> {
    let toward = step.pointer - position;
    let near = dot(toward, toward);
    let fade = exp(-near / (REACH * REACH));
    let strength = step.pull * PULL * fade / (near + SOFTENING * SOFTENING);
    return strength * (toward + SWIRL * vec2(-toward.y, toward.x));
}

@compute @workgroup_size(64)
fn simulate(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= arrayLength(&stars) {
        return;
    }
    var star = stars[id.x];
    let held = STIFFNESS * (0.3 + 0.7 * fract(star.shine * 97.31));
    let damping = 2.0 * DAMPING * sqrt(held);
    let delta = step.delta / f32(SUBSTEPS);
    var turned = step.turned - step.turning * step.delta;
    for (var at = 0u; at < SUBSTEPS; at++) {
        // The angle is kept within a turn, so it holds its precision however long the page runs.
        let angle = fract((star.angle + travel(star.orbit, star.angle) * delta) / TAU) * TAU;
        let was = home(star.orbit, star.angle, turned);
        let now = home(star.orbit, angle, turned + step.turning * delta);
        let moving = (now - was) / max(delta, 0.0001);
        var push = held * (now - star.position) + damping * (moving - star.velocity) + pointer(star.position);
        let size = length(push);
        if size > MOST {
            push *= MOST / size;
        }
        star.velocity += push * delta;
        star.position += star.velocity * delta;
        star.angle = angle;
        turned += step.turning * delta;
    }
    stars[id.x] = star;
}
