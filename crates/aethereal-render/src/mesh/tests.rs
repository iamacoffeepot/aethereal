//! A mesh's check: what WebGPU would refuse, or draw wrong, is refused as it is made.

use super::*;

/// A layout of a position, three floats, and a colour, four bytes, after it.
fn layout() -> VertexLayout {
    let position = Attribute { location: 0, format: VertexFormat::Float32x3, offset: 0 };
    let colour = Attribute { location: 1, format: VertexFormat::Unorm8x4, offset: 12 };

    VertexLayout { stride: 16, attributes: vec![position, colour] }
}

/// The bytes of `indices`, as a mesh is sent them.
fn indices(indices: &[u32]) -> Vec<u8> {
    indices.iter().flat_map(|index| index.to_ne_bytes()).collect()
}

#[test]
fn a_mesh_is_counted_in_whole_vertices_and_indices() {
    // Catches a count of bytes where a count of vertices or indices is meant: a draw would read past its buffers.
    let counted = check(&layout(), &[0; 64], &indices(&[0, 1, 2, 2, 1, 3]));

    assert_eq!(counted, Ok(Counted { vertices: 4, indices: 6 }));
}

#[test]
fn an_index_past_the_last_vertex_is_refused() {
    // Catches a check that lets an index equal to the vertex count through: the GPU would read a vertex that is not
    // there.
    let past = check(&layout(), &[0; 64], &indices(&[0, 1, 4]));

    assert_eq!(past, Err(MeshRefused::Index { index: 4, vertices: 4 }));
}

#[test]
fn vertices_or_indices_that_are_not_whole_are_refused() {
    // Catches a check that divides without asking what is left over, or lets an empty mesh through.
    let part_vertex = check(&layout(), &[0; 60], &indices(&[0, 1, 2]));
    let no_vertices = check(&layout(), &[], &indices(&[0, 1, 2]));
    let part_triangle = check(&layout(), &[0; 64], &indices(&[0, 1]));
    let part_index = check(&layout(), &[0; 64], &[0; 14]);

    assert_eq!(part_vertex, Err(MeshRefused::Vertices { bytes: 60 }));
    assert_eq!(no_vertices, Err(MeshRefused::Vertices { bytes: 0 }));
    assert_eq!(part_triangle, Err(MeshRefused::Indices { bytes: 8 }));
    assert_eq!(part_index, Err(MeshRefused::Indices { bytes: 14 }));
}

#[test]
fn a_layout_webgpu_would_refuse_is_refused() {
    // Catches a layout let through that WebGPU refuses when the pipeline is made, or that reads past each vertex: a
    // stride off a word, an attribute past the vertex or off a word, and two attributes at one location.
    let mut off_word = layout();
    off_word.stride = 18;
    let mut past_end = layout();
    past_end.attributes[1].offset = 14;
    let mut unaligned = layout();
    unaligned.stride = 20;
    unaligned.attributes[1].offset = 13;
    let mut shared = layout();
    shared.attributes[1].location = 0;

    let refused = [off_word, past_end, unaligned, shared].map(|layout| check(&layout, &[0; 80], &indices(&[0, 1, 2])));

    assert_eq!(
        refused,
        [
            Err(MeshRefused::Stride { stride: 18 }),
            Err(MeshRefused::Attribute { location: 1 }),
            Err(MeshRefused::Attribute { location: 1 }),
            Err(MeshRefused::Location { location: 0 }),
        ]
    );
}
