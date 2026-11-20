//! Hardware facts used by recommendation rules (design 4.5).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Macos,
    Windows,
    Linux,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum Arch {
    #[serde(rename = "aarch64")]
    Aarch64,
    #[serde(rename = "x86_64")]
    X86_64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HardwareInfo {
    /// Total system memory in GB, rounded down.
    pub memory_gb: u32,
    pub os: Os,
    pub arch: Arch,
}

impl HardwareInfo {
    pub fn new(memory_gb: u32, os: Os, arch: Arch) -> Self {
        HardwareInfo {
            memory_gb,
            os,
            arch,
        }
    }

    /// Detect the current machine. Memory is 0 when it cannot be determined,
    /// which makes memory-gated recommendations not match (the safe side).
    pub fn detect() -> HardwareInfo {
        let os = if cfg!(target_os = "macos") {
            Os::Macos
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else {
            Os::Linux
        };
        let arch = if cfg!(target_arch = "aarch64") {
            Arch::Aarch64
        } else {
            Arch::X86_64
        };
        let bytes = total_memory_bytes().unwrap_or(0);
        HardwareInfo {
            memory_gb: (bytes / (1024 * 1024 * 1024)) as u32,
            os,
            arch,
        }
    }
}

#[cfg(target_os = "macos")]
fn total_memory_bytes() -> Option<u64> {
    let mut value: u64 = 0;
    let mut len = std::mem::size_of::<u64>();
    // SAFETY: the name is a valid NUL-terminated string and `value`/`len`
    // describe a writable buffer of the size sysctl is told about.
    let rc = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&mut value as *mut u64).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (rc == 0).then_some(value)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn total_memory_bytes() -> Option<u64> {
    // SAFETY: sysconf has no preconditions.
    let (pages, size) = unsafe {
        (
            libc::sysconf(libc::_SC_PHYS_PAGES),
            libc::sysconf(libc::_SC_PAGESIZE),
        )
    };
    (pages > 0 && size > 0).then(|| pages as u64 * size as u64)
}

#[cfg(windows)]
fn total_memory_bytes() -> Option<u64> {
    #[repr(C)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page_file: u64,
        avail_page_file: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended_virtual: u64,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
    }
    let mut s = MemoryStatusEx {
        length: std::mem::size_of::<MemoryStatusEx>() as u32,
        memory_load: 0,
        total_phys: 0,
        avail_phys: 0,
        total_page_file: 0,
        avail_page_file: 0,
        total_virtual: 0,
        avail_virtual: 0,
        avail_extended_virtual: 0,
    };
    // SAFETY: `s` is a properly sized, initialised MEMORYSTATUSEX.
    let ok = unsafe { GlobalMemoryStatusEx(&mut s) };
    (ok != 0).then_some(s.total_phys)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_reports_something_on_this_machine() {
        let hw = HardwareInfo::detect();
        assert!(hw.memory_gb >= 1);
    }
}
