use super::{NAME, hub};
use aethereal_chassis_headless::native::{Invocation, Invoked, NativeLayers};

#[test]
fn the_hub_reads_its_sections_and_flags_with_no_two_of_one_name() {
    // Catches a root listed here whose settings suggest a flag another listed root already suggests, left so: the hub
    // would refuse to start, as it did when `Http` and `Docker` each suggested `--window`.
    let invocation = Invocation::new([NAME], Vec::<(String, String)>::new());

    let read = hub().read(NAME, invocation);

    let runs = matches!(read, Ok(Invoked::Run(_)));
    assert!(runs, "the hub reads an invocation of no arguments: {:?}", read.err());
}
