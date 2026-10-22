//! What the screen's instances hold: a record for each rectangle drawn, its centre and half its size in court units,
//! the plane it lies in, and its colour, as the program reads one per instance (ADR-0023).

use aethereal_render::mesh::{Attribute, VertexFormat, VertexLayout};

/// A colour, red, green, blue, and alpha, a byte each.
pub type Colour = [u8; 4];

/// How many bytes a record takes.
const STRIDE: u32 = 24;

/// The planes a rectangle lies in, nearest first: depth orders what is drawn over what (ADR-0023), as the order of a
/// pass's placements is not promised.
#[derive(Clone, Copy)]
pub enum Plane {
    /// The paddles and the ball, over the score as the ball passes it.
    Play,
    /// The trail a spinning ball leaves, under the ball.
    Trail,
    /// The board's words and the score.
    Text,
    /// The panel the board's words stand on, over the court's lines.
    Panel,
    /// The court's walls and its centre line.
    Lines,
    /// The court's floor.
    Floor,
}

impl Plane {
    /// The depth the plane is drawn at, from nought, nearest, to one.
    const fn depth(self) -> f32 {
        match self {
            Self::Play => 0.1,
            Self::Trail => 0.15,
            Self::Text => 0.2,
            Self::Panel => 0.3,
            Self::Lines => 0.6,
            Self::Floor => 0.9,
        }
    }
}

/// How a record is laid out, as the program reads it per instance: its centre, half its size, its depth, and its
/// colour, after the quad's corner at location nought.
pub fn layout() -> VertexLayout {
    let centre = Attribute { location: 1, format: VertexFormat::Float32x2, offset: 0 };
    let half = Attribute { location: 2, format: VertexFormat::Float32x2, offset: 8 };
    let depth = Attribute { location: 3, format: VertexFormat::Float32, offset: 16 };
    let colour = Attribute { location: 4, format: VertexFormat::Unorm8x4, offset: 20 };

    VertexLayout { stride: STRIDE, attributes: vec![centre, half, depth, colour] }
}

/// Records, as instances are written: their bytes in order, and how many there are.
pub struct Records {
    bytes: Vec<u8>,
    count: u32,
}

impl Records {
    /// No records.
    pub const fn new() -> Self {
        Self { bytes: Vec::new(), count: 0 }
    }

    /// Adds a rectangle centred at `x`, `y`, `half_width` and `half_height` from its centre to its sides, in court
    /// units, lying in `plane`, coloured `colour`.
    pub fn rect(&mut self, x: f32, y: f32, half_width: f32, half_height: f32, plane: Plane, colour: Colour) {
        for value in [x, y, half_width, half_height, plane.depth()] {
            self.bytes.extend_from_slice(&value.to_ne_bytes());
        }
        self.bytes.extend_from_slice(&colour);
        self.count += 1;
    }

    /// How many records there are.
    pub const fn count(&self) -> u32 {
        self.count
    }

    /// Their bytes, as a blob of them is put.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}
