//! A dispatch's check: what WebGPU would refuse as a frame runs it is refused as it is made, and so is what would leave
//! storage to whichever dispatch ran last, or have frames read each other round.

use super::*;
use crate::frame::FrameId;
use crate::instances::InstancesId;
use crate::mesh::{MeshId, VertexLayout};
use crate::pass::{Compute, Main};
use crate::placement::{Fits, Reads};
use crate::program::ProgramId;
use crate::texture::TextureId;
use crate::uniforms::UniformsId;
use aethereal_data::{Budget, from_bytes};
use aethereal_window::WindowId;

/// The window opened `nth`, as a host's window capability numbers it.
fn window(nth: u64) -> WindowId {
    let Ok(window) = from_bytes(&nth.to_le_bytes(), &mut Budget::new(8)) else {
        panic!("a window's id is its number");
    };

    window
}

/// A second compute pass of the window's main frame, after [`Compute`].
#[derive(Plain)]
#[kind(name = "aether.test.settle")]
struct Settle;

impl Pass for Settle {}

/// A compute program that moves a field by a clock and a pull: a clock's uniforms of 4 bytes; a field it writes, of
/// a count padded to 16 bytes and particles of 32; and a pull it reads, all array, of 16 bytes an element; in
/// workgroups of 64.
const SIMULATE: ComputeId = ComputeId { id: 1 };
/// Uniforms of 16 bytes.
const CLOCK: UniformsId = UniformsId { id: 2 };
/// Storage of a field of 100 particles, made by [`ME`].
const FIELD: StorageId = StorageId { id: 3 };
/// Storage of 4 pulls, made by [`OTHER`].
const PULL: StorageId = StorageId { id: 4 };
/// Storage of a field of 100 particles, made by [`OTHER`].
const THEIRS: StorageId = StorageId { id: 5 };
/// A frame that ends on a texture, with one compute pass, [`Compute`].
const MAP: FrameId = FrameId { id: 6 };
/// Another frame that ends on a texture, with one compute pass, [`Compute`].
const INSET: FrameId = FrameId { id: 7 };
/// The actor that asks for the dispatch.
const ME: ActorId = ActorId::new(1);
/// Another actor.
const OTHER: ActorId = ActorId::new(2);

/// What a test keeps: [`SIMULATE`], [`CLOCK`], [`FIELD`], [`PULL`], and [`THEIRS`]; the window [`window`] `0`, whose
/// main frame computes in [`Compute`] then [`Settle`] and draws in [`Main`]; [`MAP`] and [`INSET`]; what reads what,
/// `reads`; and each use of storage, `uses`.
struct Kept {
    bindings: Vec<Binding>,
    arrays: Vec<Array>,
    most: u32,
    reads: Vec<(u64, u64)>,
    uses: Vec<(StorageId, Use)>,
}

impl Kept {
    fn new() -> Self {
        let bindings = vec![
            Binding { group: 0, binding: 0, kind: BindingKind::Uniform { bytes: 4 } },
            Binding { group: 0, binding: 1, kind: BindingKind::Storage { bytes: 48, access: Access::Write } },
            Binding { group: 0, binding: 2, kind: BindingKind::Storage { bytes: 16, access: Access::Read } },
        ];
        let arrays = vec![
            Array { group: 0, binding: 1, fixed: 16, stride: 32 },
            Array { group: 0, binding: 2, fixed: 0, stride: 16 },
        ];

        Self { bindings, arrays, most: 65_535, reads: Vec::new(), uses: Vec::new() }
    }
}

impl Finds for Kept {
    fn pass(&self, _: Frame, _: PassName) -> Result<Fits, JoinRefused> {
        Err(JoinRefused::Frame)
    }

    fn program(&self, _: ProgramId) -> Option<Reads<'_>> {
        None
    }

    fn drawn_by(&self, _: TextureId) -> Option<u64> {
        None
    }

    fn reads(&self) -> Vec<(u64, u64)> {
        self.reads.clone()
    }

    fn uses(&self, storage: StorageId) -> Vec<Use> {
        self.uses.iter().filter(|(used, _)| *used == storage).map(|(_, used)| *used).collect()
    }

    fn mesh(&self, _: MeshId) -> Option<&VertexLayout> {
        None
    }

    fn instances(&self, _: InstancesId) -> Option<&VertexLayout> {
        None
    }

    fn uniforms(&self, id: UniformsId) -> Option<u32> {
        (id == CLOCK).then_some(16)
    }

    fn storage(&self, id: StorageId) -> Option<u32> {
        match id {
            FIELD | THEIRS => Some(16 + 32 * 100),
            PULL => Some(64),
            _ => None,
        }
    }

    fn has_texture(&self, _: TextureId) -> bool {
        false
    }
}

impl Dispatches for Kept {
    fn computes(&self, frame: Frame, pass: PassName) -> Result<(), DispatchRefused> {
        let passes = match frame {
            Frame::Main(window) if window == self::window(0) => [PassName::of::<Compute>(), PassName::of::<Settle>()],
            Frame::Texture(MAP | INSET) => [PassName::of::<Compute>(); 2],
            Frame::Main(_) | Frame::Texture(_) => return Err(DispatchRefused::Frame),
        };
        if !passes.contains(&pass) {
            return Err(DispatchRefused::Pass);
        }

        Ok(())
    }

    fn compute(&self, id: ComputeId) -> Option<Computes<'_>> {
        let computes = Computes { bindings: &self.bindings, workgroup: [64, 1, 1], arrays: &self.arrays };

        (id == SIMULATE).then_some(computes)
    }

    fn maker(&self, id: StorageId) -> Option<ActorId> {
        match id {
            FIELD => Some(ME),
            PULL | THEIRS => Some(OTHER),
            _ => None,
        }
    }

    fn most_workgroups(&self) -> u32 {
        self.most
    }
}

/// A dispatch of [`SIMULATE`] in the [`Compute`] pass of `frame`, binding the clock, `field` to write, and the pull,
/// one invocation for each particle of the field.
fn dispatch(frame: Frame, field: StorageId) -> CreateDispatch {
    let bindings = vec![
        Bind { group: 0, binding: 0, to: Resource::Uniforms(CLOCK) },
        Bind { group: 0, binding: 1, to: Resource::Storage(field) },
        Bind { group: 0, binding: 2, to: Resource::Storage(PULL) },
    ];

    CreateDispatch { join: Dispatch::to::<Compute>(frame, SIMULATE, bindings), size: Invocations::PerElement(field) }
}

/// [`dispatch`] in the window's main frame, of [`FIELD`].
fn main() -> CreateDispatch {
    dispatch(Frame::Main(window(0)), FIELD)
}

/// [`main`], with `change` made to it.
fn changed(change: impl FnOnce(&mut CreateDispatch)) -> CreateDispatch {
    let mut dispatch = main();
    change(&mut dispatch);

    dispatch
}

/// A use of `storage` in the pass `P` of `frame`, as `access` says.
fn used<P: Pass>(storage: StorageId, frame: Frame, access: Access) -> (StorageId, Use) {
    (storage, Use { frame, pass: PassName::of::<P>(), access })
}

#[test]
fn a_dispatch_for_each_element_runs_the_workgroups_that_cover_its_storage_s_array() {
    // Catches elements counted from the storage's whole bytes or from its fixed part's, or workgroups rounded down:
    // the last particles would never move, or invocations past the end would run past the array. The field's 100
    // particles past its 16 fixed bytes are two workgroups of 64; the pull's 4 elements are one.
    let pulled = changed(|dispatch| dispatch.size = Invocations::PerElement(PULL));
    let grid = changed(|dispatch| dispatch.size = Invocations::Workgroups([3, 2, 1]));
    let kept = Kept::new();

    assert_eq!(check(&main(), ME, &kept), Ok(Runs::Workgroups([2, 1, 1])));
    assert_eq!(check(&pulled, ME, &kept), Ok(Runs::Workgroups([1, 1, 1])));
    assert_eq!(check(&grid, ME, &kept), Ok(Runs::Workgroups([3, 2, 1])));
}

#[test]
fn a_dispatch_naming_what_is_not_kept_or_a_pass_that_is_not_a_compute_pass_is_refused() {
    // Catches a dispatch let into a pass that draws, where it would never run, or one of a compute program or a frame
    // that is gone, or bound short of what its program binds, which WebGPU refuses as it is recorded.
    let missing = changed(|dispatch| dispatch.join.compute = ComputeId { id: 9 });
    let elsewhere = changed(|dispatch| dispatch.join.frame = Frame::Texture(FrameId { id: 9 }));
    let draws = changed(|dispatch| dispatch.join.pass = PassName::of::<Main>());
    let unbound = changed(|dispatch| {
        dispatch.join.bindings.remove(0);
    });
    let kept = Kept::new();

    assert_eq!(check(&missing, ME, &kept), Err(DispatchRefused::Compute));
    assert_eq!(check(&elsewhere, ME, &kept), Err(DispatchRefused::Frame));
    assert_eq!(check(&draws, ME, &kept), Err(DispatchRefused::Pass));
    assert_eq!(
        check(&unbound, ME, &kept),
        Err(DispatchRefused::Bindings { why: JoinRefused::Unbound { group: 0, binding: 0 } })
    );
}

#[test]
fn storage_another_made_is_bound_to_read_and_never_to_write() {
    // Catches the maker's check left out, or made of every storage bound: an actor would write what another made, or
    // be refused reading what it was handed.
    let theirs = dispatch(Frame::Main(window(0)), THEIRS);

    assert_eq!(check(&theirs, ME, &Kept::new()), Err(DispatchRefused::NotOwner { group: 0, binding: 1 }));
    assert_eq!(check(&theirs, OTHER, &Kept::new()), Ok(Runs::Workgroups([2, 1, 1])));
}

#[test]
fn storage_bound_to_write_and_bound_again_is_refused() {
    // Catches storage written at one place and read at another of one dispatch, which WebGPU refuses as it is run.
    let aliased = changed(|dispatch| dispatch.join.bindings[2].to = Resource::Storage(FIELD));

    assert_eq!(check(&aliased, ME, &Kept::new()), Err(DispatchRefused::Aliased { group: 0, binding: 1 }));
}

#[test]
fn a_dispatch_for_each_element_of_storage_unbound_or_ending_in_no_array_is_refused() {
    // Catches elements counted of storage the dispatch does not bind, or of a binding whose shader type has no array
    // whose length is its buffer's, which has no elements to count.
    let unbound = changed(|dispatch| dispatch.size = Invocations::PerElement(THEIRS));
    let pulled = changed(|dispatch| dispatch.size = Invocations::PerElement(PULL));
    let mut no_array = Kept::new();
    no_array.arrays.pop();

    assert_eq!(check(&unbound, ME, &Kept::new()), Err(DispatchRefused::Elements));
    assert_eq!(check(&pulled, ME, &no_array), Err(DispatchRefused::Elements));
}

#[test]
fn more_workgroups_than_the_gpu_dispatches_along_a_dimension_is_refused() {
    // Catches a grid checked in all rather than along each dimension, or a count of elements left unchecked, either of
    // which WebGPU refuses as it is run.
    let grid = changed(|dispatch| dispatch.size = Invocations::Workgroups([1, 65_536, 1]));
    let mut one = Kept::new();
    one.most = 1;

    assert_eq!(check(&grid, ME, &Kept::new()), Err(DispatchRefused::Size { workgroups: [1, 65_536, 1], most: 65_535 }));
    assert_eq!(check(&main(), ME, &one), Err(DispatchRefused::Size { workgroups: [2, 1, 1], most: 1 }));
}

#[test]
fn storage_written_in_a_pass_another_dispatch_of_it_uses_is_refused() {
    // Catches two writers of one pass let through, or a writer and a reader: dispatches of one pass run in no order,
    // so the storage would be whichever ran last, or read before or after it was written.
    let main_frame = Frame::Main(window(0));
    let in_pass = |uses: Vec<(StorageId, Use)>| {
        let mut kept = Kept::new();
        kept.uses = uses;
        check(&main(), ME, &kept)
    };

    assert_eq!(in_pass(vec![used::<Compute>(FIELD, main_frame, Access::Write)]), Err(DispatchRefused::Written));
    assert_eq!(in_pass(vec![used::<Compute>(FIELD, main_frame, Access::Read)]), Err(DispatchRefused::Written));
    assert_eq!(in_pass(vec![used::<Compute>(PULL, main_frame, Access::Write)]), Err(DispatchRefused::Written));
    assert_eq!(in_pass(vec![used::<Compute>(PULL, main_frame, Access::Read)]), Ok(Runs::Workgroups([2, 1, 1])));
    assert_eq!(in_pass(vec![used::<Settle>(FIELD, main_frame, Access::Write)]), Ok(Runs::Workgroups([2, 1, 1])));
}

#[test]
fn storage_written_in_two_frames_is_made_only_when_the_frames_run_in_an_order() {
    // Catches writers in frames that neither reads let through, whose storage would be whichever frame ran last, or
    // writers that frames order refused: a window's main frame runs after every frame that ends on a texture.
    let in_map = dispatch(Frame::Texture(MAP), FIELD);
    let written_in = |frame: Frame, reads: Vec<(u64, u64)>, dispatch: &CreateDispatch| {
        let mut kept = Kept::new();
        kept.uses = vec![used::<Compute>(FIELD, frame, Access::Write)];
        kept.reads = reads;
        check(dispatch, ME, &kept)
    };

    assert_eq!(written_in(Frame::Texture(INSET), Vec::new(), &in_map), Err(DispatchRefused::Written));
    assert_eq!(written_in(Frame::Texture(INSET), vec![(INSET.id, MAP.id)], &in_map), Ok(Runs::Workgroups([2, 1, 1])));
    assert_eq!(written_in(Frame::Texture(INSET), vec![(MAP.id, INSET.id)], &in_map), Ok(Runs::Workgroups([2, 1, 1])));
    assert_eq!(written_in(Frame::Texture(INSET), Vec::new(), &main()), Ok(Runs::Workgroups([2, 1, 1])));
    assert_eq!(written_in(Frame::Main(window(0)), Vec::new(), &in_map), Ok(Runs::Workgroups([2, 1, 1])));
}

#[test]
fn writers_in_two_frames_nothing_orders_any_longer_are_held() {
    // Catches writers left running once the placement that ordered their frames is released, whose storage would be
    // whichever frame ran last, and writers held that frames still order, or that write different storage.
    let writers = [(1, Frame::Texture(MAP), FIELD.id), (2, Frame::Texture(INSET), FIELD.id)];
    let main_and_map = [(1, Frame::Texture(MAP), FIELD.id), (2, Frame::Main(window(0)), FIELD.id)];
    let apart = [(1, Frame::Texture(MAP), FIELD.id), (2, Frame::Texture(INSET), PULL.id)];

    assert_eq!(unordered(&writers, &[]), vec![1, 2]);
    assert_eq!(unordered(&writers, &[(INSET.id, MAP.id)]), Vec::<u64>::new());
    assert_eq!(unordered(&main_and_map, &[]), Vec::<u64>::new());
    assert_eq!(unordered(&apart, &[]), Vec::<u64>::new());
}

#[test]
fn a_dispatch_that_would_have_frames_read_each_other_round_is_refused() {
    // Catches storage left out of the order of frames: a frame reading storage written by a frame that reads it, or
    // writing storage read by a frame it reads, would each read what the other wrote a `Render` before.
    let in_map = dispatch(Frame::Texture(MAP), FIELD);
    let kept = |uses: Vec<(StorageId, Use)>, reads: Vec<(u64, u64)>| {
        let mut kept = Kept::new();
        kept.uses = uses;
        kept.reads = reads;
        kept
    };
    let pull_from_inset = vec![used::<Compute>(PULL, Frame::Texture(INSET), Access::Write)];
    let field_to_inset = vec![used::<Main>(FIELD, Frame::Texture(INSET), Access::Read)];

    assert_eq!(
        check(&in_map, ME, &kept(pull_from_inset.clone(), vec![(INSET.id, MAP.id)])),
        Err(DispatchRefused::Round)
    );
    assert_eq!(check(&in_map, ME, &kept(pull_from_inset, Vec::new())), Ok(Runs::Workgroups([2, 1, 1])));
    assert_eq!(
        check(&in_map, ME, &kept(field_to_inset.clone(), vec![(MAP.id, INSET.id)])),
        Err(DispatchRefused::Round)
    );
    assert_eq!(check(&in_map, ME, &kept(field_to_inset, Vec::new())), Ok(Runs::Workgroups([2, 1, 1])));
}

/// [`main`], running as many workgroups as the three words of `storage` from the byte `at` hold.
fn indirect(storage: StorageId, at: u32) -> CreateDispatch {
    changed(|dispatch| dispatch.size = Invocations::Indirect { storage, at })
}

#[test]
fn workgroups_read_from_storage_not_kept_or_past_its_bytes_or_off_a_word_are_refused() {
    // Catches the three words checked short of their end, by their first word alone, or with the sum overflowing, any
    // of which WebGPU refuses as the dispatch is recorded, or the last place they fit refused, or a grid the GPU counts
    // checked against the most it dispatches as a grid given is. Of the pull's 64 bytes, the three words fit from 52
    // and not from 56; read from another's storage, as anyone handed it may read it.
    let mut one = Kept::new();
    one.most = 1;
    let kept = Kept::new();

    assert_eq!(check(&indirect(PULL, 52), ME, &kept), Ok(Runs::Indirect { storage: PULL, at: 52 }));
    assert_eq!(check(&indirect(PULL, 52), ME, &one), Ok(Runs::Indirect { storage: PULL, at: 52 }));
    assert_eq!(check(&indirect(PULL, 56), ME, &kept), Err(DispatchRefused::Arguments { at: 56, bytes: 64 }));
    assert_eq!(check(&indirect(PULL, 2), ME, &kept), Err(DispatchRefused::Arguments { at: 2, bytes: 64 }));
    assert_eq!(
        check(&indirect(PULL, u32::MAX - 3), ME, &kept),
        Err(DispatchRefused::Arguments { at: u32::MAX - 3, bytes: 64 })
    );
    assert_eq!(check(&indirect(StorageId { id: 9 }, 0), ME, &kept), Err(DispatchRefused::Storage));
}

#[test]
fn workgroups_read_from_storage_the_dispatch_writes_are_refused() {
    // Catches storage written at a place and read as the dispatch's workgroups, which WebGPU refuses as it is run.
    assert_eq!(check(&indirect(FIELD, 0), ME, &Kept::new()), Err(DispatchRefused::Aliased { group: 0, binding: 1 }));
}

#[test]
fn workgroups_read_from_storage_join_the_order_of_dispatches_and_frames() {
    // Catches workgroups read from storage left out of what is read: a dispatch of the same pass writing it would leave
    // the grid to whichever ran first, and a frame writing it that reads this one would have frames read each other
    // round.
    let with = |uses: Vec<(StorageId, Use)>, reads: Vec<(u64, u64)>| {
        let mut kept = Kept::new();
        kept.uses = uses;
        kept.reads = reads;
        kept
    };
    let main_frame = Frame::Main(window(0));
    let in_map = changed(|dispatch| {
        dispatch.join.frame = Frame::Texture(MAP);
        dispatch.size = Invocations::Indirect { storage: THEIRS, at: 0 };
    });
    let from_inset = vec![used::<Compute>(THEIRS, Frame::Texture(INSET), Access::Write)];
    let counted = Ok(Runs::Indirect { storage: THEIRS, at: 0 });

    assert_eq!(
        check(&indirect(THEIRS, 0), ME, &with(vec![used::<Compute>(THEIRS, main_frame, Access::Write)], Vec::new())),
        Err(DispatchRefused::Written)
    );
    assert_eq!(
        check(&indirect(THEIRS, 0), ME, &with(vec![used::<Settle>(THEIRS, main_frame, Access::Write)], Vec::new())),
        counted
    );
    assert_eq!(check(&in_map, ME, &with(from_inset.clone(), vec![(INSET.id, MAP.id)])), Err(DispatchRefused::Round));
    assert_eq!(check(&in_map, ME, &with(from_inset, Vec::new())), counted);
}
