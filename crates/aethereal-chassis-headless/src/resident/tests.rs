use super::*;

#[test]
fn a_native_host_reads_its_resident_memory() {
    // Catches a system whose way of asking was left out, or answers nothing, where the host has one: the band's
    // process meter would read unknown on the desktop.
    let read = resident();

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    assert!(matches!(read, Process::Resident { bytes } if bytes > 0), "{read:?}");
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    assert_eq!(read, Process::Unknown);
}
