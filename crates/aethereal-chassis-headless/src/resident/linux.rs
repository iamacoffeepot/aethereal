//! Resident memory on Linux: the `VmRSS` line of `/proc/self/status`, which the kernel answers from its own counts
//! without waiting on a device.

use std::fs;

/// The bytes of the program resident in memory: none when the file cannot be read, or holds no such line.
pub fn resident_bytes() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;

    resident_of(&status)
}

/// The bytes the `VmRSS` line of `status`, the text of `/proc/self/status`, says are resident, which it gives in
/// kibibytes.
fn resident_of(status: &str) -> Option<u64> {
    let line = status.lines().find_map(|line| line.strip_prefix("VmRSS:"))?;
    let kibibytes: u64 = line.trim().strip_suffix("kB")?.trim().parse().ok()?;

    kibibytes.checked_mul(1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_resident_line_is_read_in_bytes() {
        // Catches a figure read from the wrong line, left in kibibytes, or read as nothing for the spacing the kernel
        // pads it with.
        let status = "Name:\tperf\nVmPeak:\t  999 kB\nVmRSS:\t   2048 kB\nRssAnon:\t 1024 kB\n";

        assert_eq!(resident_of(status), Some(2048 * 1024));
    }
}
