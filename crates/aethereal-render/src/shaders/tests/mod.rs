// The mirrors below write each sum as the shader writes it, unfused, so they read as the shader does.
#![allow(clippy::suboptimal_flops)]

mod scan;

use super::PACKAGE;
use naga::front::wgsl;
use naga::valid::{Capabilities, ValidationFlags, Validator};
use wesl::resolver::{PackageResolver, Router, VirtualResolver};
use wesl::syntax::{ModulePath, PathOrigin};
use wesl::{CompileOptions, Compiler, Resolver};

/// Links `main` as the main module with this package as its dependency, keeping every declaration it holds, and
/// answers the WGSL.
fn linked(main: &ModulePath, resolver: impl Resolver) -> String {
    let options = CompileOptions { keep_main: true, ..CompileOptions::default() };
    match Compiler::new_with_resolver(options, resolver).compile_module(main) {
        Ok(linked) => linked.to_string(),
        Err(error) => panic!("{main} links: {error}"),
    }
}

fn package() -> PackageResolver {
    let mut resolver = PackageResolver::new();
    resolver.add_package(&PACKAGE);
    resolver
}

/// Links the package's module `name` alone, as the main module.
fn piece(name: &str) -> String {
    let main = ModulePath::new(PathOrigin::Package(String::from("aethereal_render")), vec![String::from(name)]);
    linked(&main, package())
}

/// Links the program `source`, which imports from the package, as a dependent's build script links its shader.
fn program(source: &'static str) -> String {
    let main = ModulePath::new(PathOrigin::Absolute, vec![String::from("main")]);
    let mut own = VirtualResolver::new();
    own.add_module(main.clone(), source.into());
    let mut router = Router::new();
    router.mount_resolver(ModulePath::new_root(), own);
    router.mount_fallback_resolver(package());
    linked(&main, router)
}

/// Reads and validates `text` as the renderer reads a program's WGSL, with its `naga`.
fn validate(what: &str, text: &str) {
    let module = match wgsl::parse_str(text) {
        Ok(module) => module,
        Err(error) => panic!("{what} parses: {}", error.emit_to_string(text)),
    };
    let checked = Validator::new(ValidationFlags::all(), Capabilities::empty()).validate(&module);
    if let Err(error) = checked {
        panic!("{what} validates: {}", error.emit_to_string(text));
    }
}

// Each module, linked alone with what it imports, is WGSL the renderer's `naga` reads and validates, as it reads a
// program's: a name WGSL reserves, a built-in shadowed, an import the module does not make, or WGSL the two parsers
// read differently fails here.
#[test]
fn each_piece_links_alone_into_wgsl_the_renderer_validates() {
    let names: Vec<&str> = PACKAGE.root.submodules.iter().map(|module| module.name).collect();
    assert_eq!(names.len(), 7, "the package holds its seven modules: {names:?}");

    for name in names {
        validate(&format!("the module {name}"), &piece(name));
    }
}

// Linking keeps every function a module holds, so the validation above checked each of them.
#[test]
fn a_piece_linked_alone_keeps_its_functions_and_brings_what_it_imports() {
    let noise = piece("noise");

    for function in ["fn noise(", "fn fbm(", "fn _1aethereal_render_hash_hash("] {
        assert!(noise.contains(function), "the linked noise holds {function}:\n{noise}");
    }
}

// A program samples its own grid by what `bilinear` answers, as the module's documentation shows: the struct crosses
// the package's edge and indexes the program's buffer.
#[test]
fn a_program_reads_its_grid_by_the_cells_and_weights_bilinear_answers() {
    let linked = program(
        "
import aethereal_render::grid::bilinear;

@group(0) @binding(0) var<storage, read> heights: array<f32>;

fn height_at(uv: vec2<f32>) -> f32 {
    let b = bilinear(uv, vec2(1000, 1000));
    let near = mix(heights[b.cells.x], heights[b.cells.y], b.weights.x);
    let far = mix(heights[b.cells.z], heights[b.cells.w], b.weights.x);
    return mix(near, far, b.weights.y);
}
",
    );

    validate("the program sampling its grid", &linked);
}

/// The hash's mixing on 32 bits, as the shader's: `h = i·374761393 + j·668265263 + k·1274126177 + seed·1442695041`,
/// then `h = (h ^ h >> 13)·1274126177` and `h ^= h >> 16`, every product and sum wrapping as `Math.imul` and `| 0` wrap.
#[allow(clippy::cast_sign_loss)]
const fn mixed(i: i32, j: i32, k: i32, seed: i32) -> u32 {
    let h = (i as u32)
        .wrapping_mul(374_761_393)
        .wrapping_add((j as u32).wrapping_mul(668_265_263))
        .wrapping_add((k as u32).wrapping_mul(1_274_126_177))
        .wrapping_add((seed as u32).wrapping_mul(1_442_695_041));
    let h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    h ^ (h >> 16)
}

/// The shader's hash: the top 24 bits of the mixing, as a fraction.
#[allow(clippy::cast_precision_loss)]
fn hash(i: i32, j: i32, k: i32, seed: i32) -> f32 {
    (mixed(i, j, k, seed) >> 8) as f32 / 16_777_216.0
}

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// The shader's value noise, in 32-bit floats as the shader computes it, with the shader's names.
#[allow(clippy::cast_possible_truncation, clippy::many_single_char_names)]
fn noise(x: f32, z: f32, k: i32, seed: i32) -> f32 {
    let fi = x.floor();
    let fj = z.floor();
    let i = fi as i32;
    let j = fj as i32;
    let fx = x - fi;
    let fz = z - fj;
    let u = fx * fx * (3.0 - 2.0 * fx);
    let v = fz * fz * (3.0 - 2.0 * fz);
    let near = mix(hash(i, j, k, seed), hash(i + 1, j, k, seed), u);
    let far = mix(hash(i, j + 1, k, seed), hash(i + 1, j + 1, k, seed), u);
    mix(near, far, v) * 2.0 - 1.0
}

/// The shader's two-octave sum: the second octave at `(2x + 17.3, 2z - 9.1)` in stream `k + 1`, at half weight, the
/// sum over 1.5.
fn fbm(x: f32, z: f32, k: i32, seed: i32) -> f32 {
    (noise(x, z, k, seed) + 0.5 * noise(2.0 * x + 17.3, 2.0 * z - 9.1, k + 1, seed)) / 1.5
}

/// The shader's smooth minimum: `min(a, b) - h²k / 4`.
fn smooth_min(a: f32, b: f32, k: f32) -> f32 {
    let h = (k - (a - b).abs()).max(0.0) / k;
    a.min(b) - h * h * k / 4.0
}

// Tripwire: the hash's mixing matches JavaScript's `Math.imul` mixing bit for bit, and its fraction is the top 24
// bits. The expected words were computed by the JavaScript generator the shader ports, under node.
#[test]
fn the_hash_mixes_as_math_imul_mixes() {
    let cases = [
        ((0, 0, 0, 0), 0x0000_0000),
        ((1, 2, 3, 4), 0x07a5_da52),
        ((-1, -7, 11, 42), 0x57f5_cb33),
        ((i32::MAX, i32::MIN, 123_456_789, -987_654_321), 0x3e31_35f2),
        ((-300, 512, 7, 1337), 0xbff8_9dd5),
        ((65535, 65536, -65537, 99), 0x3746_302f),
    ];

    for ((i, j, k, seed), word) in cases {
        assert_eq!(mixed(i, j, k, seed), word, "the mixing of ({i}, {j}, {k}, {seed})");
    }
    assert_eq!(hash(1, 2, 3, 4), 0.029_874_444, "the fraction of (1, 2, 3, 4), as the JavaScript's top 24 bits");
}

// Tripwire: noise runs `× 2 − 1`, the second octave is sampled at `2x + 17.3, 2z − 9.1` in stream `k + 1`, and the sum is
// divided by 1.5. The expected values were computed by the JavaScript generator the shader ports, under node, in
// doubles and from all 32 bits of the hash; the shader's 32-bit floats and 24 bits stay within 1e-5 of them, and a
// changed constant moves them far more.
#[test]
fn the_noise_and_its_two_octave_sum_match_the_javascript() {
    let cases = [
        ((0.25, 0.75, 0, 7), 0.221_176_23, 0.059_474_645),
        ((3.6, -2.2, 5, 42), 0.004_416_445, 0.059_337_386),
        ((-11.13, 4.4, 9, 1337), 0.227_042_65, 0.374_949_5),
        ((100.5, -0.01, 23, -5), -0.463_555_2, -0.376_165_95),
    ];

    for ((x, z, k, seed), expected_noise, expected_sum) in cases {
        let at = noise(x, z, k, seed);
        let summed = fbm(x, z, k, seed);
        assert!((at - expected_noise).abs() < 1e-5, "noise at ({x}, {z}) is {at}, the JavaScript's {expected_noise}");
        assert!((summed - expected_sum).abs() < 1e-5, "fbm at ({x}, {z}) is {summed}, the JavaScript's {expected_sum}");
    }
}

// Tripwire: the smooth minimum is `min(a, b) − h²k / 4`, as the JavaScript computes it under node.
#[test]
fn the_smooth_minimum_matches_the_javascript() {
    let cases =
        [((1.0, 1.0, 0.4), 0.9), ((0.3, 0.5, 0.4), 0.275), ((2.0, -1.0, 0.5), -1.0), ((0.9, 1.0, 0.25), 0.8775)];

    for ((a, b, k), expected) in cases {
        let smoothed = smooth_min(a, b, k);
        assert!(
            (smoothed - expected).abs() < 1e-6,
            "smooth_min({a}, {b}, {k}) is {smoothed}, the JavaScript's {expected}"
        );
    }
}
