//! Resident memory on macOS: `proc_pidinfo`'s task information for the program's own process, which the
//! kernel answers from its own counts without waiting on a device.

use core::ffi::c_int;
use core::mem::{self, MaybeUninit};

/// The bytes of the program resident in memory: none when the kernel does not answer in full.
pub fn resident_bytes() -> Option<u64> {
    let mut info = MaybeUninit::<libc::proc_taskinfo>::zeroed();
    let size = c_int::try_from(mem::size_of::<libc::proc_taskinfo>()).ok()?;
    // SAFETY: `info` is a zeroed `proc_taskinfo` this frame owns, `size` is its size, and `proc_pidinfo` writes at
    // most that many bytes through the pointer. It is read only once the kernel says it wrote all of it.
    let written =
        unsafe { libc::proc_pidinfo(libc::getpid(), libc::PROC_PIDTASKINFO, 0, info.as_mut_ptr().cast(), size) };
    if written != size {
        return None;
    }
    // SAFETY: the kernel wrote every byte of `info`, as `written` says, and every pattern of bytes is a
    // `proc_taskinfo`, whose fields are all integers.
    let info = unsafe { info.assume_init() };

    Some(info.pti_resident_size)
}
