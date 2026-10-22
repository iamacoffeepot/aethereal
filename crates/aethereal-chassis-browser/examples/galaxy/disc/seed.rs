//! The stars as they begin, drawn from a seeded generator: a bright bulge of small round orbits, a disc of orbits
//! thinning outward, and a faint scatter of field stars far out, every one gathered at the centre, so the galaxy
//! opens as a burst that settles into its arms.

use crate::bytes::floats;
use core::f32::consts::TAU;

/// How many stars there are.
pub const STARS: u32 = 1 << 16;

/// The floats of each star in storage, as the shaders' `Star` lays them out: its place and its velocity, its orbit's
/// size, where it is along its orbit, its height above the disc, and its shine.
const FLOATS: usize = 8;

/// The seed the stars are drawn from, so the page shows the same galaxy each time.
const SEED: u64 = 0x5eed_0fab_1e57_a2c0;

/// The share of the stars in the bulge, and of those far out in the field, of each thousand.
const BULGE_PER_MILLE: u32 = 110;
const FIELD_PER_MILLE: u32 = 30;

/// A generator of the stars' chances: `SplitMix64`.
struct Chance {
    state: u64,
}

impl Chance {
    /// The next sixty-four bits.
    const fn bits(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut mixed = self.state;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);

        mixed ^ (mixed >> 31)
    }

    /// A float from zero up to one, of the top twenty-three bits as a float's fraction.
    fn unit(&mut self) -> f32 {
        let fraction = u32::try_from(self.bits() >> 41).unwrap_or(0);

        f32::from_bits(0x3f80_0000 | fraction) - 1.0
    }

    /// A float from zero up to `most`, but never zero, so its logarithm is finite.
    fn above_zero(&mut self, most: f32) -> f32 {
        (1.0 - self.unit()) * most
    }

    /// A float from the normal distribution of no mean and a deviation of `deviation`.
    fn normal(&mut self, deviation: f32) -> f32 {
        let (radius, angle) = (self.above_zero(1.0), self.unit() * TAU);

        (-2.0 * radius.ln()).sqrt() * angle.cos() * deviation
    }

    /// Whether a chance of `per_mille` in a thousand comes up.
    fn per_mille(&mut self, per_mille: u32) -> bool {
        self.bits() % 1000 < u64::from(per_mille)
    }
}

/// One star as it begins: the size of its orbit, its height above the disc, and its shine.
struct Seeded {
    orbit: f32,
    lift: f32,
    shine: f32,
}

/// A star of the bulge: a small round orbit, high above and below the disc, its shine even.
fn bulge(chance: &mut Chance) -> Seeded {
    let orbit = chance.normal(0.07).abs() + 0.004;
    let lift = chance.normal(0.05) * (1.0 - orbit * 4.0).max(0.2);

    Seeded { orbit, lift, shine: chance.unit().mul_add(0.35, 0.15) }
}

/// A star of the disc: its orbit drawn so the disc's stars thin outward, near the plane, and mostly faint.
fn disc(chance: &mut Chance) -> Seeded {
    let orbit = (-0.19 * (chance.above_zero(1.0).ln() + chance.above_zero(1.0).ln())).clamp(0.03, 1.4);
    let thickness = 0.022f32.mul_add((-orbit / 0.15).exp(), 0.012);

    Seeded { orbit, lift: chance.normal(thickness), shine: chance.unit().powi(3) }
}

/// A star of the field: far out, well off the plane, and faint.
fn field(chance: &mut Chance) -> Seeded {
    let orbit = chance.unit().mul_add(1.4, 1.3);

    Seeded { orbit, lift: chance.normal(0.45), shine: chance.unit() * 0.3 }
}

/// The bytes of every star as it begins, as the storage holds them: at the centre and at rest, each on its orbit at
/// an angle of its own.
pub fn stars() -> Vec<u8> {
    let mut chance = Chance { state: SEED };
    let mut values = Vec::with_capacity(STARS as usize * FLOATS);
    for _ in 0..STARS {
        let seeded = if chance.per_mille(BULGE_PER_MILLE) {
            bulge(&mut chance)
        } else if chance.per_mille(FIELD_PER_MILLE) {
            field(&mut chance)
        } else {
            disc(&mut chance)
        };
        let angle = chance.unit() * TAU;
        values.extend([0.0, 0.0, 0.0, 0.0, seeded.orbit, angle, seeded.lift, seeded.shine]);
    }

    floats(values)
}
