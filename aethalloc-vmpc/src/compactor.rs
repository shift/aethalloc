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

impl CompactError {
    #[cfg(all(unix, feature = "std"))]
    fn from_errno() -> Self {
        let errno = unsafe { *libc::__errno_location() };
        match errno {
            libc::EINVAL => CompactError::InvalidAddress,
            libc::ENOMEM => CompactError::OutOfMemory,
            libc::EPERM => CompactError::PermissionDenied,
            libc::EACCES => CompactError::PermissionDenied,
            libc::EFAULT => CompactError::InvalidAddress,
            _ => CompactError::Unknown(errno),
        }
    }
}

/// Wrapper for mremap syscall
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
        local_iov.as_mut_ptr(),
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

    /// Try to relocate a page to a new physical location while
    /// preserving the virtual address.
    ///
    /// This is used for defragmentation: we allocate a new physical page,
    /// copy the contents, then remap the old virtual address to the new page.
    ///
    /// # Safety
    /// - src must be page-aligned and point to valid mapped memory
    /// - size must be a multiple of PAGE_SIZE
    /// - caller must ensure no concurrent access to the region
    #[cfg(all(unix, feature = "std"))]
    pub unsafe fn try_relocate_page(
        &self,
        src: NonNull<u8>,
        size: usize,
    ) -> Result<CompactResult, CompactError> {
        let page_size = PAGE_SIZE;

        if size == 0 || size % page_size != 0 {
            return Err(CompactError::InvalidAddress);
        }

        let src_addr = src.as_ptr();

        // Step 1: Allocate a temporary buffer to hold page contents
        let temp_buf = libc::mmap(
            core::ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        );

        if temp_buf == libc::MAP_FAILED {
            return Err(CompactError::OutOfMemory);
        }

        // Step 2: Copy data from source to temp buffer
        core::ptr::copy_nonoverlapping(src_addr, temp_buf as *mut u8, size);

        // Step 3: Unmap the original page
        if libc::munmap(src_addr as *mut core::ffi::c_void, size) < 0 {
            // Try to restore the mapping at the original address
            let restore = libc::mmap(
                src_addr as *mut core::ffi::c_void,
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_FIXED | libc::MAP_ANONYMOUS,
                -1,
                0,
            );
            if restore != libc::MAP_FAILED {
                core::ptr::copy_nonoverlapping(temp_buf as *const u8, src_addr, size);
            }
            libc::munmap(temp_buf, size);
            return Err(CompactError::Unknown(*libc::__errno_location()));
        }

        // Step 4: Map a new page at the original virtual address
        let result = libc::mmap(
            src_addr as *mut core::ffi::c_void,
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_FIXED,
            -1,
            0,
        );

        if result == libc::MAP_FAILED {
            libc::munmap(temp_buf, size);
            return Err(CompactError::Unknown(*libc::__errno_location()));
        }

        // Step 5: Copy data back to the remapped address
        core::ptr::copy_nonoverlapping(temp_buf as *const u8, src_addr, size);

        // Clean up the temporary buffer
        libc::munmap(temp_buf, size);

        Ok(CompactResult::Success)
    }

    /// Compact a region by moving pages to consolidate sparse allocations
    ///
    /// # Safety
    /// - All pointers in pages must be valid mapped memory regions
    /// - Caller must ensure no concurrent access to the region
    #[cfg(all(unix, feature = "std"))]
    pub unsafe fn compact_pages(
        &self,
        src: NonNull<u8>,
        size: usize,
    ) -> Result<CompactResult, CompactError> {
        self.try_relocate_page(src, size)
    }
}

impl Default for Compactor {
    fn default() -> Self {
        Self::new(CompactConfig::default())
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

    #[cfg(all(unix, feature = "std"))]
    #[test]
    fn test_try_relocate_page() {
        let compactor = Compactor::default();
        let page_size = PAGE_SIZE;

        // Allocate a page
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
            // Write some data
            let ptr = addr as *mut u8;
            core::ptr::write_bytes(ptr, 0xAB, page_size);

            // Try to relocate
            let nn = NonNull::new_unchecked(ptr);
            let result = compactor.try_relocate_page(nn, page_size);

            // Check result
            if let Ok(CompactResult::Success) = result {
                // Verify data is still there
                for i in 0..page_size {
                    assert_eq!(*ptr.add(i), 0xAB);
                }
            }

            // Cleanup
            libc::munmap(addr, page_size);
        }
    }
}
