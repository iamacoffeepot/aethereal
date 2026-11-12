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
/// at its own offset, `stride` bytes after the last, with its room, at most `most` calls, and a buffer of no probes,
/// which the step is bound to in a frame that ends on a texture or a window probed nowhere.
pub(super) struct Picker {
    layout: wgpu::BindGroupLayout,
    stride: usize,
    most: usize,
    calls: Calls,
    unprobed: wgpu::Buffer,
}

/// The buffer of the calls, with room for `room` of them.
pub(super) struct Calls {
    buffer: wgpu::Buffer,
    room: usize,
}

impl Picker {
    /// What picking draws with on `device`, room for one call, and at most as many as the device's largest buffer
    /// holds, each at an offset a `u32` counts, as a draw binds it.
    pub fn new(device: &wgpu::Device) -> Self {
        // The alignment is a few hundred bytes.
        let stride =
            usize::try_from(device.limits().min_uniform_buffer_offset_alignment).unwrap_or(256).max(CALL_BYTES);
        let largest = device.limits().max_buffer_size.min(u64::from(u32::MAX));
        // At most a `u32`'s count of bytes, which a `usize` holds on every target.
        let most = usize::try_from(largest).unwrap_or(usize::MAX) / stride;
        let calls = Calls { buffer: calls_buffer(device, stride, 1), room: 1 };
        let unprobed = probes_buffer(device);

        Self { layout: picking::layout(device), stride, most, calls, unprobed }
    }

    /// The layout of the step's group, which a pipeline binds after its own groups.
    pub const fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    /// The picking of a `Render` that probes `probed`, each window's probes beside its buffer: no call recorded yet
    /// but the first, which is none.
    pub fn picking(&self, probed: Vec<(Probes, &wgpu::Buffer)>) -> Picking {
        let frames = probed
            .into_iter()
            .map(|(probes, buffer)| Framed { probes, buffer: buffer.clone(), calls: Vec::new(), hits: Vec::new() })
            .collect();

        Picking {
            stride: self.stride,
            most: self.most,
            records: vec![0; self.stride],
            frames,
            at: At::Unprobed,
            past: 0,
        }
    }

    /// Binds what `picking` recorded, on `gpu`, once the walk has recorded every call of its `Render`, before any draw
    /// is recorded: the calls' buffer fitted to every record, made again, larger, when it has less, the records
    /// written into it, and the step's group of each frame probed, and of one probed nowhere, made binding it. So no
    /// draw binds an offset past the buffer.
    pub fn bound(&self, (device, queue): (&wgpu::Device, &wgpu::Queue), picking: Picking) -> Recorded {
        let Picking { records, frames, past, .. } = picking;
        let needed = records.len() / self.stride;
        let calls = if needed <= self.calls.room {
            Calls { buffer: self.calls.buffer.clone(), room: self.calls.room }
        } else {
            let room = needed.next_power_of_two().min(self.most);
            Calls { buffer: calls_buffer(device, self.stride, room), room }
        };
        queue.write_buffer(&calls.buffer, 0, &records);
        let unprobed = self.group(device, &self.unprobed, &calls.buffer);
        let frames = frames
            .into_iter()
            .map(|framed| {
                let group = self.group(device, &framed.buffer, &calls.buffer);
                (framed, group)
            })
            .collect();

        Recorded { calls, unprobed, frames, past }
    }

    /// Keeps `calls`, fitted to a `Render`'s records, for the next `Render` to fit from, once the draws that bind it
    /// are recorded.
    pub fn keep(&mut self, calls: Calls) {
        self.calls = calls;
    }

    /// The step's group binding the probes of `probes`, a window's buffer, or none, and the calls of `calls`.
    fn group(&self, device: &wgpu::Device, probes: &wgpu::Buffer, calls: &wgpu::Buffer) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: probes.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: calls,
                        offset: 0,
                        size: wgpu::BufferSize::new(CALL_BYTES as u64),
                    }),
                },
            ],
        })
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

/// What a `Render`'s walk records for picking, before any draw is recorded: each call's record, `stride` bytes after
/// the last, the first no call, at most `most` of them; each frame of a window it probes; which of them the calls now
/// recorded are in; and how many draws were refused, past the most.
pub(super) struct Picking {
    stride: usize,
    most: usize,
    records: Vec<u8>,
    frames: Vec<Framed>,
    at: At,
    past: usize,
}

/// A draw refused its call, past the most calls a `Render`'s buffer holds: not drawn, rather than drawn where its list
/// is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PastRoom;

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

/// A window's main frame as a `Render` picks it: its probes and their buffer, the entry each call reaches, by its
/// number less one, and each hit, by its key, over its rect in the window's pixels.
pub(super) struct Framed {
    probes: Probes,
    buffer: wgpu::Buffer,
    calls: Vec<Reach>,
    hits: Vec<(u32, Rect2)>,
}

/// What a `Render`'s walk recorded for picking, bound for its draws: the calls' buffer, fitted to every record and
/// written, the step's group of a frame probed nowhere, each frame probed beside its group, and how many draws were
/// refused, past the most calls a buffer holds.
pub(super) struct Recorded {
    calls: Calls,
    unprobed: wgpu::BindGroup,
    frames: Vec<(Framed, wgpu::BindGroup)>,
    past: usize,
}

impl Recorded {
    /// The step's group the draws of `frame` bind: a window's main frame's it probes, or that of one probed nowhere.
    pub fn group(&self, frame: Frame) -> &wgpu::BindGroup {
        let probed = match frame {
            Frame::Main(window) => self.frames.iter().find(|(framed, _)| framed.probes.window == window),
            Frame::Texture(_) => None,
        };

        probed.map_or(&self.unprobed, |(_, group)| group)
    }

    /// How many draws were refused, past the most calls a buffer holds, and not drawn.
    pub const fn past(&self) -> usize {
        self.past
    }

    /// The calls' buffer, for the picker to keep, and each frame probed, with what it picked.
    pub fn finish(self) -> (Calls, Vec<Framed>) {
        (self.calls, self.frames.into_iter().map(|(framed, _)| framed).collect())
    }
}

impl Picking {
    /// Records the calls from now as `frame`'s: a window's main frame it probes, or one probed nowhere.
    pub fn enter(&mut self, frame: Frame) {
        let probed = match frame {
            Frame::Main(window) => self.frames.iter().position(|framed| framed.probes.window == window),
            Frame::Texture(_) => None,
        };
        self.at = probed.map_or(At::Unprobed, At::Probed);
    }

    /// The offset of the next draw's call, which reaches what `picks` says, drawn in `pass` with its list's origin at
    /// `origin`, in the window's pixels: the first, no call and no shift, for a draw at the window's origin the pointer
    /// does not reach. A draw the pointer does not reach, in a frame probed nowhere, or past the calls a frame numbers,
    /// is keyed no call, and still moved to its origin. Refused past the most calls a buffer holds, counted.
    pub fn call(&mut self, picks: Picks, origin: Vec2, pass: InPass) -> Result<u32, PastRoom> {
        let key = self.number(picks, origin, pass);
        let shift = pass.shift(origin);
        let unmoved = shift == [0.0, 0.0];
        let plain = key == 0 && unmoved;
        if plain {
            return Ok(0);
        }
        let slot = self.records.len() / self.stride;
        let has_room = slot < self.most;
        // Below the most, every record starts at an offset a `u32` counts.
        let offset = u32::try_from(slot * self.stride);
        let (true, Ok(offset)) = (has_room, offset) else {
            self.past += 1;
            return Err(PastRoom);
        };
        let mut record = picking::call_bytes(key, pass.tests_depth, pass.scale, shift).to_vec();
        record.resize(self.stride, 0);
        self.records.extend(record);

        Ok(offset)
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
