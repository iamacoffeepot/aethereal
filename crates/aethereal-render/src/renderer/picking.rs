//! The renderer's side of picking (ADR-0055): each window's probes as its actor sends them, written for the GPU as a
//! frame is drawn; each draw's call, numbered in the order the frame issues them, keyed by its pass, and holding the
//! shift that moves it to its list's origin; and what the GPU wrote at the probes, read back and answered to the
//! window as the stacks of the entries drawn there, each with the origin it was drawn at.

use crate::draw_list::{self, Picks, Reaching};
use crate::frame::Frame;
use crate::picking::{self, CALL_BYTES, MOST_CALLS, MOST_PROBES, PROBES_BYTES, STACKS_BYTES};
use aethereal_math::{Rect2, Vec2};
use aethereal_window::{Picked, Pressed, Probed, Reach, WindowId};
use std::iter;

/// What every frame's picking draws with: the layout of the step's group, the buffer each draw's call is bound from,
/// at its own offset, `stride` bytes after the last's, room for `room` calls, and a buffer of no probes, which the
/// step is bound to in a frame that ends on a texture or a window probed nowhere.
pub(super) struct Picker {
    layout: wgpu::BindGroupLayout,
    stride: usize,
    calls: wgpu::Buffer,
    room: usize,
    unprobed: wgpu::Buffer,
}

impl Picker {
    /// What picking draws with on `device`, room for one call.
    pub fn new(device: &wgpu::Device) -> Self {
        // The alignment is a few hundred bytes.
        let stride =
            usize::try_from(device.limits().min_uniform_buffer_offset_alignment).unwrap_or(256).max(CALL_BYTES);
        let unprobed = probes_buffer(device);

        Self { layout: picking::layout(device), stride, calls: calls_buffer(device, stride, 1), room: 1, unprobed }
    }

    /// The layout of the step's group, which a pipeline binds after its own groups.
    pub const fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    /// Makes room for `calls` calls, the buffer made again, larger, when it has less.
    pub fn fit(&mut self, device: &wgpu::Device, calls: usize) {
        if calls <= self.room {
            return;
        }
        let room = calls.next_power_of_two();
        self.calls = calls_buffer(device, self.stride, room);
        self.room = room;
    }

    /// The step's group binding the probes of `probes`, a window's buffer, or none, and the calls.
    fn group(&self, device: &wgpu::Device, probes: &wgpu::Buffer) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: probes.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.calls,
                        offset: 0,
                        size: wgpu::BufferSize::new(CALL_BYTES as u64),
                    }),
                },
            ],
        })
    }

    /// The picking of a `Render` that probes `probed`, each window's probes beside its buffer, with room for `calls`
    /// calls, on `device`.
    pub fn picking(&self, device: &wgpu::Device, probed: Vec<(Probes, &wgpu::Buffer)>, calls: usize) -> Picking {
        let frames = probed
            .into_iter()
            .map(|(probes, buffer)| Framed {
                probes,
                group: self.group(device, buffer),
                buffer: buffer.clone(),
                calls: Vec::new(),
                hits: Vec::new(),
            })
            .collect();
        let room = calls.min(self.room);

        Picking {
            stride: self.stride,
            room,
            records: vec![0; self.stride],
            unprobed: self.group(device, &self.unprobed),
            frames,
            at: At::Unprobed,
        }
    }

    /// Writes `records`, each call's at its offset, into the calls' buffer, before the commands that read them are
    /// submitted.
    pub fn write(&self, queue: &wgpu::Queue, records: &[u8]) {
        queue.write_buffer(&self.calls, 0, records);
    }
}

/// A buffer of the calls, `room` of them each `stride` bytes after the last.
fn calls_buffer(device: &wgpu::Device, stride: usize, room: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (stride * room) as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// A buffer of a window's probes, as the step reads and writes it: none, as it is made.
fn probes_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: PROBES_BYTES as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}

/// A window's probes, as the renderer keeps them: the buffer the step reads them from and writes their stacks into,
/// where the pointer was last, the presses since the last frame drawn, and the number of the last pointer event heard.
pub(super) struct Probing {
    buffer: wgpu::Buffer,
    pointer: Pointer,
    presses: Vec<Probe>,
    through: u64,
}

/// Where a window's pointer is, as its probes say.
#[derive(Clone, Copy)]
enum Pointer {
    /// No pointer event has been heard: nothing is probed.
    Unheard,
    At(Probe),
}

/// One probe: the pointer event it is, by its number, and its pixel of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Probe {
    pub at: u64,
    pub pixel: [u32; 2],
}

/// The probes of a window as one frame draws them: the pointer's first, then each press, and the number of the last
/// pointer event heard before it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Probes {
    pub window: WindowId,
    pub points: Vec<Probe>,
    pub through: u64,
}

impl Probing {
    /// No probe heard, on `device`.
    pub fn new(device: &wgpu::Device) -> Self {
        Self { buffer: probes_buffer(device), pointer: Pointer::Unheard, presses: Vec::new(), through: 0 }
    }

    /// Takes in a pointer event the window heard: the pointer is there now, and a press is probed whatever it does
    /// next, as many as fit beside the pointer.
    pub fn heard(&mut self, probed: &Probed) {
        // A pixel's place is whole and not negative: `as` floors it, and saturates what is past the window.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let pixel = [probed.x as u32, probed.y as u32];
        let probe = Probe { at: probed.at, pixel };
        self.pointer = Pointer::At(probe);
        self.through = self.through.max(probed.at);
        let has_room = self.presses.len() < MOST_PROBES - 1;
        if probed.press && has_room {
            self.presses.push(probe);
        }
    }

    /// The probes of `window` the next frame draws, written for the GPU on `queue`, with the buffer they are written
    /// in, and the presses taken: none while no pointer event has been heard.
    pub fn take(&mut self, queue: &wgpu::Queue, window: WindowId) -> Option<(Probes, &wgpu::Buffer)> {
        let Pointer::At(pointer) = self.pointer else {
            return None;
        };
        let points: Vec<Probe> = iter::once(pointer).chain(self.presses.drain(..)).collect();
        let pixels: Vec<[u32; 2]> = points.iter().map(|probe| probe.pixel).collect();
        queue.write_buffer(&self.buffer, 0, &picking::probes_bytes(&pixels));

        Some((Probes { window, points, through: self.through }, &self.buffer))
    }
}

/// What a `Render` records for picking: each call's record, `stride` bytes after the last, room for `room` of them,
/// the first no call; the group a frame probed nowhere binds; each frame of a window it probes; and which of them the
/// draws now recorded are in.
pub(super) struct Picking {
    stride: usize,
    room: usize,
    records: Vec<u8>,
    unprobed: wgpu::BindGroup,
    frames: Vec<Framed>,
    at: At,
}

/// The frame the draws now recorded are in: one probed nowhere, or the frame at its place among those probed.
#[derive(Clone, Copy)]
enum At {
    Unprobed,
    Probed(usize),
}

/// A draw pass of a frame as picking keys what it draws and its calls place it: its place among the frame's draw
/// passes, from the first, whether it tests depth, so what it draws ranks nearest first, the scale of its target to
/// the window, the size of its target in its pixels, across and down, and how far one of the window's pixels moves a
/// draw in clip space, across and down: `2 / width` and `-2 / height` of the frame's end.
#[derive(Clone, Copy)]
pub(super) struct InPass {
    pub at: u32,
    pub tests_depth: bool,
    pub scale: [f32; 2],
    pub size: [u32; 2],
    pub per_pixel: [f32; 2],
}

impl InPass {
    /// The shift in clip space that moves a draw of the pass to `origin`, in the window's pixels.
    fn shift(self, origin: Vec2) -> [f32; 2] {
        [origin.x * self.per_pixel[0], origin.y * self.per_pixel[1]]
    }
}

/// A window's main frame as a `Render` picks it: its probes and their buffer, the group binding them, the entry each
/// call reaches, by its number less one, and each hit, by its key, over its rect in the window's pixels.
pub(super) struct Framed {
    probes: Probes,
    buffer: wgpu::Buffer,
    group: wgpu::BindGroup,
    calls: Vec<Reach>,
    hits: Vec<(u32, Rect2)>,
}

impl Picking {
    /// Records the draws from now as `frame`'s: a window's main frame it probes, or one probed nowhere.
    pub fn enter(&mut self, frame: Frame) {
        let probed = match frame {
            Frame::Main(window) => self.frames.iter().position(|framed| framed.probes.window == window),
            Frame::Texture(_) => None,
        };
        self.at = probed.map_or(At::Unprobed, At::Probed);
    }

    /// The step's group the draws now recorded bind.
    pub fn group(&self) -> &wgpu::BindGroup {
        match self.at {
            At::Unprobed => &self.unprobed,
            At::Probed(at) => &self.frames[at].group,
        }
    }

    /// The offset of the next draw's call, which reaches what `picks` says, drawn in `pass` with its list's origin at
    /// `origin`, in the window's pixels: the first, no call and no shift, for a draw at the window's origin the pointer
    /// does not reach, or past the room. A draw the pointer does not reach, in a frame probed nowhere, or past the
    /// calls a frame numbers, is keyed no call, and still moved to its origin.
    pub fn call(&mut self, picks: Picks, origin: Vec2, pass: InPass) -> u32 {
        let slot = self.records.len() / self.stride;
        let has_room = slot < self.room;
        if !has_room {
            return 0;
        }
        let key = self.number(picks, origin, pass);
        let shift = pass.shift(origin);
        let unmoved = shift == [0.0, 0.0];
        let plain = key == 0 && unmoved;
        if plain {
            return 0;
        }
        let mut record = picking::call_bytes(key, pass.tests_depth, pass.scale, shift).to_vec();
        record.resize(self.stride, 0);
        self.records.extend(record);

        // The calls' buffer is far smaller than a `u32` counts.
        u32::try_from(slot * self.stride).unwrap_or(0)
    }

    /// The key of the next call, which reaches what `picks` says, drawn in `pass` with its list's origin at `origin`,
    /// its entry numbered: 0 for one the pointer does not reach, in a frame probed nowhere, or past the calls a frame
    /// numbers.
    fn number(&mut self, picks: Picks, origin: Vec2, pass: InPass) -> u32 {
        let (At::Probed(at), Picks::Reaches(reaching)) = (self.at, picks) else {
            return 0;
        };
        let framed = &mut self.frames[at];
        framed.numbered(reaching, origin).map_or(0, |call| picking::key(pass.at, call))
    }

    /// Takes in a hit, which reaches what `picks` says within `rect`, in the window's pixels, its list's origin at
    /// `origin`, numbered as the next call, in `pass`: keyed at the farthest, as it has no depth.
    pub fn hit(&mut self, rect: Rect2, (picks, origin): (Picks, Vec2), pass: InPass) {
        let (At::Probed(at), Picks::Reaches(reaching)) = (self.at, picks) else {
            return;
        };
        let framed = &mut self.frames[at];
        if let Some(call) = framed.numbered(reaching, origin) {
            framed.hits.push((picking::key(pass.at, call), rect));
        }
    }

    /// The records of every call, each at its offset, and each frame probed, with what it picked.
    pub fn finish(self) -> (Vec<u8>, Vec<Framed>) {
        (self.records, self.frames)
    }
}

impl Framed {
    /// Numbers the next call, which reaches `reaching`, drawn with its list's origin at `origin`: none past the calls a
    /// frame numbers.
    fn numbered(&mut self, reaching: Reaching, origin: Vec2) -> Option<u32> {
        if self.calls.len() >= MOST_CALLS {
            return None;
        }
        self.calls.push(reaching.at(origin));

        // At most `MOST_CALLS`.
        u32::try_from(self.calls.len()).ok()
    }

    /// Copies its stacks, as the GPU left them, on `encoder` into a buffer to read back once it is mapped.
    pub fn copied(self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder) -> Stacks {
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: STACKS_BYTES as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(&self.buffer, 0, &read, 0, STACKS_BYTES as u64);

        Stacks { buffer: read, probes: self.probes, calls: self.calls, hits: self.hits }
    }
}

/// A frame's stacks, copied into `buffer` to read back, with its probes, the entry each call reaches, by its number
/// less one, and each hit, by its key.
pub(super) struct Stacks {
    pub buffer: wgpu::Buffer,
    probes: Probes,
    calls: Vec<Reach>,
    hits: Vec<(u32, Rect2)>,
}

impl Stacks {
    /// The window whose probes they are, and what was drawn at them, of `bytes`, the stacks as the GPU left them: each
    /// probe's calls with the hits that hold its pixel, the highest first, each as the entry it reaches.
    pub fn picked(&self, bytes: &[u8]) -> (WindowId, Picked) {
        let Probes { window, points, through } = &self.probes;
        let hits = |probe: usize| {
            let pixel = points[probe].pixel;
            self.hits.iter().filter(|(_, rect)| draw_list::holds(*rect, pixel)).map(|(key, _)| *key).collect()
        };
        let reaches = |calls: Vec<u32>| -> Vec<Reach> {
            calls.into_iter().filter_map(|call| self.calls.get(call.checked_sub(1)? as usize).copied()).collect()
        };
        let mut stacks = picking::stacks(bytes, points.len(), hits).into_iter().map(reaches);
        let pointer = stacks.next().unwrap_or_default();
        let presses = points.iter().skip(1).zip(stacks).map(|(press, stack)| Pressed { at: press.at, stack }).collect();

        (*window, Picked { through: *through, pointer, presses })
    }
}
