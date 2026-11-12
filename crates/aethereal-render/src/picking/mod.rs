//! Picking (ADR-0055): every program drawing in a pass also records, at the window's probes, the call it was drawn by,
//! so the window delivers the pointer through what was drawn under it, in the order of the calls.
//!
//! The renderer adds the step to every draw program and to the glyph program as it makes their pipelines, and no
//! program's author writes it. Its own group, after the program's last, binds the probes, with a stack for each, and
//! the draw's call. Before each return of the fragment stage, a fragment that lands on a probe inserts the call's
//! number into the probe's stack of [`STACK`] slots, which keeps the highest numbers, each once, the highest first:
//! the calls drawn last there, on top. Fragments elsewhere write nothing, and a draw whose call is `0` writes nowhere,
//! so the cost does not grow with the window's size, and a frame that ends on a texture, which has no pointer, probes
//! nothing.
//!
//! A fragment's storage writes are made before the depth test that may then reject it, so within a pass that tests
//! depth, a call drawn later and hidden behind an earlier one still stands above it in the stack.

#[cfg(test)]
mod tests;

use crate::program::{self, ProgramRefused, Stage};
use naga::back::wgsl::{WriterFlags, write_string};
use naga::valid::{Capabilities, ValidationFlags, Validator};
use naga::{
    Arena, Binding, Block, BuiltIn, Expression, Function, FunctionArgument, Handle, Module, Range, Scalar, Span,
    Statement, Type, TypeInner, UniqueArena, VectorSize,
};
use std::mem;

/// The most probes one frame of a window holds: the pointer, and as many presses since the last frame as fit beside it.
pub const MOST_PROBES: usize = 16;

/// The slots of a probe's stack: the most calls one probe keeps, the highest.
pub const STACK: usize = 4;

/// The bytes of the stacks at the start of the probes' buffer, which are read back: [`STACK`] words for each probe.
pub const STACKS_BYTES: usize = MOST_PROBES * STACK * 4;

/// The bytes of the probes' buffer: the stacks, how many probes there are, and each probe's pixel, two words.
pub const PROBES_BYTES: usize = STACKS_BYTES + 8 + MOST_PROBES * 8;

/// The bytes a draw's call is bound as: its number, a word of padding, and the scale of the pass's target to the
/// window, two floats.
pub const CALL_BYTES: usize = 16;

/// The name of the function each return of a fragment stage calls first.
const PICK: &str = "aethereal_pick";

/// The step, in WGSL, its group `group`: names beginning `aethereal_pick` and `AetherealPick` are the renderer's.
fn step(group: u32) -> String {
    format!(
        "
struct AetherealPickProbes {{
    stacks: array<array<atomic<u32>, {STACK}>, {MOST_PROBES}>,
    count: u32,
    points: array<vec2<u32>, {MOST_PROBES}>,
}}

struct AetherealPickCall {{
    call: u32,
    scale: vec2<f32>,
}}

@group({group}) @binding(0) var<storage, read_write> aethereal_pick_probes: AetherealPickProbes;
@group({group}) @binding(1) var<uniform> aethereal_pick_call: AetherealPickCall;

fn {PICK}(position: vec4<f32>) {{
    let call = aethereal_pick_call.call;
    if call == 0u {{
        return;
    }}
    let pixel = vec2<u32>(position.xy);
    let count = min(aethereal_pick_probes.count, {MOST_PROBES}u);
    for (var probe = 0u; probe < count; probe += 1u) {{
        let point = (vec2<f32>(aethereal_pick_probes.points[probe]) + vec2(0.5)) * aethereal_pick_call.scale;
        if all(vec2<u32>(point) == pixel) {{
            var carried = call;
            for (var slot = 0u; slot < {STACK}u; slot += 1u) {{
                let held = atomicMax(&aethereal_pick_probes.stacks[probe][slot], carried);
                if held == 0u || held == carried {{
                    break;
                }}
                carried = min(held, carried);
            }}
        }}
    }}
}}
"
    )
}

/// The WGSL `wgsl`, whose fragment stage is the entry point `fragment`, with the step added, its group `group`: each
/// return of the fragment stage first records the fragment at the probes, so a fragment the stage discards records
/// nothing. A fragment stage that reads no position reads it as the step's.
///
/// # Errors
///
/// Why the WGSL with the step is no valid module: [`ProgramRefused::Parse`] or [`ProgramRefused::Invalid`] when it
/// names what the step does, as no WGSL [`program::check`] lets through otherwise is refused.
pub fn picked(wgsl: &str, fragment: &str, group: u32) -> Result<String, ProgramRefused> {
    let source = format!("{wgsl}\n{}", step(group));
    let (mut module, _) = program::parsed(&source)?;
    let pick = module
        .functions
        .fetch_if(|function| function.name.as_deref() == Some(PICK))
        .ok_or_else(|| ProgramRefused::Invalid { message: format!("`{PICK}` is the renderer's") })?;
    let at = program::entry(&module, Stage::Fragment, fragment)?;
    let Module { types, entry_points, .. } = &mut module;
    let function = &mut entry_points[at].function;
    let (position, emitted) = position(types, function);
    let call = Statement::Call { function: pick, arguments: vec![position], result: None };
    before_returns(&mut function.body, &call);
    if let Some(emit) = emitted {
        function.body.splice(0..0, Block::from_vec(vec![emit]));
    }
    let info = Validator::new(ValidationFlags::all(), Capabilities::empty())
        .validate(&module)
        .map_err(|error| ProgramRefused::Invalid { message: error.emit_to_string(&source) })?;

    write_string(&module, &info, WriterFlags::empty())
        .map_err(|error| ProgramRefused::Invalid { message: error.to_string() })
}

/// The fragment's position in `function`, a fragment stage, as an expression of it: the argument that is it, or the
/// member of the argument that holds it, read by an expression emitted at the start, which is answered beside it, or
/// an argument added to read it, of a type in `types`.
fn position(types: &mut UniqueArena<Type>, function: &mut Function) -> (Handle<Expression>, Option<Statement>) {
    let is_position = |binding: Option<&Binding>| matches!(binding, Some(Binding::BuiltIn(BuiltIn::Position { .. })));
    let found = function.arguments.iter().zip(0..).find_map(|(argument, at)| {
        if is_position(argument.binding.as_ref()) {
            return Some((at, None));
        }
        let TypeInner::Struct { members, .. } = &types[argument.ty].inner else {
            return None;
        };
        members
            .iter()
            .zip(0..)
            .find(|(member, _)| is_position(member.binding.as_ref()))
            .map(|(_, index)| (at, Some(index)))
    });
    match found {
        Some((at, None)) => (argument_expression(&mut function.expressions, at), None),
        Some((at, Some(index))) => {
            let base = argument_expression(&mut function.expressions, at);
            let read = function.expressions.append(Expression::AccessIndex { base, index }, Span::UNDEFINED);
            (read, Some(Statement::Emit(Range::new_from_bounds(read, read))))
        }
        None => {
            let ty = types.insert(
                Type { name: None, inner: TypeInner::Vector { size: VectorSize::Quad, scalar: Scalar::F32 } },
                Span::UNDEFINED,
            );
            // A stage has a handful of arguments, far below what a `u32` counts.
            let at = u32::try_from(function.arguments.len()).unwrap_or(u32::MAX);
            function.arguments.push(FunctionArgument {
                name: Some(String::from("aethereal_pick_position")),
                ty,
                binding: Some(Binding::BuiltIn(BuiltIn::Position { invariant: false })),
            });
            (argument_expression(&mut function.expressions, at), None)
        }
    }
}

/// The expression that reads the argument `at` of a function, among `expressions`, appended when there is none.
fn argument_expression(expressions: &mut Arena<Expression>, at: u32) -> Handle<Expression> {
    let found =
        expressions.fetch_if(|expression| matches!(expression, Expression::FunctionArgument(read) if *read == at));

    found.unwrap_or_else(|| expressions.append(Expression::FunctionArgument(at), Span::UNDEFINED))
}

/// Puts `call` before each return in `body` and every block within it, walked with a stack of the blocks still to
/// visit, so a deep nesting costs no frames.
fn before_returns(body: &mut Block, call: &Statement) {
    let mut blocks = vec![body];
    while let Some(block) = blocks.pop() {
        let statements = mem::take(block);
        for (statement, span) in statements.span_into_iter() {
            let returns = matches!(statement, Statement::Return { .. });
            if returns {
                block.push(call.clone(), span);
            }
            block.push(statement, span);
        }
        for statement in block.iter_mut() {
            match statement {
                Statement::Block(inner) => blocks.push(inner),
                Statement::If { accept, reject, .. } => {
                    blocks.push(accept);
                    blocks.push(reject);
                }
                Statement::Switch { cases, .. } => blocks.extend(cases.iter_mut().map(|case| &mut case.body)),
                Statement::Loop { body, continuing, .. } => {
                    blocks.push(body);
                    blocks.push(continuing);
                }
                _ => {}
            }
        }
    }
}

/// The layout of the step's group, which every draw program's pipeline and the glyph program's bind after their own:
/// the probes' buffer, which the fragment stage writes, and the draw's call, at an offset given as each is drawn.
pub fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(PROBES_BYTES as u64),
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(CALL_BYTES as u64),
                },
                count: None,
            },
        ],
    })
}

/// What a draw program may use of `limits`, the step's group and its two buffers taken out: a group, a uniform block,
/// and a storage buffer fewer.
pub fn reserved(limits: &wgpu::Limits) -> wgpu::Limits {
    wgpu::Limits {
        max_bind_groups: limits.max_bind_groups.saturating_sub(1),
        max_uniform_buffers_per_shader_stage: limits.max_uniform_buffers_per_shader_stage.saturating_sub(1),
        max_storage_buffers_per_shader_stage: limits.max_storage_buffers_per_shader_stage.saturating_sub(1),
        ..limits.clone()
    }
}

/// The bytes of the probes' buffer for `points`, each a pixel of the window, the first [`MOST_PROBES`] of them: every
/// stack empty, how many, and each pixel.
#[must_use]
pub fn probes_bytes(points: &[[u32; 2]]) -> Vec<u8> {
    let points = &points[..points.len().min(MOST_PROBES)];
    // At most `MOST_PROBES`.
    let count = u32::try_from(points.len()).unwrap_or(0);
    let mut bytes = vec![0; STACKS_BYTES];
    bytes.extend(count.to_le_bytes());
    bytes.extend([0; 4]);
    for [x, y] in points {
        bytes.extend(x.to_le_bytes());
        bytes.extend(y.to_le_bytes());
    }
    bytes.resize(PROBES_BYTES, 0);

    bytes
}

/// The bytes of a draw's call: its number, `call`, and the scale of its pass's target to the window, `scale`.
#[must_use]
pub fn call_bytes(call: u32, scale: [f32; 2]) -> [u8; CALL_BYTES] {
    let mut bytes = [0; CALL_BYTES];
    bytes[..4].copy_from_slice(&call.to_le_bytes());
    bytes[8..12].copy_from_slice(&scale[0].to_le_bytes());
    bytes[12..].copy_from_slice(&scale[1].to_le_bytes());

    bytes
}

/// The stack of each of `probes` probes in `stacks`, the stacks as the GPU left them: each the calls it holds, the
/// highest first, with those of `hits` that hold the probe, each a call's number beside whether it holds the probe at
/// `at`, merged in, and the highest [`STACK`] kept. A call is in a stack once.
#[must_use]
pub fn stacks(stacks: &[u8], probes: usize, hits: impl Fn(usize) -> Vec<u32>) -> Vec<Vec<u32>> {
    let words: Vec<u32> =
        stacks.chunks_exact(4).map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]])).collect();

    (0..probes.min(MOST_PROBES))
        .map(|probe| {
            let held = words.get(probe * STACK..(probe + 1) * STACK).unwrap_or_default();
            let mut calls: Vec<u32> = held.iter().copied().chain(hits(probe)).filter(|call| *call != 0).collect();
            calls.sort_unstable_by(|one, other| other.cmp(one));
            calls.dedup();
            calls.truncate(STACK);
            calls
        })
        .collect()
}
