// An ordered compaction: the elements a predicate keeps, written densely in the order they had, with how many were
// kept. Built on the workgroup scan: each kept element's place is the count of kept elements before it. It places items
// in a fixed order where only some survive, such as painting order, or one filler per cell where only empty cells
// emit.
//
// WGSL passes no functions, so the predicate is the program's: it tests each element and hands `place` whether it
// keeps it. A module of a package cannot name the program's storage buffers, so the program reads each element and
// writes the kept ones where `place` says. Every invocation of the workgroup calls `place`, in control flow every
// invocation takes alike, as `scan` says: an invocation past the end keeps nothing rather than returning early. The
// entry point is `@workgroup_size(WORKGROUP_SIZE)`, imported from `scan`.
//
// Up to `WORKGROUP_SIZE` elements compact in one workgroup:
//
//     let placed = place(keep, local, 0u);
//     if keep {
//         kept[placed.at] = element;
//     }
//     if local == 0u {
//         count = placed.end;
//     }
//
// More are compacted in three dispatches, each in a compute pass of its own:
//
//     1. one invocation per element: each workgroup counts what it keeps, `place(keep, local, 0u).end`, into
//        `totals[group]`;
//     2. one workgroup: it scans `totals` in place with `scan_from`, a width at a time, as `scan` shows, so each holds
//        its workgroup's offset, the place of its first kept element; the last carry is the count;
//     3. one invocation per element again: each tests its element as in the first pass, and a kept one is written at
//        `place(keep, local, totals[group]).at`.
import package::scan::scan_from;

// Where `place` puts an element: `at`, its place among the kept elements, if it is kept; and `end`, the place after
// the workgroup's last kept element, the same for each invocation.
public struct Placed {
    at: u32,
    end: u32,
}

// Where the element `keep` says whether to keep goes, among the workgroup's kept elements placed from `first`, each
// invocation handing its own and `local`, its `@builtin(local_invocation_index)`. Kept elements are placed in the order
// of `local`, densely from `first`, and `end - first` is how many the workgroup keeps.
public fn place(keep: bool, local: u32, first: u32) -> Placed {
    let summed = scan_from(select(0u, 1u, keep), local, first);
    return Placed(summed.exclusive, summed.total);
}
