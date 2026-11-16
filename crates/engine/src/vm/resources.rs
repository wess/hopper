//! Host capacity detection, kept out of the UI and the domain model.

pub fn capacity() -> model::EngineCapacity {
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1);
    model::EngineCapacity::from_hardware(cpus, memory())
}

#[cfg(target_os = "macos")]
fn memory() -> Option<u64> {
    use std::ffi::{c_char, c_int, c_void};
    unsafe extern "C" {
        fn sysctlbyname(
            name: *const c_char,
            old: *mut c_void,
            len: *mut usize,
            new: *mut c_void,
            newlen: usize,
        ) -> c_int;
    }
    let mut bytes = 0u64;
    let mut len = std::mem::size_of::<u64>();
    // The fixed sysctl is read-only and writes at most the supplied u64 size.
    let result = unsafe {
        sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&mut bytes as *mut u64).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (result == 0 && len == std::mem::size_of::<u64>() && bytes > 0).then_some(bytes)
}

#[cfg(target_os = "linux")]
fn memory() -> Option<u64> {
    std::fs::read_to_string("/proc/meminfo")
        .ok()?
        .lines()
        .find_map(|line| {
            line.strip_prefix("MemTotal:")?
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()
        })?
        .checked_mul(1024)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn memory() -> Option<u64> {
    None
}
