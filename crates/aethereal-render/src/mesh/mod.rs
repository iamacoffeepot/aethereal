//! Meshes: vertices and the triangles that index them, asked for by any actor and checked as they are made
//! (ADR-0021).

#[cfg(test)]
mod tests;

use aethereal_actor::BlobRef;
use aethereal_data::{Bound, Plain};

/// Asks the renderer for a mesh: `vertices`, laid out as `layout` says, and the triangles of `indices`, each three
/// `u32`s in the platform's byte order that index the vertices. Answered with [`Meshed`].
#[derive(Bound)]
#[kind(name = "aether.render.create_mesh")]
pub struct CreateMesh {
    pub layout: VertexLayout,
    pub vertices: BlobRef,
    pub indices: BlobRef,
}

/// How a mesh's vertices are laid out: how many bytes each takes, and each attribute a program's vertex stage reads
/// of it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.vertex_layout")]
pub struct VertexLayout {
    pub stride: u32,
    pub attributes: Vec<Attribute>,
}

impl VertexLayout {
    /// Whether an attribute of this layout is read at `location`.
    #[must_use]
    pub fn reads(&self, location: u32) -> bool {
        self.attributes.iter().any(|attribute| attribute.location == location)
    }
}

/// One attribute of a vertex: the location a vertex stage reads it at, its format, and where in the vertex it lies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.attribute")]
pub struct Attribute {
    pub location: u32,
    pub format: VertexFormat,
    pub offset: u32,
}

/// The format of a vertex attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.vertex_format")]
pub enum VertexFormat {
    Float32,
    Float32x2,
    Float32x3,
    Float32x4,
    Uint32,
    /// Four bytes, each read as a fraction from zero to one: a colour.
    Unorm8x4,
}

impl VertexFormat {
    /// How many bytes an attribute of this format takes.
    #[must_use]
    pub const fn bytes(self) -> u32 {
        match self {
            Self::Float32 | Self::Uint32 | Self::Unorm8x4 => 4,
            Self::Float32x2 => 8,
            Self::Float32x3 => 12,
            Self::Float32x4 => 16,
        }
    }

    /// The format, as wgpu names it.
    #[must_use]
    pub const fn wgpu(self) -> wgpu::VertexFormat {
        match self {
            Self::Float32 => wgpu::VertexFormat::Float32,
            Self::Float32x2 => wgpu::VertexFormat::Float32x2,
            Self::Float32x3 => wgpu::VertexFormat::Float32x3,
            Self::Float32x4 => wgpu::VertexFormat::Float32x4,
            Self::Uint32 => wgpu::VertexFormat::Uint32,
            Self::Unorm8x4 => wgpu::VertexFormat::Unorm8x4,
        }
    }
}

/// A mesh the renderer made, which any actor handed it may draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.mesh_id")]
pub struct MeshId {
    pub(crate) id: u64,
}

/// What the renderer answers [`CreateMesh`] with.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.meshed")]
pub enum Meshed {
    Made(MeshId),
    Refused(MeshRefused),
}

/// Why a mesh was not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.mesh_refused")]
pub enum MeshRefused {
    /// The layout's stride is zero, or not a whole number of four bytes.
    Stride { stride: u32 },
    /// An attribute lies past the end of the vertex, or not on four bytes, or at a location past the sixteenth.
    Attribute { location: u32 },
    /// Two attributes are read at one location.
    Location { location: u32 },
    /// The vertices are no whole number of the stride, or there are none.
    Vertices { bytes: u64 },
    /// The indices are no whole number of triangles of `u32`s, or there are none.
    Indices { bytes: u64 },
    /// An index names a vertex past the last.
    Index { index: u32, vertices: u32 },
}

/// A mesh's counts, as its check found them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counted {
    pub vertices: u32,
    pub indices: u32,
}

/// Checks a mesh before it is made: its layout, that its vertices are whole, and that every index names one of them.
///
/// # Errors
///
/// Why it would not be drawn as asked.
pub fn check(layout: &VertexLayout, vertices: &[u8], indices: &[u8]) -> Result<Counted, MeshRefused> {
    check_layout(layout)?;

    let stride = u64::from(layout.stride);
    let vertex_bytes = vertices.len() as u64;
    let whole_vertices = vertex_bytes > 0 && vertex_bytes.is_multiple_of(stride);
    let counted = u32::try_from(vertex_bytes / stride);
    let (true, Ok(vertex_count)) = (whole_vertices, counted) else {
        return Err(MeshRefused::Vertices { bytes: vertex_bytes });
    };

    let index_bytes = indices.len() as u64;
    let whole_triangles = index_bytes > 0 && index_bytes.is_multiple_of(12);
    let counted = u32::try_from(index_bytes / 4);
    let (true, Ok(index_count)) = (whole_triangles, counted) else {
        return Err(MeshRefused::Indices { bytes: index_bytes });
    };
    let past = indices
        .chunks_exact(4)
        .map(|bytes| u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        .find(|index| *index >= vertex_count);
    if let Some(index) = past {
        return Err(MeshRefused::Index { index, vertices: vertex_count });
    }

    Ok(Counted { vertices: vertex_count, indices: index_count })
}

/// The locations a vertex stage reads attributes at: as many as WebGPU lets every device read.
const LOCATIONS: u32 = 16;

/// Checks that a layout's stride is whole words, and that each attribute lies inside it, on a word, at a location of
/// its own that a vertex stage can read: what WebGPU asks of a vertex buffer's layout.
pub(crate) fn check_layout(layout: &VertexLayout) -> Result<(), MeshRefused> {
    let stride = layout.stride;
    let whole_words = stride > 0 && stride.is_multiple_of(4);
    if !whole_words {
        return Err(MeshRefused::Stride { stride });
    }
    for (at, attribute) in layout.attributes.iter().enumerate() {
        let location = attribute.location;
        let end = attribute.offset.checked_add(attribute.format.bytes());
        let inside = end.is_some_and(|end| end <= stride);
        let aligned = attribute.offset.is_multiple_of(4);
        let readable = location < LOCATIONS;
        let fits = inside && aligned && readable;
        if !fits {
            return Err(MeshRefused::Attribute { location });
        }
        let shared = layout.attributes[..at].iter().any(|before| before.location == location);
        if shared {
            return Err(MeshRefused::Location { location });
        }
    }

    Ok(())
}
