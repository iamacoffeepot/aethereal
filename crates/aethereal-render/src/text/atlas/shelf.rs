//! A shelf packer: boxes laid left to right along shelves, each shelf as tall as the first box that opened it, and
//! shelves stacked top to bottom. A box goes on the lowest shelf tall enough with room left, else on a new shelf.
//! Glyphs of one font at one size are much alike in height, so few texels are left between them.

/// One shelf: where its top is, how tall it is, and how far along it is filled.
#[derive(Debug, Clone, Copy)]
struct Shelf {
    y: u32,
    height: u32,
    filled: u32,
}

/// Shelves in a square of `side` texels.
#[derive(Debug)]
pub struct Shelves {
    side: u32,
    shelves: Vec<Shelf>,
}

impl Shelves {
    /// No shelves, in a square of `side` texels.
    #[must_use]
    pub const fn new(side: u32) -> Self {
        Self { side, shelves: Vec::new() }
    }

    /// The square grown to `side` texels, the shelves where they were: each has more room to its right, and there is
    /// room below them for more.
    pub const fn grow(&mut self, side: u32) {
        self.side = side;
    }

    /// Where the top left of a box `width` by `height` goes: on the shortest shelf as tall as it with room left, or on
    /// a new shelf below the last. None when it fits nowhere.
    pub fn place(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        let side = self.side;
        let roomy = self
            .shelves
            .iter_mut()
            .filter(|shelf| {
                let tall = shelf.height >= height;
                let room = shelf.filled.saturating_add(width) <= side;
                tall && room
            })
            .min_by_key(|shelf| shelf.height);
        if let Some(shelf) = roomy {
            let at = (shelf.filled, shelf.y);
            shelf.filled += width;
            return Some(at);
        }

        let below = self.shelves.last().map_or(0, |last| last.y + last.height);
        let fits = width <= side && below.saturating_add(height) <= side;
        if !fits {
            return None;
        }
        self.shelves.push(Shelf { y: below, height, filled: width });

        Some((0, below))
    }
}
