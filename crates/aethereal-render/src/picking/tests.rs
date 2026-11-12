//! The picking step's tests: the step added to vertex and fragment stages of every shape a program writes, the call's
//! bytes, and the stacks read back.

use super::*;

/// A fragment stage that reads its position in a struct, and returns early from a branch.
const IN_A_STRUCT: &str = "
struct Between {
    @builtin(position) at: vec4<f32>,
    @location(0) shade: f32,
}

@vertex
fn lift(@location(0) corner: vec2<f32>) -> Between {
    return Between(vec4(corner, 0.5, 1.0), corner.x);
}

@fragment
fn shade(between: Between) -> @location(0) vec4<f32> {
    if between.shade < 0.0 {
        return vec4(1.0);
    }
    return vec4(between.shade);
}
";

/// A fragment stage that reads its position as its own argument.
const AN_ARGUMENT: &str = "
@vertex
fn lift(@location(0) corner: vec2<f32>) -> @builtin(position) vec4<f32> {
    return vec4(corner, 0.5, 1.0);
}

@fragment
fn shade(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4(at.x);
}
";

/// A fragment stage that reads no position, and returns from within a loop and a switch.
const NO_POSITION: &str = "
@vertex
fn lift(@location(0) corner: vec2<f32>) -> @builtin(position) vec4<f32> {
    return vec4(corner, 0.5, 1.0);
}

@fragment
fn shade() -> @location(0) vec4<f32> {
    for (var step = 0; step < 4; step += 1) {
        switch step {
            case 2: {
                return vec4(0.5);
            }
            default: {}
        }
    }
    return vec4(1.0);
}
";

/// A fragment stage that writes its depth among what it returns.
const WRITES_DEPTH: &str = "
struct Shaded {
    @location(0) colour: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

@vertex
fn lift(@location(0) corner: vec2<f32>) -> @builtin(position) vec4<f32> {
    return vec4(corner, 0.5, 1.0);
}

@fragment
fn shade() -> Shaded {
    return Shaded(vec4(1.0), 0.25);
}
";

/// How many times `wgsl` calls the step at its position's depth.
fn calls(wgsl: &str) -> usize {
    wgsl.matches(&format!("{PICK}(")).count() - 1
}

/// How many times `wgsl` calls the step at a depth it writes, besides the call at the position's.
fn calls_at(wgsl: &str) -> usize {
    wgsl.matches(&format!("{PICK_AT}(")).count() - 2
}

#[test]
fn the_step_is_called_before_every_return_of_the_fragment_stage_however_it_reads_its_position() {
    // Catches an early return left without the step, so a fragment drawn on that branch is never picked; a stage that
    // reads its position in a struct, or not at all, refused or given a second position the GPU refuses; and the
    // step called from the vertex stage, which has no fragment's pixel.
    for (wgsl, returns) in [(IN_A_STRUCT, 2), (AN_ARGUMENT, 1), (NO_POSITION, 2)] {
        let Ok(picked) = picked(wgsl, ("lift", "shade"), 0) else {
            panic!("the step is added to {wgsl}");
        };

        assert_eq!(calls(&picked), returns, "{picked}");
        assert!(program::parsed(&picked).is_ok(), "what the step makes is valid WGSL: {picked}");
    }
}

#[test]
fn every_return_of_the_vertex_stage_moves_the_position_it_writes_whether_alone_or_in_a_struct() {
    // Catches a vertex stage left unmoved, so a call's child draws where its own program places it and not at the
    // origin its caller gives; a stage that returns its position in a struct refused, or moved in another member; and
    // an early return left unmoved, so what is drawn on that branch stays put (ADR-0055).
    const BRANCHED: &str = "
@vertex
fn lift(@location(0) corner: vec2<f32>) -> @builtin(position) vec4<f32> {
    if corner.x < 0.0 {
        return vec4(corner, 0.25, 1.0);
    }
    return vec4(corner, 0.5, 1.0);
}

@fragment
fn shade() -> @location(0) vec4<f32> {
    return vec4(1.0);
}
";
    for (wgsl, returns) in [(IN_A_STRUCT, 1), (AN_ARGUMENT, 1), (BRANCHED, 2)] {
        let Ok(picked) = picked(wgsl, ("lift", "shade"), 0) else {
            panic!("the step is added to {wgsl}");
        };
        let Ok((module, _)) = program::parsed(&picked) else {
            panic!("what the step makes is valid WGSL: {picked}");
        };
        let Some(lift) = module.entry_points.iter().find(|entry| entry.name == "lift") else {
            panic!("the vertex stage stays: {picked}");
        };
        let shift = module.functions.fetch_if(|function| function.name.as_deref() == Some(SHIFT));
        let moved = lift
            .function
            .expressions
            .iter()
            .filter(|(_, expression)| matches!(expression, Expression::CallResult(called) if Some(*called) == shift))
            .count();

        assert_eq!(moved, returns, "{picked}");
    }
}

#[test]
fn a_call_s_bytes_lie_where_the_step_reads_them() {
    // Tripwire: the layout of `AetherealPickCall` in the step's WGSL, as naga lays a struct out, against the bytes
    // `call_bytes` writes: a field moved in one and not the other keys a draw as another call, or moves it by its
    // scale rather than its shift, and fits the binding the layout declares.
    let Ok((module, _)) = program::parsed(&format!("{AN_ARGUMENT}\n{}", step(0))) else {
        panic!("the step is valid WGSL");
    };
    let members = module.types.iter().find_map(|(_, ty)| match &ty.inner {
        TypeInner::Struct { members, span } if ty.name.as_deref() == Some("AetherealPickCall") => {
            Some((members, *span))
        }
        _ => None,
    });
    let Some((members, span)) = members else {
        panic!("the step reads an `AetherealPickCall`");
    };
    assert!(span as usize <= CALL_BYTES, "the call's {span} bytes fit its binding");

    let bytes = call_bytes(7, true, [0.5, 0.25], [-1.5, 2.0]);
    let word = |at: u32| {
        let at = at as usize;
        [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]
    };
    let offset = |name: &str| {
        let Some(member) = members.iter().find(|member| member.name.as_deref() == Some(name)) else {
            panic!("`AetherealPickCall` has `{name}`");
        };
        member.offset
    };
    let float = |at: u32| f32::from_le_bytes(word(at));
    assert_eq!(u32::from_le_bytes(word(offset("key"))), 7);
    assert_eq!(u32::from_le_bytes(word(offset("depth"))), 1);
    assert_eq!([float(offset("scale")), float(offset("scale") + 4)], [0.5, 0.25]);
    assert_eq!([float(offset("shift")), float(offset("shift") + 4)], [-1.5, 2.0]);
}

#[test]
fn a_stage_that_writes_its_depth_is_picked_at_the_depth_it_writes() {
    // Catches a stage that writes its depth ranked at its position's, which is not the depth it is tested at, so the
    // stack's top is not what is seen there.
    let Ok(picked) = picked(WRITES_DEPTH, ("lift", "shade"), 0) else {
        panic!("the step is added to {WRITES_DEPTH}");
    };

    assert_eq!((calls(&picked), calls_at(&picked)), (0, 1), "{picked}");
    assert!(picked.contains(".depth);"), "the step is called with the depth written: {picked}");
    assert!(program::parsed(&picked).is_ok(), "what the step makes is valid WGSL: {picked}");
}

#[test]
fn a_program_that_names_what_the_step_does_is_refused() {
    // Catches WGSL whose own `aethereal_pick` the step's would clash with, let through the check and failing as the
    // pipeline is made.
    let clashing = format!("{AN_ARGUMENT}\nfn {PICK}() {{}}");

    assert!(matches!(picked(&clashing, ("lift", "shade"), 0), Err(ProgramRefused::Parse { .. })));
}

#[test]
fn a_stack_keeps_the_highest_calls_once_each_with_the_hits_merged_and_never_more_than_its_slots() {
    // Catches a stack read in the order the GPU wrote it rather than by key, a call kept twice when two triangles of
    // one draw cover the probe, at one depth or two, a key answered rather than its call, a hit that is never merged
    // in, or one that grows a stack past its slots.
    let nearer = |call: u32| (1 << 16) | call;
    let mut written = vec![0_u32; MOST_PROBES * STACK];
    written[..STACK].copy_from_slice(&[7, 9, 9, 2]);
    written[STACK..2 * STACK].copy_from_slice(&[nearer(3), 6, 3, 0]);
    let bytes: Vec<u8> = written.iter().flat_map(|word| word.to_le_bytes()).collect();
    let hits = |probe| {
        if probe == 1 {
            vec![1, 5]
        } else {
            vec![8, 1]
        }
    };

    let stacks = stacks(&bytes, 2, hits);

    assert_eq!(stacks, vec![vec![9, 8, 7, 2], vec![3, 6, 5, 1]]);
}

#[test]
fn the_probes_bytes_hold_empty_stacks_the_count_and_each_pixel_where_the_step_reads_them() {
    // Tripwire: the layout of `AetherealPickProbes` in the step's WGSL, as WGSL lays a struct out: the stacks first,
    // a word each, the count after them, and each pixel two words at the next eight bytes. A layout written otherwise
    // probes the wrong pixels, or none.
    let bytes = probes_bytes(&[[3, 4], [5, 6]]);

    assert_eq!(bytes.len(), PROBES_BYTES);
    assert!(bytes[..STACKS_BYTES].iter().all(|byte| *byte == 0));
    let word = |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    assert_eq!([word(256), word(264), word(268), word(272), word(276)], [2, 3, 4, 5, 6]);
}
