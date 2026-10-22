// Keeps each item inside the disc, in the items' order, as an ordered compaction across workgroups in three passes, each
// an entry point dispatched in a compute pass of its own: `tally` counts what each workgroup keeps, `offsets` scans the
// counts into where each workgroup's kept items begin and writes how many were kept into the draw's arguments, and
// `scatter` writes the index of each kept item at its place. Invocations past the items' end keep nothing, and every
// one calls `place`, as it waits at the workgroup's barriers.
import aethereal_render::scan::{WORKGROUP_SIZE, scan_from};
import aethereal_render::compact::place;

struct Arguments {
    index_count: u32,
    instance_count: u32,
    first_index: u32,
    base_vertex: u32,
    first_instance: u32,
}

@group(0) @binding(0) var<storage, read> items: array<vec2<f32>>;
// One word for each workgroup of `tally` and `scatter`: what it keeps, then where its kept items begin.
@group(0) @binding(1) var<storage, read_write> totals: array<u32>;
@group(0) @binding(2) var<storage, read_write> visible: array<u32>;
@group(0) @binding(3) var<storage, read_write> arguments: Arguments;
// The totals again, as `scatter` only reads them, once `offsets` has made them offsets.
@group(0) @binding(4) var<storage, read> offsets_of: array<u32>;

// Whether the item `at` is one, inside the disc of radius 0.75.
fn keeps(at: u32) -> bool {
    if at >= arrayLength(&items) {
        return false;
    }
    let item = items[at];
    return dot(item, item) < 0.5625;
}

@compute @workgroup_size(WORKGROUP_SIZE)
fn tally(
    @builtin(global_invocation_id) id: vec3<u32>,
    @builtin(local_invocation_index) local: u32,
    @builtin(workgroup_id) group: vec3<u32>,
) {
    let placed = place(keeps(id.x), local, 0u);
    if local == 0u {
        totals[group.x] = placed.end;
    }
}

@compute @workgroup_size(WORKGROUP_SIZE)
fn offsets(@builtin(local_invocation_index) local: u32) {
    let groups = arrayLength(&totals);
    var carry = 0u;
    for (var first = 0u; first < groups; first += WORKGROUP_SIZE) {
        let at = first + local;
        var total = 0u;
        if at < groups {
            total = totals[at];
        }
        let summed = scan_from(total, local, carry);
        if at < groups {
            totals[at] = summed.exclusive;
        }
        carry = summed.total;
    }
    if local == 0u {
        arguments.instance_count = carry;
    }
}

@compute @workgroup_size(WORKGROUP_SIZE)
fn scatter(
    @builtin(global_invocation_id) id: vec3<u32>,
    @builtin(local_invocation_index) local: u32,
    @builtin(workgroup_id) group: vec3<u32>,
) {
    let keep = keeps(id.x);
    let placed = place(keep, local, offsets_of[group.x]);
    if keep {
        visible[placed.at] = id.x;
    }
}
