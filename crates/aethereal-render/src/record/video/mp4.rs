//! The MP4 a clip is written as: one H.264 track at a constant rate, its frames in one `mdat` as they are encoded and
//! its `moov` after them once the last is (ISO/IEC 14496-12, and 14496-15 for H.264's `avc1` and `avcC`).

use super::Unencodable;
use core::iter;

/// The bytes of the `ftyp` that opens the file, before the `mdat`.
const FTYP_BYTES: usize = 32;

/// The bytes of the `mdat`'s header: a size of one, its name, and its size in 64 bits, so it may pass four gigabytes.
const MDAT_HEADER_BYTES: usize = 16;

/// The units a second the movie's own times are in, as players show them.
const MOVIE_TIMESCALE: u32 = 1000;

/// The NAL unit types of H.264 the writer reads: an IDR picture's slice, which a player may start at, and the sequence
/// and picture parameter sets, which go in the `avcC` rather than among the frames.
const NAL_IDR: u8 = 5;
const NAL_SPS: u8 = 7;
const NAL_PPS: u8 = 8;

/// A clip's MP4 as it is written: the `ftyp`, then the `mdat` with each frame so far, each NAL unit of it after its
/// length in four bytes; each frame's bytes, and the number of each a player may start at, from one; the sequence and
/// picture parameter sets; and the clip's rate and size.
pub struct Mp4 {
    bytes: Vec<u8>,
    sizes: Vec<u32>,
    syncs: Vec<u32>,
    sps: ParameterSet,
    pps: ParameterSet,
    frames_per_second: u32,
    width: u32,
    height: u32,
}

/// A parameter set the `avcC` holds: the first of its kind the encoder wrote, which every later one repeats, as it
/// numbers them all alike.
enum ParameterSet {
    /// No frame has brought one yet.
    Unseen,
    Seen(Vec<u8>),
}

impl ParameterSet {
    /// Keeps `nal` when none is kept.
    fn see(&mut self, nal: &[u8]) {
        if matches!(self, Self::Unseen) {
            *self = Self::Seen(nal.to_vec());
        }
    }
}

impl Mp4 {
    /// An MP4 of no frames yet, at `frames_per_second`, each `width` by `height`.
    pub fn new(frames_per_second: u32, width: u32, height: u32) -> Self {
        let mut bytes = Vec::new();
        boxed(&mut bytes, *b"ftyp", |ftyp| {
            ftyp.extend_from_slice(b"isom");
            put_u32(ftyp, 0x200);
            ftyp.extend_from_slice(b"isomiso2avc1mp41");
        });
        put_u32(&mut bytes, 1);
        bytes.extend_from_slice(b"mdat");
        put_u64(&mut bytes, 0);

        let (sizes, syncs, sps, pps) = (Vec::new(), Vec::new(), ParameterSet::Unseen, ParameterSet::Unseen);

        Self { bytes, sizes, syncs, sps, pps, frames_per_second, width, height }
    }

    /// The bytes written so far.
    pub const fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Writes one frame, `stream`, its NAL units each after an Annex B start code as the encoder writes them: its
    /// parameter sets are kept for the `avcC`, the first of each, and every other unit goes in the `mdat` after its
    /// length. A frame holding an IDR picture is one a player may start at.
    ///
    /// # Errors
    ///
    /// [`Unencodable::NoPicture`] when nothing but parameter sets was written, as a frame must show something.
    pub fn frame(&mut self, stream: &[u8]) -> Result<(), Unencodable> {
        let begins = self.bytes.len();
        let mut is_sync = false;
        for nal in nal_units(stream) {
            let Some(&header) = nal.first() else {
                continue;
            };
            match header & 0x1f {
                NAL_SPS => self.sps.see(nal),
                NAL_PPS => self.pps.see(nal),
                kind => {
                    is_sync |= kind == NAL_IDR;
                    put_u32(&mut self.bytes, u32::try_from(nal.len()).map_err(|_| Unencodable::PastMostBytes)?);
                    self.bytes.extend_from_slice(nal);
                }
            }
        }

        let size = self.bytes.len() - begins;
        if size == 0 {
            return Err(Unencodable::NoPicture);
        }
        self.sizes.push(u32::try_from(size).map_err(|_| Unencodable::PastMostBytes)?);
        if is_sync {
            self.syncs.push(u32::try_from(self.sizes.len()).unwrap_or(u32::MAX));
        }

        Ok(())
    }

    /// The file whole: the `mdat`'s size written, and the `moov` after it, which says where each frame is.
    ///
    /// # Errors
    ///
    /// [`Unencodable::NoParameterSets`] when no frame brought a sequence and a picture parameter set, which a player
    /// needs to decode any frame.
    pub fn finish(mut self) -> Result<Vec<u8>, Unencodable> {
        let (ParameterSet::Seen(sps), ParameterSet::Seen(pps)) = (&self.sps, &self.pps) else {
            return Err(Unencodable::NoParameterSets);
        };
        let moov = self.moov(sps, pps);

        let mdat_bytes = u64::try_from(self.bytes.len() - FTYP_BYTES).unwrap_or(u64::MAX);
        self.bytes[FTYP_BYTES + 8..FTYP_BYTES + MDAT_HEADER_BYTES].copy_from_slice(&mdat_bytes.to_be_bytes());
        self.bytes.extend_from_slice(&moov);

        Ok(self.bytes)
    }

    /// The `moov`: the movie's header and its one track.
    fn moov(&self, sps: &[u8], pps: &[u8]) -> Vec<u8> {
        let frames = u32::try_from(self.sizes.len()).unwrap_or(u32::MAX);
        let movie_duration = u64::from(frames) * u64::from(MOVIE_TIMESCALE) / u64::from(self.frames_per_second.max(1));
        let movie_duration = u32::try_from(movie_duration).unwrap_or(u32::MAX);

        let mut moov = Vec::new();
        boxed(&mut moov, *b"moov", |moov| {
            full(moov, *b"mvhd", 0, |mvhd| {
                put_u32s(mvhd, &[0, 0, MOVIE_TIMESCALE, movie_duration, 0x0001_0000]);
                put_u16(mvhd, 0x0100);
                put_u16(mvhd, 0);
                put_u32s(mvhd, &[0, 0]);
                put_u32s(mvhd, &IDENTITY);
                put_u32s(mvhd, &[0; 6]);
                put_u32(mvhd, 2);
            });
            boxed(moov, *b"trak", |trak| {
                full(trak, *b"tkhd", 3, |tkhd| {
                    put_u32s(tkhd, &[0, 0, 1, 0, movie_duration, 0, 0]);
                    put_u16(tkhd, 0);
                    put_u16(tkhd, 0);
                    put_u16(tkhd, 0);
                    put_u16(tkhd, 0);
                    put_u32s(tkhd, &IDENTITY);
                    put_u32s(tkhd, &[self.width << 16, self.height << 16]);
                });
                boxed(trak, *b"mdia", |mdia| self.mdia(mdia, frames, sps, pps));
            });
        });

        moov
    }

    /// The track's media: its header, at a timescale of its rate so each frame lasts one unit, its handler, and the
    /// tables of its frames.
    fn mdia(&self, mdia: &mut Vec<u8>, frames: u32, sps: &[u8], pps: &[u8]) {
        full(mdia, *b"mdhd", 0, |mdhd| {
            put_u32s(mdhd, &[0, 0, self.frames_per_second, frames]);
            // The language `und`, five bits a letter.
            put_u16(mdhd, 0x55c4);
            put_u16(mdhd, 0);
        });
        full(mdia, *b"hdlr", 0, |hdlr| {
            put_u32(hdlr, 0);
            hdlr.extend_from_slice(b"vide");
            put_u32s(hdlr, &[0, 0, 0]);
            hdlr.extend_from_slice(b"VideoHandler\0");
        });
        boxed(mdia, *b"minf", |minf| {
            full(minf, *b"vmhd", 1, |vmhd| put_u16s(vmhd, &[0, 0, 0, 0]));
            boxed(minf, *b"dinf", |dinf| {
                full(dinf, *b"dref", 0, |dref| {
                    put_u32(dref, 1);
                    full(dref, *b"url ", 1, |_| {});
                });
            });
            boxed(minf, *b"stbl", |stbl| self.stbl(stbl, frames, sps, pps));
        });
    }

    /// The tables of the track's frames: how to decode them, how long each lasts, which a player may start at, and
    /// how large each is and where, each frame a chunk of its own.
    fn stbl(&self, stbl: &mut Vec<u8>, frames: u32, sps: &[u8], pps: &[u8]) {
        full(stbl, *b"stsd", 0, |stsd| {
            put_u32(stsd, 1);
            boxed(stsd, *b"avc1", |avc1| self.avc1(avc1, sps, pps));
        });
        full(stbl, *b"stts", 0, |stts| put_u32s(stts, &[1, frames, 1]));
        full(stbl, *b"stss", 0, |stss| {
            put_u32(stss, u32::try_from(self.syncs.len()).unwrap_or(u32::MAX));
            put_u32s(stss, &self.syncs);
        });
        full(stbl, *b"stsc", 0, |stsc| put_u32s(stsc, &[1, 1, 1, 1]));
        full(stbl, *b"stsz", 0, |stsz| {
            put_u32s(stsz, &[0, frames]);
            put_u32s(stsz, &self.sizes);
        });
        full(stbl, *b"co64", 0, |co64| {
            put_u32(co64, frames);
            let mut offset = u64::try_from(FTYP_BYTES + MDAT_HEADER_BYTES).unwrap_or(u64::MAX);
            for &size in &self.sizes {
                put_u64(co64, offset);
                offset += u64::from(size);
            }
        });
    }

    /// The track's one sample entry: H.264 at the clip's size, with its parameter sets in the `avcC`.
    fn avc1(&self, avc1: &mut Vec<u8>, sps: &[u8], pps: &[u8]) {
        avc1.extend_from_slice(&[0; 6]);
        put_u16(avc1, 1);
        put_u16s(avc1, &[0, 0]);
        put_u32s(avc1, &[0, 0, 0]);
        put_u16s(avc1, &[low_u16(self.width), low_u16(self.height)]);
        // 72 dots an inch, each way.
        put_u32s(avc1, &[0x0048_0000, 0x0048_0000, 0]);
        put_u16(avc1, 1);
        avc1.extend_from_slice(&[0; 32]);
        put_u16s(avc1, &[0x0018, 0xffff]);
        boxed(avc1, *b"avcC", |avcc| {
            let profile = sps.get(1..4).unwrap_or(&[0, 0, 0]);
            avcc.push(1);
            avcc.extend_from_slice(profile);
            // Each NAL unit's length in four bytes, then one sequence parameter set.
            avcc.extend_from_slice(&[0xff, 0xe1]);
            put_u16(avcc, low_u16(u32::try_from(sps.len()).unwrap_or(u32::MAX)));
            avcc.extend_from_slice(sps);
            avcc.push(1);
            put_u16(avcc, low_u16(u32::try_from(pps.len()).unwrap_or(u32::MAX)));
            avcc.extend_from_slice(pps);
        });
    }
}

/// The unit transform, in 16.16 and 2.30 fixed point: no rotation, scale or shift.
const IDENTITY: [u32; 9] = [0x0001_0000, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000];

/// The NAL units of `stream`, an Annex B byte stream: each from after a start code, `00 00 01`, to the next, without
/// the zeros that end it, which belong to a four-byte start code or pad the stream. No unit holds `00 00 01`, as H.264
/// escapes it.
fn nal_units(stream: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut rest = stream;
    iter::from_fn(move || {
        let begins = start_code(rest)? + START_CODE.len();
        let body = &rest[begins..];
        let ends = start_code(body).unwrap_or(body.len());
        let unit = &body[..ends];
        let kept = unit.len() - unit.iter().rev().take_while(|&&byte| byte == 0).count();
        rest = &body[ends..];

        Some(&unit[..kept])
    })
}

/// The three bytes that begin each NAL unit in an Annex B stream.
const START_CODE: [u8; 3] = [0, 0, 1];

/// Where the first start code in `bytes` begins, when there is one.
fn start_code(bytes: &[u8]) -> Option<usize> {
    bytes.windows(START_CODE.len()).position(|window| window == START_CODE)
}

/// Writes a box named `name` onto `out`: its size, its name, then what `body` writes.
fn boxed(out: &mut Vec<u8>, name: [u8; 4], body: impl FnOnce(&mut Vec<u8>)) {
    let begins = out.len();
    put_u32(out, 0);
    out.extend_from_slice(&name);
    body(out);
    let size = u32::try_from(out.len() - begins).unwrap_or(u32::MAX);
    out[begins..begins + 4].copy_from_slice(&size.to_be_bytes());
}

/// Writes a full box named `name` onto `out`: a box whose body begins with its version, always zero here, and
/// `flags`.
fn full(out: &mut Vec<u8>, name: [u8; 4], flags: u32, body: impl FnOnce(&mut Vec<u8>)) {
    boxed(out, name, |inner| {
        put_u32(inner, flags & 0x00ff_ffff);
        body(inner);
    });
}

/// `value` in sixteen bits, or the most they hold: a side or a parameter set the clip's own bounds keep within them.
fn low_u16(value: u32) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_u16s(out: &mut Vec<u8>, values: &[u16]) {
    for &value in values {
        put_u16(out, value);
    }
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_u32s(out: &mut Vec<u8>, values: &[u32]) {
    for &value in values {
        put_u32(out, value);
    }
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}
