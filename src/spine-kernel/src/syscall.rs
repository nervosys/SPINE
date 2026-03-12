//! Direct System Call Interface
//!
//! Raw syscall wrappers for maximum performance, bypassing libc overhead.
//! Use these only when libc wrapper overhead is measurable.
//!
//! **Warning**: These are unsafe and platform-specific.

use std::io::{Error, Result};

// =============================================================================
// MEMORY MAPPING
// =============================================================================

/// Memory protection flags
#[derive(Debug, Clone, Copy)]
pub struct MemProt(pub i32);

impl MemProt {
    pub const NONE: Self = Self(0);
    pub const READ: Self = Self(1);
    pub const WRITE: Self = Self(2);
    pub const EXEC: Self = Self(4);
    pub const READ_WRITE: Self = Self(1 | 2);
    pub const READ_EXEC: Self = Self(1 | 4);
    pub const READ_WRITE_EXEC: Self = Self(1 | 2 | 4);
}

/// Memory mapping flags
#[derive(Debug, Clone, Copy)]
pub struct MemFlags(pub i32);

impl MemFlags {
    pub const SHARED: Self = Self(0x01);
    pub const PRIVATE: Self = Self(0x02);
    pub const ANONYMOUS: Self = Self(0x20);
    pub const FIXED: Self = Self(0x10);
    #[cfg(target_os = "linux")]
    pub const HUGE_2MB: Self = Self(0x200000 | 0x40000);
    #[cfg(target_os = "linux")]
    pub const HUGE_1GB: Self = Self(0x40000000 | 0x40000);
    pub const POPULATE: Self = Self(0x8000);
    pub const LOCKED: Self = Self(0x2000);
}

impl std::ops::BitOr for MemFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// Memory map a region (Linux)
///
/// # Safety
///
/// - `len` must be non-zero and a multiple of the page size
/// - If `addr` is `Some`, the caller must ensure the address is valid and properly aligned
/// - If `fd` is valid, it must refer to a file that can be memory-mapped
/// - The returned pointer must be unmapped with `munmap` when no longer needed
#[cfg(target_os = "linux")]
pub unsafe fn mmap(
    addr: Option<*mut u8>,
    len: usize,
    prot: MemProt,
    flags: MemFlags,
    fd: i32,
    offset: i64,
) -> Result<*mut u8> {
    let addr = addr
        .map(|p| p as *mut libc::c_void)
        .unwrap_or(std::ptr::null_mut());

    let result = libc::mmap(addr, len, prot.0, flags.0, fd, offset);

    if result == libc::MAP_FAILED {
        Err(Error::last_os_error())
    } else {
        Ok(result as *mut u8)
    }
}

/// Memory map a region (Windows)
///
/// # Safety
///
/// - `len` must be non-zero
/// - The returned pointer must be freed with `munmap` (VirtualFree) when no longer needed
/// - The caller must ensure proper synchronization when multiple threads access the mapped memory
#[cfg(target_os = "windows")]
pub unsafe fn mmap(
    _addr: Option<*mut u8>,
    len: usize,
    prot: MemProt,
    _flags: MemFlags,
    _fd: i32,
    _offset: i64,
) -> Result<*mut u8> {
    use windows_sys::Win32::System::Memory::*;

    let protect = match (prot.0 & 1 != 0, prot.0 & 2 != 0, prot.0 & 4 != 0) {
        (true, true, true) => PAGE_EXECUTE_READWRITE,
        (true, true, false) => PAGE_READWRITE,
        (true, false, true) => PAGE_EXECUTE_READ,
        (true, false, false) => PAGE_READONLY,
        (false, false, false) => PAGE_NOACCESS,
        _ => PAGE_READWRITE,
    };

    let ptr = VirtualAlloc(std::ptr::null(), len, MEM_COMMIT | MEM_RESERVE, protect);

    if ptr.is_null() {
        Err(Error::last_os_error())
    } else {
        Ok(ptr as *mut u8)
    }
}

/// Unmap a memory region (Linux)
///
/// # Safety
///
/// - `addr` must be a pointer previously returned by `mmap`
/// - `len` must match the length used in the corresponding `mmap` call
/// - The memory region must not be accessed after this call returns
#[cfg(target_os = "linux")]
pub unsafe fn munmap(addr: *mut u8, len: usize) -> Result<()> {
    if libc::munmap(addr as *mut libc::c_void, len) == 0 {
        Ok(())
    } else {
        Err(Error::last_os_error())
    }
}

/// Unmap a memory region (Windows)
///
/// # Safety
///
/// - `addr` must be a pointer previously returned by `mmap` (VirtualAlloc)
/// - The memory region must not be accessed after this call returns
#[cfg(target_os = "windows")]
pub unsafe fn munmap(addr: *mut u8, _len: usize) -> Result<()> {
    use windows_sys::Win32::System::Memory::*;

    if VirtualFree(addr as *mut _, 0, MEM_RELEASE) != 0 {
        Ok(())
    } else {
        Err(Error::last_os_error())
    }
}

// =============================================================================
// MAPPED REGION (RAII WRAPPER)
// =============================================================================

/// RAII wrapper for memory-mapped regions.
///
/// Automatically calls `munmap` on `Drop`, preventing memory leaks from
/// mismatched mmap/munmap calls. This addresses MISRA D7 (R.21.3).
///
/// # Examples
///
/// ```no_run
/// use spine_kernel::syscall::*;
/// let region = MappedRegion::new(4096, MemProt::READ_WRITE, MemFlags::PRIVATE | MemFlags::ANONYMOUS).unwrap();
/// let ptr = region.as_ptr();
/// // region automatically unmapped on drop
/// ```
pub struct MappedRegion {
    ptr: *mut u8,
    len: usize,
}

impl MappedRegion {
    /// Create a new anonymous memory-mapped region.
    ///
    /// This is the safe equivalent of `mmap` + `munmap` with RAII cleanup.
    pub fn new(len: usize, prot: MemProt, flags: MemFlags) -> Result<Self> {
        if len == 0 {
            return Err(Error::new(
                std::io::ErrorKind::InvalidInput,
                "len must be > 0",
            ));
        }
        // SAFETY: len > 0, anonymous mapping (fd=-1, offset=0), no fixed address
        let ptr = unsafe { mmap(None, len, prot, flags, -1, 0)? };
        Ok(Self { ptr, len })
    }

    /// Get a raw pointer to the mapped memory.
    #[inline]
    pub fn as_ptr(&self) -> *mut u8 {
        self.ptr
    }

    /// Get the length of the mapped region in bytes.
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns true if the region has zero length (never happens after construction).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Get a byte slice view of the mapped memory.
    ///
    /// # Safety
    ///
    /// Caller must ensure no concurrent writes to the region.
    pub unsafe fn as_slice(&self) -> &[u8] {
        std::slice::from_raw_parts(self.ptr, self.len)
    }

    /// Get a mutable byte slice view of the mapped memory.
    ///
    /// # Safety
    ///
    /// Caller must ensure exclusive access to the region.
    pub unsafe fn as_mut_slice(&mut self) -> &mut [u8] {
        std::slice::from_raw_parts_mut(self.ptr, self.len)
    }
}

impl Drop for MappedRegion {
    fn drop(&mut self) {
        // SAFETY: ptr and len are valid from the mmap call in `new()`
        unsafe {
            let _ = munmap(self.ptr, self.len);
        }
    }
}

// SAFETY: MappedRegion owns its memory region exclusively
unsafe impl Send for MappedRegion {}

/// Lock memory to prevent swapping (Linux)
///
/// # Safety
///
/// - `addr` must point to a valid memory region of at least `len` bytes
/// - The memory region must remain valid for the duration of the lock
#[cfg(target_os = "linux")]
pub unsafe fn mlock(addr: *const u8, len: usize) -> Result<()> {
    if libc::mlock(addr as *const libc::c_void, len) == 0 {
        Ok(())
    } else {
        Err(Error::last_os_error())
    }
}

/// Lock memory to prevent swapping (Windows)
///
/// # Safety
///
/// - `addr` must point to a valid memory region of at least `len` bytes
/// - The memory region must remain valid for the duration of the lock
#[cfg(target_os = "windows")]
pub unsafe fn mlock(addr: *const u8, len: usize) -> Result<()> {
    use windows_sys::Win32::System::Memory::VirtualLock;

    if VirtualLock(addr as *mut _, len) != 0 {
        Ok(())
    } else {
        Err(Error::last_os_error())
    }
}

/// Advise the kernel about memory usage patterns (Linux only)
#[cfg(target_os = "linux")]
pub unsafe fn madvise(addr: *mut u8, len: usize, advice: MadviseAdvice) -> Result<()> {
    if libc::madvise(addr as *mut libc::c_void, len, advice.0) == 0 {
        Ok(())
    } else {
        Err(Error::last_os_error())
    }
}

/// Memory advice flags (Linux only)
#[derive(Debug, Clone, Copy)]
pub struct MadviseAdvice(pub i32);

#[cfg(target_os = "linux")]
impl MadviseAdvice {
    pub const NORMAL: Self = Self(0);
    pub const RANDOM: Self = Self(1);
    pub const SEQUENTIAL: Self = Self(2);
    pub const WILLNEED: Self = Self(3);
    pub const DONTNEED: Self = Self(4);
    pub const HUGEPAGE: Self = Self(14);
    pub const NOHUGEPAGE: Self = Self(15);
}

// =============================================================================
// CPU AFFINITY
// =============================================================================

/// Set CPU affinity for the current thread (Linux)
#[cfg(target_os = "linux")]
pub fn set_cpu_affinity(cpu: usize) -> Result<()> {
    use std::mem;

    unsafe {
        let mut cpuset: libc::cpu_set_t = mem::zeroed();
        libc::CPU_ZERO(&mut cpuset);
        libc::CPU_SET(cpu, &mut cpuset);

        let result = libc::sched_setaffinity(0, mem::size_of::<libc::cpu_set_t>(), &cpuset);
        if result == 0 {
            Ok(())
        } else {
            Err(Error::last_os_error())
        }
    }
}

/// Set CPU affinity for the current thread (Windows)
#[cfg(target_os = "windows")]
pub fn set_cpu_affinity(cpu: usize) -> Result<()> {
    use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadAffinityMask};

    unsafe {
        let mask = 1usize << cpu;
        let result = SetThreadAffinityMask(GetCurrentThread(), mask);
        if result != 0 {
            Ok(())
        } else {
            Err(Error::last_os_error())
        }
    }
}

/// Get the current CPU number (Linux)
#[cfg(target_os = "linux")]
pub fn get_cpu() -> usize {
    unsafe { libc::sched_getcpu() as usize }
}

/// Get the current CPU number (Windows)  
#[cfg(target_os = "windows")]
pub fn get_cpu() -> usize {
    // Simplified: return 0 as GetCurrentProcessorNumber requires additional features
    0
}

// =============================================================================
// NUMA SUPPORT
// =============================================================================

/// NUMA node information
#[derive(Debug, Clone)]
pub struct NumaInfo {
    pub num_nodes: usize,
    pub current_node: usize,
}

/// Get NUMA information (Linux)
#[cfg(target_os = "linux")]
pub fn numa_info() -> Result<NumaInfo> {
    let num_nodes = std::fs::read_dir("/sys/devices/system/node")
        .map(|entries| {
            entries
                .filter(|e| {
                    e.as_ref()
                        .map(|e| e.file_name().to_string_lossy().starts_with("node"))
                        .unwrap_or(false)
                })
                .count()
        })
        .unwrap_or(1);

    Ok(NumaInfo {
        num_nodes,
        current_node: 0,
    })
}

/// Get NUMA information (Windows)
#[cfg(target_os = "windows")]
pub fn numa_info() -> Result<NumaInfo> {
    // Simplified: assume single NUMA node
    Ok(NumaInfo {
        num_nodes: 1,
        current_node: 0,
    })
}

// =============================================================================
// HUGE PAGES
// =============================================================================

/// Allocate huge pages (2MB or 1GB) - Linux
#[cfg(target_os = "linux")]
pub unsafe fn alloc_huge_pages(size: usize, huge_1gb: bool) -> Result<*mut u8> {
    let flags = if huge_1gb {
        MemFlags::PRIVATE | MemFlags::ANONYMOUS | MemFlags::HUGE_1GB
    } else {
        MemFlags::PRIVATE | MemFlags::ANONYMOUS | MemFlags::HUGE_2MB
    };

    mmap(None, size, MemProt::READ_WRITE, flags, -1, 0)
}

/// Allocate huge pages - Windows (requires admin privileges)
///
/// # Safety
///
/// - `size` must be non-zero and aligned to the large page size
/// - The caller must have the SeLockMemoryPrivilege enabled
/// - The returned pointer must be freed with VirtualFree when no longer needed
#[cfg(target_os = "windows")]
pub unsafe fn alloc_huge_pages(size: usize, _huge_1gb: bool) -> Result<*mut u8> {
    use windows_sys::Win32::System::Memory::*;

    let ptr = VirtualAlloc(
        std::ptr::null(),
        size,
        MEM_COMMIT | MEM_RESERVE | MEM_LARGE_PAGES,
        PAGE_READWRITE,
    );

    if ptr.is_null() {
        Err(Error::last_os_error())
    } else {
        Ok(ptr as *mut u8)
    }
}

// =============================================================================
// THREAD PRIORITY
// =============================================================================

/// Thread priority level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    Idle,
    Low,
    Normal,
    High,
    Realtime,
}

/// Set thread priority (Linux)
#[cfg(target_os = "linux")]
pub fn set_thread_priority(priority: Priority) -> Result<()> {
    let policy = match priority {
        Priority::Realtime => libc::SCHED_FIFO,
        _ => libc::SCHED_OTHER,
    };

    let nice = match priority {
        Priority::Idle => 19,
        Priority::Low => 10,
        Priority::Normal => 0,
        Priority::High => -10,
        Priority::Realtime => -20,
    };

    unsafe {
        if priority == Priority::Realtime {
            let param = libc::sched_param { sched_priority: 99 };
            if libc::sched_setscheduler(0, policy, &param) != 0 {
                return Err(Error::last_os_error());
            }
        }

        if libc::setpriority(libc::PRIO_PROCESS, 0, nice) != 0 {
            return Err(Error::last_os_error());
        }

        Ok(())
    }
}

/// Set thread priority (Windows)
#[cfg(target_os = "windows")]
pub fn set_thread_priority(priority: Priority) -> Result<()> {
    use windows_sys::Win32::System::Threading::*;

    let level = match priority {
        Priority::Idle => THREAD_PRIORITY_IDLE,
        Priority::Low => THREAD_PRIORITY_BELOW_NORMAL,
        Priority::Normal => THREAD_PRIORITY_NORMAL,
        Priority::High => THREAD_PRIORITY_ABOVE_NORMAL,
        Priority::Realtime => THREAD_PRIORITY_TIME_CRITICAL,
    };

    unsafe {
        if SetThreadPriority(GetCurrentThread(), level) != 0 {
            Ok(())
        } else {
            Err(Error::last_os_error())
        }
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_cpu() {
        let cpu = get_cpu();
        println!("Current CPU: {}", cpu);
    }

    #[test]
    fn test_numa_info() {
        if let Ok(info) = numa_info() {
            println!("NUMA nodes: {}", info.num_nodes);
        }
    }

    #[test]
    fn test_mmap() {
        unsafe {
            let ptr = mmap(
                None,
                4096,
                MemProt::READ_WRITE,
                MemFlags::PRIVATE | MemFlags::ANONYMOUS,
                -1,
                0,
            )
            .unwrap();

            // Write and read
            *ptr = 42;
            assert_eq!(*ptr, 42);

            munmap(ptr, 4096).unwrap();
        }
    }
}
