// What a bilinear sample of a grid of floats needs: the indices of the four cells around the point, in a buffer that
// holds the grid row by row, and the two weights to blend them by.
//
// The grid is a storage buffer, which WebGPU cannot filter as it filters a texture. A module of a package cannot name a
// buffer the program declares, and a function cannot take one by pointer without WGSL's
// `unrestricted_pointer_parameters`, which WebGPU does not enable by default. So the program reads the four cells
// itself and blends them, as `bilinear` says.
public struct Bilinear {
    // The near row's two cells, then the far row's: (near, near + 1 across, far, far + 1 across).
    cells: vec4<i32>,
    // The weight across, then the weight down.
    weights: vec2<f32>,
}

// The four cells and the weights to sample a grid `size` cells across and down at `uv`, the point as a fraction of
// the grid's extent each way, clamped to [0, 1]. Each cell is a sample at its middle; at the edges the outermost
// cells repeat. The program reads and blends them so:
//
//     let b = bilinear(uv, size);
//     let near = mix(grid[b.cells.x], grid[b.cells.y], b.weights.x);
//     let far = mix(grid[b.cells.z], grid[b.cells.w], b.weights.x);
//     let sampled = mix(near, far, b.weights.y);
public fn bilinear(uv: vec2<f32>, size: vec2<i32>) -> Bilinear {
    let t = clamp(uv, vec2(0.0), vec2(1.0)) * vec2<f32>(size) - 0.5;
    let f = floor(t);
    let w = t - f;
    let a = clamp(vec2<i32>(f), vec2(0), size - 1);
    let b = clamp(vec2<i32>(f) + 1, vec2(0), size - 1);
    let cells = vec4(a.y * size.x + a.x, a.y * size.x + b.x, b.y * size.x + a.x, b.y * size.x + b.x);
    return Bilinear(cells, w);
}
