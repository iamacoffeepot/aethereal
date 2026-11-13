//! The program a screen draws its quads with: valid WGSL whose layouts are the ones its placements are made with, on
//! the device that draws nothing; and, where the machine has a GPU adapter, the pixels it draws. A screen placed in its
//! caller's coordinates is moved after its program places what it draws (ADR-0055), so these tests draw with the
//! position each record is placed at moved by a shift in clip space as the renderer moves it, and the program sees
//! only the position it placed.

// The GPU tests say which adapter drew, or that there was none and the run was skipped.
#![allow(clippy::print_stderr)]

use super::{
    Binding, Content, FILL_BYTES, FILL_PROGRAM, FRAME_BYTES, PLACE, PROGRAM, RECORD_BYTES, SHADE, corner_layout,
    corners, fill_layout, frame_bytes, layered, record, record_layout, triangles,
};
use crate::style::Edges;
use crate::widget::{Piece, Quad, Sample, Slices};
use aethereal_data::{Budget, from_bytes};
use aethereal_math::{Rect2, Vec2};
use aethereal_render::mesh::VertexLayout;
use aethereal_render::text::Colour;
use aethereal_render::texture::TextureId;
use std::borrow::Cow;

/// Where the program places each record before the renderer moves it.
const PLACED: &str = "return Between(vec4(placed, 0.0, 1.0),";

/// The program, with each position it places moved by a shift in clip space bound in a group after its own, as the
/// renderer moves a draw by its list's origin once the program has placed it.
fn shifted() -> String {
    assert!(PROGRAM.contains(PLACED), "the program places each record at one return");
    let moved = PROGRAM.replace(PLACED, "return Between(vec4(placed + shift.xy, 0.0, 1.0),");

    format!("@group(1) @binding(0) var<uniform> shift: vec4<f32>;\n{moved}")
}

/// A device on the machine's adapter, if it has one, with the adapter's name.
fn adapter_device() -> Result<(wgpu::Device, wgpu::Queue, String), String> {
    let mut described = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
    described.backends &= wgpu::Backends::PRIMARY;
    let instance = wgpu::Instance::new(described);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .map_err(|error| error.to_string())?;
    let name = adapter.get_info().name;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
        .map_err(|error| error.to_string())?;

    Ok((device, queue, name))
}

/// The stride and attributes of `layout`, as wgpu takes them.
fn buffer_layout(layout: &VertexLayout) -> (u64, Vec<wgpu::VertexAttribute>) {
    let attributes = layout
        .attributes
        .iter()
        .map(|attribute| wgpu::VertexAttribute {
            format: attribute.format.wgpu(),
            offset: u64::from(attribute.offset),
            shader_location: attribute.location,
        })
        .collect();

    (u64::from(layout.stride), attributes)
}

/// The program's pipeline on `device`, drawing into Rgba8 by alpha, with its groups' layouts: the frame's size, the
/// texture, and the sampler; then the shift.
fn pipeline(device: &wgpu::Device) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout, wgpu::BindGroupLayout) {
    let entry = |binding, visibility, ty| wgpu::BindGroupLayoutEntry { binding, visibility, ty, count: None };
    let uniform = wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Uniform,
        has_dynamic_offset: false,
        min_binding_size: wgpu::BufferSize::new(u64::from(FRAME_BYTES)),
    };
    let both = wgpu::ShaderStages::VERTEX_FRAGMENT;
    let own = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            entry(0, both, uniform),
            entry(
                1,
                both,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
            ),
            entry(2, both, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
        ],
    });
    let shift = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[entry(0, wgpu::ShaderStages::VERTEX, uniform)],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&own), Some(&shift)],
        immediate_size: 0,
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(Cow::Owned(shifted())),
    });
    let (corner_stride, corner_attributes) = buffer_layout(&corner_layout());
    let (record_stride, record_attributes) = buffer_layout(&record_layout());
    let buffers = [
        Some(wgpu::VertexBufferLayout {
            array_stride: corner_stride,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &corner_attributes,
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: record_stride,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &record_attributes,
        }),
    ];
    let target = wgpu::ColorTargetState {
        format: wgpu::TextureFormat::Rgba8Unorm,
        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
        write_mask: wgpu::ColorWrites::ALL,
    };
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some(PLACE),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &buffers,
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some(SHADE),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(target)],
        }),
        multiview_mask: None,
        cache: None,
    });

    (pipeline, own, shift)
}

/// A texture `width` by `height` of Rgba8 `pixels`, row by row, on `device`.
fn texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    (width, height): (u32, u32),
    pixels: &[[u8; 4]],
) -> wgpu::Texture {
    let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
    let made = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let bytes: Vec<u8> = pixels.iter().flatten().copied().collect();
    let layout =
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width * 4), rows_per_image: Some(height) };
    queue.write_texture(made.as_image_copy(), &bytes, layout, size);

    made
}

/// A buffer on `device` holding `bytes`, used as `usage`.
fn buffer(device: &wgpu::Device, bytes: &[u8], usage: wgpu::BufferUsages) -> wgpu::Buffer {
    let made = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: bytes.len() as u64,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: true,
    });
    made.slice(..).get_mapped_range_mut().expect("a buffer made mapped maps").copy_from_slice(bytes);
    made.unmap();

    made
}

/// What one test draws: a frame `width` by `height` pixels, the quads' records, the texture they sample, `width` by
/// `height` texels of `pixels`, and the shift that moves them, in pixels of the frame.
struct Drawing {
    frame: (u32, u32),
    quads: Vec<Quad>,
    texture: ((u32, u32), Vec<[u8; 4]>),
    shift: Vec2,
}

/// Each pixel of the frame, row by row, once `drawing` is drawn into it, cleared to nothing, on `device`, its texture
/// sampled at the nearest texel.
fn drawn(device: &wgpu::Device, queue: &wgpu::Queue, drawing: &Drawing) -> Vec<Vec<[u8; 4]>> {
    let (width, height) = drawing.frame;
    let (pipeline, own, shifting) = pipeline(device);
    let picture = texture(device, queue, drawing.texture.0, &drawing.texture.1);
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
    let frame = buffer(device, &frame_bytes(width, height), wgpu::BufferUsages::UNIFORM);
    // A frame's sides are a handful of pixels, which an f32 counts exactly.
    #[allow(clippy::cast_precision_loss)]
    let clip_space = [drawing.shift.x * 2.0 / width as f32, -drawing.shift.y * 2.0 / height as f32, 0.0, 0.0];
    let shift_bytes: Vec<u8> = clip_space.iter().flat_map(|value: &f32| value.to_le_bytes()).collect();
    let shift = buffer(device, &shift_bytes, wgpu::BufferUsages::UNIFORM);
    let view = picture.create_view(&wgpu::TextureViewDescriptor::default());
    let own_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &own,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: frame.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
        ],
    });
    let shift_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &shifting,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: shift.as_entire_binding() }],
    });
    let records: Vec<u8> = drawing.quads.iter().flat_map(record).collect();
    let corners = buffer(device, &corners(), wgpu::BufferUsages::VERTEX);
    let instances = buffer(device, &records, wgpu::BufferUsages::VERTEX);
    let indices = buffer(device, &triangles(), wgpu::BufferUsages::INDEX);

    let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let colour = wgpu::RenderPassColorAttachment {
            view: &target_view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(colour)],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &own_group, &[]);
        pass.set_bind_group(1, &shift_group, &[]);
        pass.set_vertex_buffer(0, corners.slice(..));
        pass.set_vertex_buffer(1, instances.slice(..));
        pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
        let count = u32::try_from(drawing.quads.len()).unwrap_or(0);
        pass.draw_indexed(0..6, 0, 0..count);
    }
    let row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let copied = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row * height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let layout = wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(height) };
    encoder.copy_texture_to_buffer(target.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &copied, layout }, size);
    queue.submit([encoder.finish()]);
    copied.map_async(wgpu::MapMode::Read, .., |mapped| mapped.expect("the copy maps"));
    device.poll(wgpu::PollType::wait_indefinitely()).expect("the GPU finishes");
    let bytes = copied.slice(..).get_mapped_range().expect("the mapped copy reads").to_vec();

    bytes
        .chunks_exact(row as usize)
        .map(|line| {
            line[..(width * 4) as usize].chunks_exact(4).map(|pixel| [pixel[0], pixel[1], pixel[2], pixel[3]]).collect()
        })
        .collect()
}

/// A texture's id, which the GPU tests draw nothing by: an id is its number.
fn texture_id() -> TextureId {
    let Ok(id) = from_bytes(&0_u64.to_le_bytes(), &mut Budget::new(8)) else {
        panic!("an id is its number");
    };

    id
}

/// The rect from `min` to `max`.
const fn rect(min: [f32; 2], max: [f32; 2]) -> Rect2 {
    Rect2 { min: Vec2::new(min[0], min[1]), max: Vec2::new(max[0], max[1]) }
}

/// An opaque quad over `quad`, cut to `clip`, sampling white at the texture's whole.
fn solid(quad: Rect2, clip: Rect2) -> Quad {
    let whole = Sample::Texture { texture: texture_id(), uv: rect([0.0, 0.0], [1.0, 1.0]), slices: Slices::NONE };

    Quad {
        rect: quad,
        clip,
        colour: Colour::WHITE,
        border_width: 0.0,
        border_colour: Colour::CLEAR,
        radius: 0.0,
        sample: whole,
    }
}

#[test]
fn the_program_is_valid_and_reads_what_its_placements_give_it() {
    // Catches WGSL the renderer would refuse, and a record or corner layout that does not match what the program
    // reads, on every machine, as CI's has no GPU: the device that draws nothing still checks both as it makes them.
    let (device, _) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);

    let _ = pipeline(&device);

    assert_eq!(pollster::block_on(scope.pop()).map(|error| error.to_string()), None);
}

// A quad cut to a clip in its screen's own pixels keeps to that clip when the screen is moved, as a call that places it
// in its caller's coordinates moves it. Catches a clip compared with the fragment's position in the frame, which here
// cuts the whole quad away; and a clip not applied at all. A machine with no adapter, as CI's, skips it and says so.
#[test]
fn on_a_gpu_the_clip_cuts_in_the_screen_s_own_pixels_however_the_screen_is_moved() {
    let (device, queue, adapter) = match adapter_device() {
        Ok(opened) => opened,
        Err(why) => {
            eprintln!("skipped: no GPU adapter to draw the program on: {why}");
            return;
        }
    };
    eprintln!("ran adapter={adapter:?}");
    let drawing = Drawing {
        frame: (8, 2),
        quads: vec![solid(rect([0.0, 0.0], [4.0, 2.0]), rect([0.0, 0.0], [2.0, 2.0]))],
        texture: ((1, 1), vec![[255; 4]]),
        shift: Vec2::new(4.0, 0.0),
    };

    let pixels = drawn(&device, &queue, &drawing);

    let lit: Vec<bool> = pixels[0].iter().map(|pixel| pixel[3] > 128).collect();
    assert_eq!(lit, vec![false, false, false, false, true, true, false, false], "on {adapter}");
}

// An image drawn three times its texture's size keeps its corners at their texels' size, its slices' bands stretched
// along their length alone and its middle stretched to fill. Catches slices that scale with the quad, so a panel's
// frame thickens as it grows; and a middle that samples past the bands, so the frame repeats inside it. A machine with
// no adapter, as CI's, skips it and says so.
#[test]
fn on_a_gpu_a_nine_slice_keeps_its_corners_unscaled() {
    let (device, queue, adapter) = match adapter_device() {
        Ok(opened) => opened,
        Err(why) => {
            eprintln!("skipped: no GPU adapter to draw the program on: {why}");
            return;
        }
    };
    eprintln!("ran adapter={adapter:?}");
    // Three by three texels: red corners, green edges, and a blue middle.
    let (red, green, blue) = ([255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]);
    let texels = vec![red, green, red, green, blue, green, red, green, red];
    let third = 1.0 / 3.0;
    let slices = Slices { inset: Edges::all(2.0), uv: Edges::all(third) };
    let image = Quad {
        sample: Sample::Texture { texture: texture_id(), uv: rect([0.0, 0.0], [1.0, 1.0]), slices },
        ..solid(rect([0.0, 0.0], [12.0, 12.0]), rect([0.0, 0.0], [12.0, 12.0]))
    };
    let drawing = Drawing { frame: (12, 12), quads: vec![image], texture: ((3, 3), texels), shift: Vec2::ZERO };

    let pixels = drawn(&device, &queue, &drawing);

    // The sampler takes the nearest texel, so each pixel is one texel's colour, judged by the channel that leads it.
    let colour = |x: usize, y: usize| {
        let [r, g, b, _] = pixels[y][x];
        if r > g && r > b {
            "red"
        } else if g > b {
            "green"
        } else {
            "blue"
        }
    };
    assert_eq!(colour(0, 0), "red", "on {adapter}");
    assert_eq!(colour(11, 11), "red", "on {adapter}");
    assert_eq!(colour(3, 0), "green", "the top band begins past the corner's two pixels, on {adapter}");
    assert_eq!(colour(0, 6), "green", "the left band runs down the middle, on {adapter}");
    assert_eq!(colour(6, 6), "blue", "the middle stretches over the rest, on {adapter}");
    assert_eq!(colour(3, 3), "blue", "the middle starts past the bands, on {adapter}");
}

/// The fill program's pipeline on `device`, drawing into Rgba8 by alpha, binding the frame's size alone.
fn fill_pipeline(device: &wgpu::Device) -> wgpu::RenderPipeline {
    let uniform = wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Uniform,
        has_dynamic_offset: false,
        min_binding_size: wgpu::BufferSize::new(u64::from(FRAME_BYTES)),
    };
    let own = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: uniform,
            count: None,
        }],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&own)],
        immediate_size: 0,
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(FILL_PROGRAM)),
    });
    let (corner_stride, corner_attributes) = buffer_layout(&corner_layout());
    let (fill_stride, fill_attributes) = buffer_layout(&fill_layout());
    let buffers = [
        Some(wgpu::VertexBufferLayout {
            array_stride: corner_stride,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &corner_attributes,
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: fill_stride,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &fill_attributes,
        }),
    ];
    let target = wgpu::ColorTargetState {
        format: wgpu::TextureFormat::Rgba8Unorm,
        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
        write_mask: wgpu::ColorWrites::ALL,
    };

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some(PLACE),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &buffers,
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some(SHADE),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(target)],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// A plain fill over `quad`, cut to `clip`: solid, square, and borderless.
fn plain(quad: Rect2, clip: Rect2) -> Quad {
    Quad { sample: Sample::Solid, ..solid(quad, clip) }
}

#[test]
fn the_fill_program_is_valid_and_reads_what_its_placements_give_it() {
    // Catches fill WGSL the renderer would refuse, and a fill record layout that does not match what it reads, on
    // every machine, as the library's program's test does.
    let (device, _) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);

    let _ = fill_pipeline(&device);

    assert_eq!(pollster::block_on(scope.pop()).map(|error| error.to_string()), None);
}

#[test]
fn a_run_of_plain_fills_writes_each_cut_to_its_clip_in_a_quarter_of_a_record() {
    // Catches a run of plain fills, as a chart's hundreds of columns redrawn each frame, written in the library's whole
    // record, four times the bytes a fill needs; a fill not cut to its clip, which the fill program does not cut; one
    // the clip leaves nothing of still written; and a run holding a rounded quad drawn as fills, which squares it.
    let clip = rect([0.0, 0.0], [10.0, 10.0]);
    let fills = vec![
        Piece::Quad(plain(rect([2.0, 2.0], [4.0, 4.0]), clip)),
        Piece::Quad(plain(rect([8.0, 0.0], [12.0, 10.0]), clip)),
        Piece::Quad(plain(rect([20.0, 0.0], [30.0, 10.0]), clip)),
    ];
    let rounded = Quad { radius: 2.0, ..plain(rect([0.0, 0.0], [4.0, 4.0]), clip) };
    let mixed = vec![Piece::Quad(plain(rect([0.0, 0.0], [4.0, 4.0]), clip)), Piece::Quad(rounded)];

    let filled = layered(fills);
    let kept = layered(mixed);

    let floats = |records: &[u8]| -> Vec<f32> {
        records.chunks_exact(4).map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])).collect()
    };
    let [Content::Quads { binding: Binding::Fill, records }] = filled.as_slice() else {
        panic!("plain fills are one layer of fills: {filled:?}");
    };
    assert_eq!(records.len(), 2 * FILL_BYTES as usize, "the fill the clip leaves nothing of is not written");
    let boxes: Vec<f32> = floats(records).chunks_exact(8).flat_map(|fill| fill[..4].to_vec()).collect();
    assert_eq!(boxes, vec![2.0, 2.0, 4.0, 4.0, 8.0, 0.0, 10.0, 10.0]);
    let [Content::Quads { binding: Binding::White, records }] = kept.as_slice() else {
        panic!("a run holding a rounded quad stays the library's: {kept:?}");
    };
    assert_eq!(records.len(), 2 * RECORD_BYTES as usize);
}
