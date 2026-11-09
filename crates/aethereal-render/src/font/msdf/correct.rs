//! Error correction, msdfgen's basic `msdfErrorCorrection`: a texel whose channels differ from a neighbour's so much
//! that sampling between them would make an edge where there is none is a clash, and has each channel set to its
//! median, which loses the corner it kept there but never makes a false edge.

use super::Grid;
use super::sign::median;

/// Sets each clashing texel of `field`, in the field's values from 0 to 1, to its median: first those clashing with a
/// neighbour beside, above, or below, then those clashing with a neighbour on a diagonal. A clash is a difference of
/// at least `threshold` in the channel that differs second most.
pub fn correct(field: &mut [[f64; 3]], grid: Grid, threshold: f64) {
    let beside = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    let diagonal = [(-1, -1), (1, -1), (-1, 1), (1, 1)];
    equalize(field, grid, &beside, threshold);
    equalize(field, grid, &diagonal, threshold * 2.0);
}

/// Sets each texel clashing with a neighbour at one of `offsets` to its median.
fn equalize(field: &mut [[f64; 3]], grid: Grid, offsets: &[(isize, isize)], threshold: f64) {
    let mut clashing = Vec::new();
    for row in 0..grid.height {
        for column in 0..grid.width {
            let texel = field[row * grid.width + column];
            let clashes = offsets.iter().any(|&(across, down)| {
                let neighbour = grid.neighbour(column, row, across, down);
                neighbour.is_some_and(|neighbour| clash(texel, field[neighbour], threshold))
            });
            if clashes {
                clashing.push(row * grid.width + column);
            }
        }
    }
    for index in clashing {
        field[index] = [median(field[index]); 3];
    }
}

/// Whether texel `a` clashes with its neighbour `b`: their channels' differences, ordered from greatest, have the
/// second at least `threshold`, `b` has not been set to its median already, and `a` is the further of the two from
/// the edge in the channel that differs least, so that only one of the pair is changed.
fn clash(a: [f64; 3], b: [f64; 3], threshold: f64) -> bool {
    let mut channels = [0, 1, 2];
    channels.sort_by(|&first, &second| (b[second] - a[second]).abs().total_cmp(&(b[first] - a[first]).abs()));
    let [_, second, least] = channels;

    let differs = (b[second] - a[second]).abs() >= threshold;
    let equalized = b[0] == b[1] && b[0] == b[2];
    let further = (a[least] - 0.5).abs() >= (b[least] - 0.5).abs();
    differs && !equalized && further
}
