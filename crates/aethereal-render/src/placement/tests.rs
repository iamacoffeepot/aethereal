//! A placement's check: what WebGPU would refuse as a frame draws it is refused as it is placed.

use super::*;
use crate::mesh::{Attribute, VertexFormat};

/// What a test keeps: one program, reading a position per vertex and an offset per instance, binding a camera's
/// uniforms, a texture, and a sampler; a mesh and instances laid out as it reads; uniforms of 64 bytes; a texture.
struct Kept {
    layout: VertexLayout,
    mesh: VertexLayout,
    per_instance: PerInstance,
    records: VertexLayout,
    bindings: Vec<Binding>,
}

const PROGRAM: ProgramId = ProgramId { id: 1 };
const MESH: MeshId = MeshId { id: 2 };
const INSTANCES: InstancesId = InstancesId { id: 3 };
const UNIFORMS: UniformsId = UniformsId { id: 4 };
const TEXTURE: TextureId = TextureId { id: 5 };
const SAMPLING: Sampling = Sampling { filter: Filter::Linear, wrap: Wrap::Clamp };

impl Kept {
    fn new() -> Self {
        let position = Attribute { location: 0, format: VertexFormat::Float32x3, offset: 0 };
        let offset = Attribute { location: 1, format: VertexFormat::Float32x3, offset: 0 };
        let records = VertexLayout { stride: 12, attributes: vec![offset] };
        let bindings = vec![
            Binding { group: 0, binding: 0, kind: BindingKind::Uniform { bytes: 64 } },
            Binding { group: 1, binding: 0, kind: BindingKind::Texture },
            Binding { group: 1, binding: 1, kind: BindingKind::Sampler },
        ];

        let layout = VertexLayout { stride: 12, attributes: vec![position] };

        Self { mesh: layout.clone(), layout, per_instance: PerInstance::Records(records.clone()), records, bindings }
    }
}

impl Finds for Kept {
    fn program(&self, id: ProgramId) -> Option<Shape<'_>> {
        let shape = Shape { layout: &self.layout, instances: &self.per_instance, bindings: &self.bindings };
        (id == PROGRAM).then_some(shape)
    }

    fn mesh(&self, id: MeshId) -> Option<&VertexLayout> {
        (id == MESH).then_some(&self.mesh)
    }

    fn instances(&self, id: InstancesId) -> Option<&VertexLayout> {
        (id == INSTANCES).then_some(&self.records)
    }

    fn uniforms(&self, id: UniformsId) -> Option<u32> {
        (id == UNIFORMS).then_some(64)
    }

    fn has_texture(&self, id: TextureId) -> bool {
        id == TEXTURE
    }
}

/// A placement of the kept mesh with the kept program, for each kept instance, binding all it binds.
fn place() -> Place {
    let bindings = vec![
        Bind { group: 0, binding: 0, to: Resource::Uniforms(UNIFORMS) },
        Bind { group: 1, binding: 0, to: Resource::Texture(TEXTURE) },
        Bind { group: 1, binding: 1, to: Resource::Sampler(SAMPLING) },
    ];

    Place { program: PROGRAM, mesh: MESH, instances: Drawn::Records(INSTANCES), bindings }
}

#[test]
fn a_placement_of_what_is_kept_laid_out_and_bound_as_its_program_reads_is_made() {
    // Catches a check that refuses what a frame can draw: every program drawn per instance would be unplaceable.
    assert_eq!(check(&place(), &Kept::new()), Ok(()));
}

#[test]
fn a_placement_naming_what_is_not_kept_is_refused() {
    // Catches an id looked up in the wrong sort, or not at all: the frame would draw a mesh or a program that is gone.
    let mut program = place();
    program.program = ProgramId { id: 9 };
    let mut mesh = place();
    mesh.mesh = MeshId { id: 9 };
    let mut instances = place();
    instances.instances = Drawn::Records(InstancesId { id: 9 });
    let mut uniforms = place();
    uniforms.bindings[0].to = Resource::Uniforms(UniformsId { id: 9 });
    let mut texture = place();
    texture.bindings[1].to = Resource::Texture(TextureId { id: 9 });
    let kept = Kept::new();

    assert_eq!(check(&program, &kept), Err(PlacementRefused::Program));
    assert_eq!(check(&mesh, &kept), Err(PlacementRefused::Mesh));
    assert_eq!(check(&instances, &kept), Err(PlacementRefused::Instances));
    assert_eq!(check(&uniforms, &kept), Err(PlacementRefused::Missing { group: 0, binding: 0 }));
    assert_eq!(check(&texture, &kept), Err(PlacementRefused::Missing { group: 1, binding: 0 }));
}

#[test]
fn a_mesh_or_instances_laid_out_otherwise_than_the_program_reads_is_refused() {
    // Catches layouts left unmatched, which WebGPU refuses as the draw is recorded, or a count drawn with a program
    // that reads records, which would read a buffer never bound.
    let mut other_records = Kept::new();
    other_records.records.stride = 16;
    let mut counted = place();
    counted.instances = Drawn::Count(4);
    let mut no_records = Kept::new();
    no_records.per_instance = PerInstance::None;
    let mut moved = Kept::new();
    moved.mesh.attributes[0].offset = 4;
    moved.mesh.stride = 16;

    assert_eq!(check(&place(), &other_records), Err(PlacementRefused::Drawn));
    assert_eq!(check(&counted, &Kept::new()), Err(PlacementRefused::Drawn));
    assert_eq!(check(&place(), &no_records), Err(PlacementRefused::Drawn));
    assert_eq!(check(&counted, &no_records), Ok(()));
    assert_eq!(check(&place(), &moved), Err(PlacementRefused::Layout));
}

#[test]
fn bindings_that_miss_add_to_repeat_or_mismatch_what_the_program_binds_are_refused() {
    // Catches a bind group made short of a place, or of a resource of another kind, or with uniforms smaller than
    // the block: each is refused by WebGPU as the bind group is made or the draw is recorded.
    let mut unbound = place();
    unbound.bindings.pop();
    let mut unlisted = place();
    unlisted.bindings.push(Bind { group: 2, binding: 0, to: Resource::Sampler(SAMPLING) });
    let mut repeated = place();
    repeated.bindings.push(Bind { group: 1, binding: 1, to: Resource::Sampler(SAMPLING) });
    let mut mismatched = place();
    mismatched.bindings[1].to = Resource::Sampler(SAMPLING);
    let mut small = Kept::new();
    small.bindings[0].kind = BindingKind::Uniform { bytes: 80 };
    let kept = Kept::new();

    assert_eq!(check(&unbound, &kept), Err(PlacementRefused::Unbound { group: 1, binding: 1 }));
    assert_eq!(check(&unlisted, &kept), Err(PlacementRefused::Unlisted { group: 2, binding: 0 }));
    assert_eq!(check(&repeated, &kept), Err(PlacementRefused::Repeated { group: 1, binding: 1 }));
    assert_eq!(check(&mismatched, &kept), Err(PlacementRefused::Mismatch { group: 1, binding: 0 }));
    assert_eq!(check(&place(), &small), Err(PlacementRefused::Mismatch { group: 0, binding: 0 }));
}
