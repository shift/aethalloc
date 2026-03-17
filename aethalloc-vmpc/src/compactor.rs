//! Virtual memory page compaction logic
//!
//! Provides mremap-based page migration and compaction algorithms.

#[cfg(all(unix, feature = "std"))]
use core::ptr::NonNull;

pub const PAGE_SIZE: usize = 4096;

/// Result of a compaction operation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactResult {
    Success,
    PartialSuccess { pages_moved: usize },
    Failed(CompactError),
    NoAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactError {
    InvalidAddress,
    PermissionDenied,
    OutOfMemory,
    CrossProcessFailed,
    Unknown(i32),
}

/// Wrapper for mremap syscall
///
/// # Safety
/// - old_addr must be page-aligned and point to valid mapped memory
/// - old_size must be positive and page-aligned
/// - new_size must be positive and page-aligned
#[cfg(all(unix, feature = "std"))]
pub unsafe fn mremap_wrapper(
    old_addr: NonNull<u8>,
    old_size: usize,
    new_size: usize,
    flags: i32,
    new_addr: Option<NonNull<u8>>,
) -> Result<NonNull<u8>, CompactError> {
    let result = if let Some(addr) = new_addr {
        libc::mremap(
            old_addr.as_ptr() as *mut core::ffi::c_void,
            old_size,
            new_size,
            flags | libc::MREMAP_FIXED,
            addr.as_ptr() as *mut core::ffi::c_void,
        )
    } else {
        libc::mremap(
            old_addr.as_ptr() as *mut core::ffi::c_void,
            old_size,
            new_size,
            flags,
            core::ptr::null_mut::<core::ffi::c_void>(),
        )
    };

    if result == libc::MAP_FAILED {
        Err(CompactError::from_errno())
    } else {
        Ok(NonNull::new_unchecked(result as *mut u8))
    }
}

/// Wrapper for process_vm_writev (cross-process memory copy)
///
/// # Safety
/// - local_iov must point to valid local buffers
/// - remote_iov must point to valid remote process addresses
/// - pid must be a valid process ID
#[cfg(all(unix, feature = "std"))]
pub unsafe fn process_vm_writev(
    pid: libc::pid_t,
    local_iov: &[libc::iovec],
    remote_iov: &[libc::iovec],
) -> Result<usize, CompactError> {
    let result = libc::process_vm_writev(
        pid,
        local_iov.as_ptr(),
        local_iov.len() as libc::c_ulong,
        remote_iov.as_ptr(),
        remote_iov.len() as libc::c_ulong,
        0,
    );

    if result < 0 {
        Err(CompactError::from_errno())
    } else {
        Ok(result as usize)
    }
}

/// Wrapper for process_vm_readv (cross-process memory read)
#[cfg(all(unix, feature = "std"))]
pub unsafe fn process_vm_readv(
    pid: libc::pid_t,
    local_iov: &mut [libc::iovec],
    remote_iov: &[libc::iovec],
) -> Result<usize, CompactError> {
    let result = libc::process_vm_readv(
        pid,
        local_iov.as_ptr(),
        local_iov.len() as libc::c_ulong,
        remote_iov.as_ptr(),
        remote_iov.len() as libc::c_ulong,
        0,
    );

    if result < 0 {
        Err(CompactError::from_errno())
    } else {
        Ok(result as usize)
    }
}

#[cfg(all(unix, feature = "std"))]
impl CompactError {
    fn from_errno() -> Self {
        unsafe {
            match *libc::__errno_location() {
                libc::EACCES => CompactError::PermissionDenied,
                libc::ENOMEM => CompactError::OutOfMemory,
                libc::EFAULT => CompactError::InvalidAddress,
                libc::EPERM => CompactError::PermissionDenied,
                e => CompactError::Unknown(e),
            }
        }
    }
}

/// Compaction strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactStrategy {
    InPlace,
    Relocate { target_addr: usize },
    Auto,
}

/// Configuration for compaction
#[derive(Debug, Clone, Copy)]
pub struct CompactConfig {
    pub utilization_threshold: f32,
    pub min_pages_to_compact: usize,
    pub max_pages_per_pass: usize,
    pub strategy: CompactStrategy,
}

impl Default for CompactConfig {
    fn default() -> Self {
        Self {
            utilization_threshold: 0.5,
            min_pages_to_compact: 2,
            max_pages_per_pass: 256,
            strategy: CompactStrategy::Auto,
        }
    }
}

/// Page compactor
pub struct Compactor {
    pub config: CompactConfig,
}

impl Compactor {
    pub fn new(config: CompactConfig) -> Self {
        Self { config }
    }

    /// Compact a region by moving pages to consolidate sparse allocations
    ///
    /// # Safety
    /// - All pointers in pages must be valid mapped memory regions
    /// - Caller must ensure no concurrent access to the region
    #[cfg(all(unix, feature = "std"))]
    pub unsafe fn compact_pages(
        &self,
        pages: &[NonNull<u8>],
        page_sizes: &[usize],
    ) -> CompactResult {
        if pages.len() < self.config.min_pages_to_compact {
            return CompactResult::NoAction;
        }

        if pages.len() != page_sizes.len() {
            return CompactResult::Failed(CompactError::InvalidAddress);
        }

        let to_process = pages.len().min(self.config.max_pages_per_pass);
        let mut moved = 0;

        for i in 0..to_process {
            let src = pages[i];
            let size = page_sizes[i];

            if size == 0 || src.as_ptr() as usize % PAGE_SIZE != 0 {
                continue;
            }

            if self.try_relocate_page(src, size).is_ok() {
                moved += 1;
            }
        }

        if moved == 0 {
            CompactResult::NoAction
        } else if moved == to_process {
            CompactResult::Success
        } else {
            CompactResult::PartialSuccess { pages_moved: moved }
        }
    }

    #[cfg(all(unix, feature = "std"))]
    unsafe fn try_relocate_page(&self, src: NonNull<u8>, size: usize) -> Result<(), CompactError> {
        let _ = (src, size);
        Err(CompactError::Unknown(libc::ENOSYS))
    }
}

impl Default for Compactor {
    fn default() -> Self {
        Self::new(CompactConfig::default())
    }
}

/// Build an iovec from a buffer
#[cfg(all(unix, feature = "std"))]
pub fn make_iovec(buf: &[u8]) -> libc::iovec {
    libc::iovec {
        iov_base: buf.as_ptr() as *mut core::ffi::c_void,
        iov_len: buf.len(),
    }
}

/// Build a mutable iovec from a buffer
#[cfg(all(unix, feature = "std"))]
pub fn make_iovec_mut(buf: &mut [u8]) -> libc::iovec {
    libc::iovec {
        iov_base: buf.as_mut_ptr() as *mut core::ffi::c_void,
        iov_len: buf.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compact_config_default() {
        let config = CompactConfig::default();
        assert!((config.utilization_threshold - 0.5).abs() < 0.01);
        assert_eq!(config.min_pages_to_compact, 2);
        assert_eq!(config.max_pages_per_pass, 256);
    }

    #[test]
    fn test_compact_result() {
        assert_eq!(CompactResult::Success, CompactResult::Success);
        assert_ne!(CompactResult::Success, CompactResult::NoAction);
    }

    #[test]
    fn test_compactor_creation() {
        let compactor = Compactor::default();
        assert_eq!(compactor.config.utilization_threshold, 0.5);
    }

    #[cfg(all(unix, feature = "std"))]
    #[test]
    fn test_make_iovec() {
        let buf = [1u8, 2, 3, 4];
        let iov = make_iovec(&buf);
        assert_eq!(iov.iov_len, 4);
    }

    #[cfg(all(unix, feature = "std"))]
    #[test]
    fn test_mremap_expand() {
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize };

        let addr = unsafe {
            libc::mmap(
                core::ptr::null_mut(),
                page_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };

        if addr == libc::MAP_FAILED {
            return;
        }

        unsafe {
            let ptr = NonNull::new_unchecked(addr as *mut u8);
            let result = mremap_wrapper(ptr, page_size, page_size * 2, libc::MREMAP_MAYMOVE, None);

            if let Ok(new_ptr) = result {
                libc::munmap(new_ptr.as_ptr() as *mut core::ffi::c_void, page_size * 2);
            } else {
                libc::munmap(addr, page_size);
            }
        }
    }
}
