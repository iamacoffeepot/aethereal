//! Instances' checks: as they are made, and as they are written.

use super::*;
use crate::mesh::{Attribute, VertexFormat};

/// A record of one position, three floats.
fn layout() -> VertexLayout {
    let offset = Attribute { location: 2, format: VertexFormat::Float32x3, offset: 0 };

    VertexLayout { stride: 12, attributes: vec![offset] }
}

#[test]
fn instances_hold_whole_records_within_their_room() {
    // Catches records counted in bytes, records past the room let through, which the upload would write past the
    // buffer, or room past what the GPU puts in a buffer.
    let held = check(&layout(), 4, &[0; 36], 1 << 20);
    let empty = check(&layout(), 4, &[], 1 << 20);
    let full = check(&layout(), 4, &[0; 48], 1 << 20);
    let past = check(&layout(), 4, &[0; 60], 1 << 20);
    let torn = check(&layout(), 4, &[0; 13], 1 << 20);
    let none = check(&layout(), 0, &[], 1 << 20);
    let roomy = check(&layout(), 4, &[], 47);

    assert_eq!([held, empty, full], [Ok(3), Ok(0), Ok(4)]);
    assert_eq!(past, Err(InstancesRefused::Records { bytes: 60 }));
    assert_eq!(torn, Err(InstancesRefused::Records { bytes: 13 }));
    assert_eq!(none, Err(InstancesRefused::Capacity { capacity: 0 }));
    assert_eq!(roomy, Err(InstancesRefused::Capacity { capacity: 4 }));
}

#[test]
fn a_record_layout_no_buffer_could_have_is_refused() {
    // Catches a layout left unchecked, which WebGPU refuses as a pipeline or a draw reads it.
    let mut torn = layout();
    torn.stride = 10;

    assert_eq!(
        check(&torn, 4, &[], 1 << 20),
        Err(InstancesRefused::Layout { why: MeshRefused::Stride { stride: 10 } })
    );
}

#[test]
fn a_write_lays_records_over_from_its_first_extends_past_the_end_and_cuts_when_empty() {
    // Catches a write that sets the count to its own end, which drops the records after a patch in the middle, or
    // one that only grows, so a swarm that shrinks draws records it left behind.
    let held = Held { stride: 12, capacity: 8, length: 5 };

    let patched = held.write(1, 24);
    let extended = held.write(4, 36);
    let appended = held.write(5, 12);
    let cut = held.write(2, 0);

    assert_eq!([patched, extended, appended, cut], [Ok(5), Ok(7), Ok(6), Ok(2)]);
}

#[test]
fn a_write_past_the_records_held_or_their_room_or_of_torn_records_is_refused() {
    // Catches a write that leaves a gap of records nothing wrote, one past the buffer, or one that is no whole
    // number of records.
    let held = Held { stride: 12, capacity: 8, length: 5 };

    let gap = held.write(6, 12);
    let past = held.write(5, 48);
    let torn = held.write(0, 13);
    let full = held.write(0, 96);

    assert_eq!(gap, Err(WriteRefused::First { first: 6, length: 5 }));
    assert_eq!(past, Err(WriteRefused::Records { bytes: 48 }));
    assert_eq!(torn, Err(WriteRefused::Records { bytes: 13 }));
    assert_eq!(full, Ok(8));
}
