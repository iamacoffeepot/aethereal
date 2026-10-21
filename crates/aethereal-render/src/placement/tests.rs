//! A placement's check: what WebGPU would refuse as a frame draws it is refused as it is placed.

use super::*;
use crate::dispatch::Use;
use crate::frame::FrameId;
use crate::mesh::{Attribute, VertexFormat};
use crate::pass::Main;
use crate::program::Access;
use crate::shape::ColourFormat;
use crate::storage::StorageId;
use aethereal_data::{Budget, from_bytes};
use aethereal_window::WindowId;

/// What a test keeps: one program, reading a position per vertex and an offset per instance, binding a camera's
/// uniforms, a texture, a sampler, and storage of splats, made against `shape`; a mesh and instances laid out as it
/// reads; uniforms of 64 bytes; storage of 256, and arguments of 256; a texture; and the main frame of the window [`window`], whose one
/// pass, [`Main`], is of `pass`.
struct Kept {
    layout: VertexLayout,
    mesh: VertexLayout,
    per_instance: PerInstance,
    records: VertexLayout,
    bindings: Vec<Binding>,
    shape: Shape,
    pass: Shape,
    reads: Vec<(u64, u64)>,
    uses: Vec<(StorageId, Use)>,
}

/// The window opened `nth`, as a host's window capability numbers it.
fn window(nth: u64) -> WindowId {
    let Ok(window) = from_bytes(&nth.to_le_bytes(), &mut Budget::new(8)) else {
        panic!("a window's id is its number");
    };

    window
}

/// A pass the kept frame does not have.
#[derive(Plain)]
struct Elsewhere;

impl Kind for Elsewhere {
    const NAME: KindName = KindName::new("aether.test.elsewhere");
}

impl Pass for Elsewhere {}

const PROGRAM: ProgramId = ProgramId { id: 1 };
const MESH: MeshId = MeshId { id: 2 };
const INSTANCES: InstancesId = InstancesId { id: 3 };
const UNIFORMS: UniformsId = UniformsId { id: 4 };
const TEXTURE: TextureId = TextureId { id: 5 };
const STORAGE: StorageId = StorageId { id: 6 };
/// Storage of 256 bytes a placement reads its counts from, and binds nowhere.
const ARGUMENTS: StorageId = StorageId { id: 12 };
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
            Binding { group: 2, binding: 0, kind: BindingKind::Storage { bytes: 48, access: Access::Read } },
        ];

        let layout = VertexLayout { stride: 12, attributes: vec![position] };
        let shape = Shape::with_depth(ColourFormat::Bgra8);

        Self {
            mesh: layout.clone(),
            layout,
            per_instance: PerInstance::Records(records.clone()),
            records,
            bindings,
            shape,
            pass: shape,
            reads: Vec::new(),
            uses: Vec::new(),
        }
    }
}

impl Finds for Kept {
    fn pass(&self, frame: Frame, pass: PassName) -> Result<Shape, JoinRefused> {
        let kept = [Frame::Main(window(0)), Frame::Texture(MAP), Frame::Texture(INSET)];
        if !kept.contains(&frame) {
            return Err(JoinRefused::Frame);
        }
        if pass != PassName::of::<Main>() {
            return Err(JoinRefused::Pass);
        }

        Ok(self.pass)
    }

    fn program(&self, id: ProgramId) -> Option<Reads<'_>> {
        let reads =
            Reads { layout: &self.layout, instances: &self.per_instance, bindings: &self.bindings, shape: self.shape };
        (id == PROGRAM).then_some(reads)
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

    fn storage(&self, id: StorageId) -> Option<u32> {
        [STORAGE, ARGUMENTS].contains(&id).then_some(256)
    }

    fn has_texture(&self, id: TextureId) -> bool {
        [TEXTURE, MAPPED, INSET_TEXTURE].contains(&id)
    }

    fn drawn_by(&self, texture: TextureId) -> Option<u64> {
        match texture {
            MAPPED => Some(MAP.id),
            INSET_TEXTURE => Some(INSET.id),
            _ => None,
        }
    }

    fn reads(&self) -> Vec<(u64, u64)> {
        self.reads.clone()
    }

    fn uses(&self, storage: StorageId) -> Vec<Use> {
        self.uses.iter().filter(|(used, _)| *used == storage).map(|(_, used)| *used).collect()
    }
}

/// A frame that ends on a texture, [`MAPPED`], with one pass, [`Main`].
const MAP: FrameId = FrameId { id: 7 };
/// The texture [`MAP`] ends on.
const MAPPED: TextureId = TextureId { id: 8 };
/// Another frame that ends on a texture, [`INSET_TEXTURE`], with one pass, [`Main`].
const INSET: FrameId = FrameId { id: 10 };
/// The texture [`INSET`] ends on.
const INSET_TEXTURE: TextureId = TextureId { id: 11 };

/// A placement of the kept mesh, for each kept instance, joining the kept frame's one pass with the kept program,
/// binding all it binds.
fn place() -> Place {
    let bindings = vec![
        Bind { group: 0, binding: 0, to: Resource::Uniforms(UNIFORMS) },
        Bind { group: 1, binding: 0, to: Resource::Texture(TEXTURE) },
        Bind { group: 1, binding: 1, to: Resource::Sampler(SAMPLING) },
        Bind { group: 2, binding: 0, to: Resource::Storage(STORAGE) },
    ];
    let join = Join::to::<Main>(Frame::Main(window(0)), PROGRAM, bindings);

    Place { mesh: MESH, instances: Drawn::Records(INSTANCES), joins: vec![join] }
}

/// What the check answers of `place`'s first join, refused for `why`.
const fn first(why: JoinRefused) -> Result<(), PlacementRefused> {
    Err(PlacementRefused::Join { join: 0, why })
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
    program.joins[0].program = ProgramId { id: 9 };
    let mut mesh = place();
    mesh.mesh = MeshId { id: 9 };
    let mut instances = place();
    instances.instances = Drawn::Records(InstancesId { id: 9 });
    let mut uniforms = place();
    uniforms.joins[0].bindings[0].to = Resource::Uniforms(UniformsId { id: 9 });
    let mut texture = place();
    texture.joins[0].bindings[1].to = Resource::Texture(TextureId { id: 9 });
    let mut storage = place();
    storage.joins[0].bindings[3].to = Resource::Storage(StorageId { id: 9 });
    let kept = Kept::new();

    assert_eq!(check(&program, &kept), first(JoinRefused::Program));
    assert_eq!(check(&mesh, &kept), Err(PlacementRefused::Mesh));
    assert_eq!(check(&instances, &kept), Err(PlacementRefused::Instances));
    assert_eq!(check(&uniforms, &kept), first(JoinRefused::Missing { group: 0, binding: 0 }));
    assert_eq!(check(&texture, &kept), first(JoinRefused::Missing { group: 1, binding: 0 }));
    assert_eq!(check(&storage, &kept), first(JoinRefused::Missing { group: 2, binding: 0 }));
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

    assert_eq!(check(&place(), &other_records), first(JoinRefused::Drawn));
    assert_eq!(check(&counted, &Kept::new()), first(JoinRefused::Drawn));
    assert_eq!(check(&place(), &no_records), first(JoinRefused::Drawn));
    assert_eq!(check(&counted, &no_records), Ok(()));
    assert_eq!(check(&place(), &moved), first(JoinRefused::Layout));
}

#[test]
fn bindings_that_miss_add_to_repeat_or_mismatch_what_the_program_binds_are_refused() {
    // Catches a bind group made short of a place, or of a resource of another kind, or with uniforms or storage
    // smaller than the program binds: each is refused by WebGPU as the bind group is made or the draw is recorded.
    let mut unbound = place();
    unbound.joins[0].bindings.remove(2);
    let mut unlisted = place();
    unlisted.joins[0].bindings.push(Bind { group: 3, binding: 0, to: Resource::Sampler(SAMPLING) });
    let mut repeated = place();
    repeated.joins[0].bindings.push(Bind { group: 1, binding: 1, to: Resource::Sampler(SAMPLING) });
    let mut mismatched = place();
    mismatched.joins[0].bindings[1].to = Resource::Sampler(SAMPLING);
    let mut small = Kept::new();
    small.bindings[0].kind = BindingKind::Uniform { bytes: 80 };
    let mut few = Kept::new();
    few.bindings[3].kind = BindingKind::Storage { bytes: 260, access: Access::Read };
    let mut fits = Kept::new();
    fits.bindings[3].kind = BindingKind::Storage { bytes: 256, access: Access::Read };
    let mut as_uniforms = place();
    as_uniforms.joins[0].bindings[3].to = Resource::Uniforms(UNIFORMS);
    let mut as_storage = place();
    as_storage.joins[0].bindings[0].to = Resource::Storage(STORAGE);
    let kept = Kept::new();

    assert_eq!(check(&unbound, &kept), first(JoinRefused::Unbound { group: 1, binding: 1 }));
    assert_eq!(check(&unlisted, &kept), first(JoinRefused::Unlisted { group: 3, binding: 0 }));
    assert_eq!(check(&repeated, &kept), first(JoinRefused::Repeated { group: 1, binding: 1 }));
    assert_eq!(check(&mismatched, &kept), first(JoinRefused::Mismatch { group: 1, binding: 0 }));
    assert_eq!(check(&place(), &small), first(JoinRefused::Mismatch { group: 0, binding: 0 }));
    assert_eq!(check(&place(), &few), first(JoinRefused::Mismatch { group: 2, binding: 0 }));
    assert_eq!(check(&place(), &fits), Ok(()));
    assert_eq!(check(&as_uniforms, &kept), first(JoinRefused::Mismatch { group: 2, binding: 0 }));
    assert_eq!(check(&as_storage, &kept), first(JoinRefused::Mismatch { group: 0, binding: 0 }));
}

#[test]
fn a_join_of_a_pass_the_frame_has_not_or_of_another_shape_is_refused() {
    // Catches a join checked against no frame, against the pass's name alone, or against a program of another shape
    // than its pass: WebGPU refuses a pipeline whose targets are not the pass's as the draw is recorded.
    let mut elsewhere = place();
    elsewhere.joins[0].pass = PassName::of::<Elsewhere>();
    let mut other_window = place();
    other_window.joins[0].frame = Frame::Main(window(1));
    let mut srgb = Kept::new();
    srgb.shape.colour = ColourFormat::Bgra8Srgb;
    let kept = Kept::new();

    assert_eq!(check(&elsewhere, &kept), first(JoinRefused::Pass));
    assert_eq!(check(&other_window, &kept), first(JoinRefused::Frame));
    assert_eq!(check(&place(), &srgb), first(JoinRefused::Shape));
}

#[test]
fn a_placement_joining_nothing_or_one_pass_twice_is_refused_naming_the_join() {
    // Catches a placement that would draw nowhere let through, a pass joined twice drawn twice where it is drawn once
    // in each pass it joins, and a refusal that names the wrong join.
    let mut unjoined = place();
    unjoined.joins.clear();
    let mut twice = place();
    twice.joins.push(twice.joins[0].clone());
    let mut second = place();
    let mut elsewhere = second.joins[0].clone();
    elsewhere.pass = PassName::of::<Elsewhere>();
    second.joins.push(elsewhere);
    let mut other_frame = place();
    let mut other = other_frame.joins[0].clone();
    other.frame = Frame::Main(window(1));
    other_frame.joins.push(other);
    let kept = Kept::new();

    assert_eq!(check(&unjoined, &kept), Err(PlacementRefused::Unjoined));
    assert_eq!(check(&twice, &kept), Err(PlacementRefused::Join { join: 1, why: JoinRefused::Twice }));
    assert_eq!(check(&second, &kept), Err(PlacementRefused::Join { join: 1, why: JoinRefused::Pass }));
    assert_eq!(check(&other_frame, &kept), Err(PlacementRefused::Join { join: 1, why: JoinRefused::Frame }));
}

/// [`place`] joining `frame` and binding `texture` where it binds a texture.
fn binding(frame: Frame, texture: TextureId) -> Place {
    let mut place = place();
    place.joins[0].frame = frame;
    place.joins[0].bindings[1].to = Resource::Texture(texture);

    place
}

#[test]
fn a_join_that_would_have_frames_read_each_other_round_is_refused() {
    // Catches frames that read each other let through, through frames between, through two joins of one placement, or
    // a frame reading its own texture: each would read what the other drew a `Render` before, or what it draws.
    let mut inset_reads_map = Kept::new();
    inset_reads_map.reads = vec![(INSET.id, MAP.id)];
    let both = {
        let mut both = binding(Frame::Texture(MAP), INSET_TEXTURE);
        let mut back = both.joins[0].clone();
        back.frame = Frame::Texture(INSET);
        back.bindings[1].to = Resource::Texture(MAPPED);
        both.joins.push(back);
        both
    };
    let kept = Kept::new();

    assert_eq!(check(&binding(Frame::Texture(MAP), MAPPED), &kept), first(JoinRefused::Round));
    assert_eq!(check(&binding(Frame::Texture(MAP), INSET_TEXTURE), &inset_reads_map), first(JoinRefused::Round));
    assert_eq!(check(&both, &kept), Err(PlacementRefused::Join { join: 1, why: JoinRefused::Round }));
    assert_eq!(check(&binding(Frame::Texture(INSET), MAPPED), &inset_reads_map), Ok(()));
    assert_eq!(check(&binding(Frame::Main(window(0)), MAPPED), &inset_reads_map), Ok(()));
}

/// A dispatch's use of `storage` in `frame`, writing it.
fn written_in(storage: StorageId, frame: Frame) -> (StorageId, Use) {
    (storage, Use { frame, pass: PassName::of::<Elsewhere>(), access: Access::Write })
}

#[test]
fn a_join_reading_storage_a_frame_that_reads_it_writes_is_refused() {
    // Catches storage left out of the order of frames: a frame reading storage that a frame reading it writes would
    // read what was written a `Render` before, or what is written after it in the same one.
    let mut inset_reads_map = Kept::new();
    inset_reads_map.reads = vec![(INSET.id, MAP.id)];
    inset_reads_map.uses = vec![written_in(STORAGE, Frame::Texture(INSET))];
    let mut written_by_main = Kept::new();
    written_by_main.uses = vec![written_in(STORAGE, Frame::Main(window(0)))];
    let mut written_by_itself = Kept::new();
    written_by_itself.uses = vec![written_in(STORAGE, Frame::Texture(MAP))];

    assert_eq!(check(&binding(Frame::Texture(MAP), TEXTURE), &inset_reads_map), first(JoinRefused::Round));
    assert_eq!(check(&binding(Frame::Main(window(0)), TEXTURE), &inset_reads_map), Ok(()));
    assert_eq!(check(&binding(Frame::Texture(MAP), TEXTURE), &written_by_main), Ok(()));
    assert_eq!(check(&binding(Frame::Texture(MAP), TEXTURE), &written_by_itself), Ok(()));
}

/// [`place`] joining `frame`, drawn as the counts of [`ARGUMENTS`] from the byte `at` hold.
fn indirect(frame: Frame, at: u32) -> Place {
    let mut place = place();
    place.instances = Drawn::Indirect { storage: ARGUMENTS, at };
    place.joins[0].frame = frame;

    place
}

/// What a test keeps, of a program that reads nothing per instance.
fn reading_none() -> Kept {
    let mut kept = Kept::new();
    kept.per_instance = PerInstance::None;

    kept
}

#[test]
fn a_placement_whose_counts_storage_holds_is_drawn_with_a_program_that_reads_nothing_per_instance() {
    // Catches counts held by storage drawn with a program that reads records, which would read a buffer never bound,
    // or refused with one that reads none, as a count is.
    let main = indirect(Frame::Main(window(0)), 0);

    assert_eq!(check(&main, &reading_none()), Ok(()));
    assert_eq!(check(&main, &Kept::new()), first(JoinRefused::Drawn));
}

#[test]
fn counts_read_from_storage_not_kept_or_past_its_bytes_or_off_a_word_are_refused() {
    // Catches the five words checked short of their end, by their first word alone, or with the sum overflowing, any of
    // which WebGPU refuses as the draw is recorded, or the last place they fit refused. Of 256 bytes, the five words
    // fit from 236 and not from 240.
    let main = |at| indirect(Frame::Main(window(0)), at);
    let mut missing = main(0);
    missing.instances = Drawn::Indirect { storage: StorageId { id: 9 }, at: 0 };
    let kept = reading_none();

    assert_eq!(check(&main(236), &kept), Ok(()));
    assert_eq!(check(&main(240), &kept), Err(PlacementRefused::Arguments { at: 240, bytes: 256 }));
    assert_eq!(check(&main(2), &kept), Err(PlacementRefused::Arguments { at: 2, bytes: 256 }));
    assert_eq!(check(&main(u32::MAX - 3), &kept), Err(PlacementRefused::Arguments { at: u32::MAX - 3, bytes: 256 }));
    assert_eq!(check(&missing, &kept), Err(PlacementRefused::Storage));
}

#[test]
fn counts_read_from_storage_a_frame_that_reads_the_placement_s_frame_writes_are_refused() {
    // Catches counts left out of the order of frames: a frame drawing as many as storage written by a frame that reads
    // it holds would draw what was counted a `Render` before.
    let mut inset_reads_map = reading_none();
    inset_reads_map.reads = vec![(INSET.id, MAP.id)];
    inset_reads_map.uses = vec![written_in(ARGUMENTS, Frame::Texture(INSET))];
    let mut written_by_main = reading_none();
    written_by_main.uses = vec![written_in(ARGUMENTS, Frame::Main(window(0)))];

    assert_eq!(check(&indirect(Frame::Texture(MAP), 0), &inset_reads_map), first(JoinRefused::Round));
    assert_eq!(check(&indirect(Frame::Main(window(0)), 0), &inset_reads_map), Ok(()));
    assert_eq!(check(&indirect(Frame::Texture(MAP), 0), &written_by_main), Ok(()));
}
