// A Schwarzschild black hole, traced one photon a pixel. Units are geometric, G = c = 1, with the hole's mass M = 1:
// the horizon is at r = 2, the photon sphere at r = 3, and the innermost stable circular orbit (ISCO) at r = 6.
//
// Each pixel's photon is followed backward from the camera, a static observer at r = distance. A null geodesic stays in
// one plane through the hole, where u = 1/r obeys the Binet equation u'' + u = 3 u². In flat 3D coordinates that same
// path is traced by the acceleration a = -3 h² x / r⁵, where h = |x × v| is conserved: integrated here with RK4, the
// step a fixed fraction of r, so it is fine near the hole and long far from it. A ray that falls inside r = 2 is
// captured; one that climbs past ESCAPE is free, and samples the sky by its direction there. The disk is a thin sheet
// in the equatorial plane, y = 0, hit wherever a step crosses it, so the far side shows over and under the shadow and
// the higher-order images ring it.
//
// What is not exact, by choice: the disk's temperature scale is picked to put its light in the visible (a real disk
// round a black hole is far hotter, glowing in ultraviolet or X-rays); its colour is the chromaticity of a blackbody at
// the seen temperature with the bolometric g⁴ T⁴ as its brightness, rather than the spectrum integrated over the eye's
// response; it is opaque in its body but thinned at its edges and mottled by a procedural texture that is art, not
// turbulence; the camera is a static observer, its own orbital motion and the sky's slight blueshift left out; and a
// ray still circling the photon sphere after STEPS steps, within a hair of the shadow's edge, is taken as captured.

struct View {
    // The camera's longitude and latitude about the hole, in radians, and its distance, in M.
    yaw: f32,
    pitch: f32,
    distance: f32,
    // The time, in M, that the disk has turned for.
    time: f32,
    // The width of the picture over its height, the tangent of half its vertical field of view, and the light's gain.
    aspect: f32,
    tan_half_fov: f32,
    exposure: f32,
    unused: f32,
}

@group(0) @binding(0) var<uniform> view: View;

struct Placed {
    @builtin(position) at: vec4<f32>,
    // Where the pixel is on the screen, from -1 to 1 across and up.
    @location(0) screen: vec2<f32>,
}

@vertex
fn place(@location(0) corner: vec2<f32>) -> Placed {
    return Placed(vec4(corner, 0.0, 1.0), corner);
}

const HORIZON: f32 = 2.0;
const ISCO: f32 = 6.0;
const DISK_OUTER: f32 = 20.0;
// Past this radius what bending is left, about M b / r², is far under a pixel.
const ESCAPE: f32 = 120.0;
// The most steps a ray takes. One still bound after them is circling the photon sphere and is taken as captured.
const STEPS: i32 = 320;
// Each step moves the photon about this fraction of its radius.
const STEP: f32 = 0.075;
// The most times a ray passes through the disk before what is behind no longer shows.
const CROSSINGS: i32 = 4;

// The disk's temperature at its hottest, in kelvin, and the gain of its light.
const DISK_PEAK_KELVIN: f32 = 5000.0;
const DISK_GAIN: f32 = 1.1;

// The acceleration of the photon at x whose conserved h² is h2: a = -3 h² x / r⁵, which makes its path the Binet
// equation's.
fn bend(x: vec3<f32>, h2: f32) -> vec3<f32> {
    let r2 = dot(x, x);
    return -3.0 * h2 * x / (r2 * r2 * sqrt(r2));
}

// The chromaticity of a blackbody at `kelvin`, in linear sRGB of luminance 1: Kim et al.'s cubic fit of the Planckian
// locus in CIE 1931 xy, valid from 1667 K to 25000 K, then xyY to XYZ to sRGB's primaries. Out of the gamut is clipped.
fn blackbody(kelvin: f32) -> vec3<f32> {
    let t = clamp(kelvin, 1667.0, 25000.0);
    let k = 1000.0 / t;
    let k2 = k * k;
    let k3 = k2 * k;
    var x: f32;
    if t < 4000.0 {
        x = -0.2661239 * k3 - 0.2343589 * k2 + 0.8776956 * k + 0.179910;
    } else {
        x = -3.0258469 * k3 + 2.1070379 * k2 + 0.2226347 * k + 0.240390;
    }
    let x2 = x * x;
    let x3 = x2 * x;
    var y: f32;
    if t < 2222.0 {
        y = -1.1063814 * x3 - 1.34811020 * x2 + 2.18555832 * x - 0.20219683;
    } else if t < 4000.0 {
        y = -0.9549476 * x3 - 1.37418593 * x2 + 2.09137015 * x - 0.16748867;
    } else {
        y = 3.0817580 * x3 - 5.87338670 * x2 + 3.75112997 * x - 0.37001483;
    }
    let xyz = vec3(x / y, 1.0, (1.0 - x - y) / y);
    let rgb = vec3(
        dot(vec3(3.2406, -1.5372, -0.4986), xyz),
        dot(vec3(-0.9689, 1.8758, 0.0415), xyz),
        dot(vec3(0.0557, -0.2040, 1.0570), xyz),
    );
    return max(rgb, vec3(0.0));
}

// A hash of three whole numbers to three in [0, 1).
fn hash3(cell: vec3<i32>) -> vec3<f32> {
    var p = vec3<u32>(cell) * vec3(1664525u, 1013904223u, 2654435761u);
    p = (p.yzx ^ (p >> vec3(16u))) * vec3(2246822519u, 3266489917u, 668265263u);
    p = p ^ (p.zxy >> vec3(13u));
    p = p * vec3(2654435761u, 1597334677u, 3812015801u);
    return vec3<f32>(p >> vec3(8u)) / 16777216.0;
}

// Smooth value noise in 3D, from 0 to 1.
fn noise(at: vec3<f32>) -> f32 {
    let cell = vec3<i32>(floor(at));
    let f = fract(at);
    let s = f * f * (3.0 - 2.0 * f);
    let a = mix(hash3(cell).x, hash3(cell + vec3(1, 0, 0)).x, s.x);
    let b = mix(hash3(cell + vec3(0, 1, 0)).x, hash3(cell + vec3(1, 1, 0)).x, s.x);
    let c = mix(hash3(cell + vec3(0, 0, 1)).x, hash3(cell + vec3(1, 0, 1)).x, s.x);
    let d = mix(hash3(cell + vec3(0, 1, 1)).x, hash3(cell + vec3(1, 1, 1)).x, s.x);
    return mix(mix(a, b, s.y), mix(c, d, s.y), s.z);
}

// Four octaves of noise, from 0 to about 1.
fn fbm(at: vec3<f32>) -> f32 {
    var sum = 0.0;
    var amplitude = 0.5;
    var p = at;
    for (var octave = 0; octave < 4; octave++) {
        sum += amplitude * noise(p);
        p = p * 2.03 + vec3(1.7, 9.2, 3.1);
        amplitude *= 0.5;
    }
    return sum / 0.9375;
}

// The gas's streaks at radius r and angle phi (turning with the orbit), seen at a moment in its own cycle: a noise
// stretched along the orbit, so it reads as streaks, and wrapped round the circle so it has no seam.
fn streaks(r: f32, phi: f32, cycle: f32) -> f32 {
    let around = vec2(cos(phi), sin(phi));
    let coarse = fbm(vec3(around * 2.0, r * 1.4 + cycle * 7.3));
    let fine = fbm(vec3(around * 5.0, r * 5.5 - cycle * 5.1));
    return coarse * 0.6 + fine * 0.4;
}

// The disk's texture at radius r and angle phi. Each ring turns at its Keplerian Ω = √(M / r³), inner rings faster,
// which would shear any fixed pattern into ever finer spirals. So two copies of the pattern are blended, each turning
// at Ω from its own start and faded out before it shears too far, half a cycle apart (a flow map); the blend is
// rescaled by √(w₀² + w₁²) so its contrast does not dip between them.
fn texture(r: f32, phi: f32, omega: f32) -> f32 {
    let period = 60.0;
    var sum = 0.0;
    var weights = 0.0;
    for (var copy = 0; copy < 2; copy++) {
        let phase = view.time / period + f32(copy) * 0.5;
        let into = fract(phase) * period;
        let weight = 1.0 - abs(2.0 * fract(phase) - 1.0);
        sum += weight * (streaks(r, phi - omega * into, floor(phase) + f32(copy) * 0.37) - 0.5);
        weights += weight * weight;
    }
    return 0.5 + sum / sqrt(max(weights, 0.0001));
}

// The light the disk gives toward the camera where the ray crosses it at x, and how much of what is behind it hides.
//
// Each element of gas moves on a circular geodesic with Ω = √(M / r³), so its 4-velocity has u^t = 1 / √(1 - 3M/r).
// A photon of energy E and axial angular momentum L, so λ = L / E, is seen by it at E u^t (1 - Ω λ), and by the
// static camera at E / √(1 - 2M / r_cam). Their ratio is the redshift factor
//     g = √(1 - 3M/r) / (√(1 - 2M/r_cam) (1 - Ω λ)),
// which holds the gravitational redshift and the relativistic Doppler shift both. The disk emits as a blackbody at
// the Novikov–Thorne temperature T ∝ r^(-3/4) (1 - √(r_isco / r))^(1/4), and I_ν / ν³ is invariant along the ray, so
// it is seen as a blackbody at g T, its bolometric intensity g⁴ times what it gave: brighter and bluer where the gas
// comes toward the camera, dimmer and redder where it leaves.
fn disk(x: vec3<f32>, lambda: f32, observer: f32) -> vec4<f32> {
    let r = length(x);
    let omega = sqrt(1.0 / (r * r * r));
    let g = sqrt(1.0 - 3.0 / r) / (observer * (1.0 - omega * lambda));
    // T(r) peaks at r = 49/36 r_isco, where r^(-3/4) (1 - √(r_isco / r))^(1/4) is 0.12726 for r_isco = 6.
    let profile = pow(r, -0.75) * pow(max(1.0 - sqrt(ISCO / r), 0.0), 0.25) / 0.12726;
    let kelvin = DISK_PEAK_KELVIN * profile;
    let seen = g * profile;
    // The angle the gas has turned through, increasing the way it orbits: about +y, from +x toward -z.
    let phi = atan2(-x.z, x.x);
    let gas = texture(r, phi, omega);
    let clumps = smoothstep(0.3, 0.8, gas);
    let edges = smoothstep(ISCO, ISCO + 0.6, r) * (1.0 - smoothstep(DISK_OUTER * 0.55, DISK_OUTER, r));
    let light = blackbody(g * kelvin) * DISK_GAIN * seen * seen * seen * seen * (0.35 + 1.3 * clumps);
    let opacity = edges * (0.82 + 0.18 * clumps);
    return vec4(light * opacity, opacity);
}

// The Milky Way: a faint band of unresolved stars along a great circle, mottled and cut by dust, with a brighter core.
fn galaxy(direction: vec3<f32>) -> vec3<f32> {
    let pole = normalize(vec3(0.42, 0.86, -0.29));
    let core = normalize(vec3(-0.83, 0.18, -0.52));
    let height = dot(direction, pole);
    let band = exp(-height * height / 0.018);
    let bulge = pow(max(dot(direction, core), 0.0), 6.0);
    let mottled = fbm(direction * 5.0);
    let dust = smoothstep(0.42, 0.72, fbm(direction * 9.0 + vec3(4.0)));
    let glow = (band * (0.35 + 0.9 * mottled) + bulge * 1.6 * band) * (1.0 - 0.75 * dust * band);
    let tint = mix(vec3(0.7, 0.76, 0.92), vec3(1.0, 0.88, 0.72), bulge);
    return tint * glow * 0.03;
}

// The stars a ray leaving in `direction` sees: three layers of cells on the sky, each holding at most one star at a
// spot within it, of a random brightness and blackbody colour. A star is drawn as a small Gaussian at least as wide as
// the pixel's footprint on the sky, its flux kept as it widens, so a star lensed into an arc brightens and none flicker.
fn stars(direction: vec3<f32>, footprint: f32) -> vec3<f32> {
    var light = vec3(0.0);
    for (var layer = 0; layer < 3; layer++) {
        let scale = 70.0 * pow(2.2, f32(layer));
        let p = direction * scale;
        let cell = vec3<i32>(floor(p));
        let seed = hash3(cell + vec3(layer * 7919, layer * 104729, layer * 1299709));
        let present = seed.x < 0.45 - 0.12 * f32(layer);
        let spot = hash3(cell + vec3(31, 17, 53));
        let centre = normalize(vec3<f32>(cell) + 0.2 + 0.6 * spot) * scale;
        let distance = length(p - centre) / scale;
        let size = 0.00045;
        let width = max(size, footprint * 0.6);
        let flux = pow(seed.y, 14.0) * 4.0 + 0.0012;
        let kelvin = mix(3300.0, 11000.0, pow(seed.z, 1.4));
        let star = blackbody(kelvin) * flux * (size * size) / (width * width) * exp(-distance * distance / (width * width));
        light += select(vec3(0.0), star, present);
    }
    return light;
}

// What lies at the end of a ray that left in `direction`.
fn sky(direction: vec3<f32>, footprint: f32) -> vec3<f32> {
    return stars(direction, footprint) + galaxy(direction);
}

@fragment
fn shade(placed: Placed) -> @location(0) vec4<f32> {
    // The camera on its orbit, looking at the hole, the disk's axis up.
    let eye = view.distance * vec3(cos(view.pitch) * sin(view.yaw), sin(view.pitch), cos(view.pitch) * cos(view.yaw));
    let forward = -eye / view.distance;
    let right = normalize(cross(forward, vec3(0.0, 1.0, 0.0)));
    let up = cross(right, forward);
    let across = placed.screen.x * view.tan_half_fov * view.aspect;
    let upward = placed.screen.y * view.tan_half_fov;
    let looking = normalize(forward + right * across + up * upward);

    // The camera is a static observer at r_cam: what it sees at angle ψ from the radial direction leaves it, in
    // Schwarzschild's coordinates, at tan α = tan ψ / √(1 - 2M/r_cam), its radial part shortened.
    let outward = eye / view.distance;
    let observer = sqrt(1.0 - HORIZON / view.distance);
    let radial = dot(looking, outward);
    var v = normalize(looking + outward * radial * (observer - 1.0));
    var x = eye;
    let h2 = dot(cross(x, v), cross(x, v));
    // The photon's λ = L_y / E about the disk's axis, from the momentum the camera sees it arrive with, -looking:
    // L_y = (x × p)_y in the static frame, and E = √(1 - 2M/r_cam) times what the camera measures.
    let lambda = cross(eye, -looking).y / observer;

    var light = vec3(0.0);
    var through = 1.0;
    var crossings = 0;
    var escaped = false;
    for (var step = 0; step < STEPS; step++) {
        let r = length(x);
        let dl = STEP * r / length(v);
        let k1v = bend(x, h2);
        let k1x = v;
        let k2v = bend(x + 0.5 * dl * k1x, h2);
        let k2x = v + 0.5 * dl * k1v;
        let k3v = bend(x + 0.5 * dl * k2x, h2);
        let k3x = v + 0.5 * dl * k2v;
        let k4v = bend(x + dl * k3x, h2);
        let k4x = v + dl * k3v;
        let next = x + dl / 6.0 * (k1x + 2.0 * k2x + 2.0 * k3x + k4x);
        v += dl / 6.0 * (k1v + 2.0 * k2v + 2.0 * k3v + k4v);

        // The step crosses the disk's plane where y changes sign; it is hit at the crossing, within its radii.
        let crosses = x.y * next.y < 0.0;
        if crosses {
            let hit = mix(x, next, x.y / (x.y - next.y));
            let at = length(hit);
            let on_disk = at > ISCO && at < DISK_OUTER;
            if on_disk {
                let seen = disk(hit, lambda, observer);
                light += through * seen.rgb;
                through *= 1.0 - seen.a;
                crossings += 1;
            }
        }
        x = next;

        let r_next = length(x);
        let captured = r_next < HORIZON;
        let free = r_next > ESCAPE;
        let hidden = crossings >= CROSSINGS || through < 0.01;
        escaped = free;
        if captured || free || hidden {
            break;
        }
    }

    // The sky at the ray's last direction; derivatives are taken here, where every pixel runs, and a pixel's
    // footprint on the sky is how far that direction moves to the next pixel.
    let direction = normalize(v);
    let footprint = length(fwidth(direction));
    let behind = sky(direction, footprint);
    let shown = select(vec3(0.0), behind, escaped);
    return vec4((light + through * shown) * view.exposure, 1.0);
}
