// A prefix sum of `u32` across a workgroup, in workgroup memory, and what scanning more than one workgroup's worth is
// built from.
//
// Every invocation of the workgroup calls a scan, in control flow every invocation takes alike, as it waits at the
// workgroup's barriers: an invocation past the end of what it scans calls it with 0 rather than returning early. The
// program declares its entry point `@workgroup_size(WORKGROUP_SIZE)`, so the workgroup is the size the scan is.
//
// A module of a package cannot name the program's storage buffers, so a scan takes each invocation's value and answers
// its sums; the program reads its values and writes what it keeps. Sums wrap past `u32`'s most, as WGSL's `u32` does.
//
// More than one workgroup's worth is scanned in three dispatches, each in a compute pass of its own, as dispatches of
// one pass run in no order (ADR-0026):
//
//     1. one invocation per element, `ceil(n / WORKGROUP_SIZE)` workgroups: each scans its own elements, writes each
//        element's sum within the workgroup, and one invocation writes the workgroup's `total` to `totals[group]`;
//     2. one workgroup: it walks `totals` a workgroup's width at a time, scanning each width with `scan_from` and the
//        carry the last one answered, writes each total's `exclusive` back in its place, the workgroup's offset, and
//        ends with the sum of everything;
//     3. one invocation per element again: each adds its workgroup's offset, `totals[group]`, to its sum.
//
// The second pass walks any number of totals in one workgroup, so three passes scan any length a buffer holds.

// The invocations in the workgroup a scan runs across, along `x`: WebGPU's least most for a workgroup, so every device
// runs it.
public const WORKGROUP_SIZE: u32 = 256u;

// What a scan answers each invocation: the sum of the values before its own, the sum through its own, and the sum of
// every value across the workgroup, the same for each.
public struct Scan {
    exclusive: u32,
    inclusive: u32,
    total: u32,
}

// The workgroup's running sums, one an invocation.
var<workgroup> sums: array<u32, WORKGROUP_SIZE>;

// The sums of `value` across the workgroup, each invocation handing its own and `local`, its
// `@builtin(local_invocation_index)`. Each step adds the sum `offset` places back, doubling `offset`, so after
// `log2(WORKGROUP_SIZE)` steps each place holds the sum through it. It ends at a barrier, so the next scan may begin
// at once.
public fn scan(value: u32, local: u32) -> Scan {
    sums[local] = value;
    workgroupBarrier();
    for (var offset = 1u; offset < WORKGROUP_SIZE; offset = offset << 1u) {
        var before = 0u;
        if local >= offset {
            before = sums[local - offset];
        }
        workgroupBarrier();
        sums[local] += before;
        workgroupBarrier();
    }
    let inclusive = sums[local];
    let total = sums[WORKGROUP_SIZE - 1u];
    workgroupBarrier();
    return Scan(inclusive - value, inclusive, total);
}

// The sums of `value` across the workgroup counted from `carry`: each sum and the total with `carry` added. The total
// it answers is the carry for the next width, so one workgroup scans a buffer longer than itself a width at a time:
//
//     var carry = 0u;
//     for (var first = 0u; first < count; first += WORKGROUP_SIZE) {
//         let at = first + local;
//         var value = 0u;
//         if at < count {
//             value = values[at];
//         }
//         let summed = scan_from(value, local, carry);
//         if at < count {
//             values[at] = summed.exclusive;
//         }
//         carry = summed.total;
//     }
//
// `count` is the same for every invocation, from a uniform or an `arrayLength`, so the loop is taken alike.
public fn scan_from(value: u32, local: u32, carry: u32) -> Scan {
    let summed = scan(value, local);
    return Scan(carry + summed.exclusive, carry + summed.inclusive, carry + summed.total);
}
