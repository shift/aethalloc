//! Page management via mmap/munmap

use core::ptr::NonNull;

pub const PAGE_SIZE: usize = 4096;

/// Page allocator using mmap/munmap syscalls
pub struct PageAllocator;

impl PageAllocator {
    /// Allocate `pages` pages of memory
    ///
    /// # Safety
    /// - Returns properly aligned memory or null
    /// - Memory is zero-initialized
    pub unsafe fn alloc(pages: usize) -> Option<NonNull<u8>> {
        if pages == 0 {
            return NonNull::new(core::ptr::NonNull::dangling().as_ptr());
        }

        let size = pages * PAGE_SIZE;

        // SAFETY: We're calling mmap with valid parameters:
        // - null pointer lets the system choose the address
        // - size is non-zero and page-aligned
        // - PROT_READ | PROT_WRITE for read/write access
        // - MAP_PRIVATE | MAP_ANONYMOUS for private anonymous mapping
        // - -1 fd and 0 offset are required for anonymous mappings
        let ptr = libc::mmap(
            core::ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        );

        if ptr == libc::MAP_FAILED {
            return None;
        }

        // SAFETY: mmap succeeded, so ptr is a valid, aligned pointer to
        // size bytes of zero-initialized memory
        NonNull::new(ptr as *mut u8)
    }

    /// Deallocate memory
    ///
    /// # Safety
    /// - ptr must have been allocated by this allocator
    /// - pages must match the original allocation
    pub unsafe fn dealloc(ptr: NonNull<u8>, pages: usize) {
        if pages == 0 {
            return;
        }

        let size = pages * PAGE_SIZE;

        // SAFETY: ptr was returned by a successful mmap call with this size,
        // and the caller guarantees it hasn't been freed yet
        let result = libc::munmap(ptr.as_ptr() as *mut libc::c_void, size);

        // munmap should not fail with valid parameters, but we can't propagate
        // the error from dealloc. In debug builds, assert on failure.
        debug_assert_eq!(result, 0, "munmap failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloc_dealloc_single_page() {
        // SAFETY: We're testing basic allocation/deallocation
        unsafe {
            let ptr = PageAllocator::alloc(1).expect("allocation should succeed");
            assert_eq!(
                ptr.as_ptr() as usize % PAGE_SIZE,
                0,
                "should be page-aligned"
            );

            // Write to the memory to verify it's usable
            core::ptr::write_volatile(ptr.as_ptr(), 0x42);
            assert_eq!(core::ptr::read_volatile(ptr.as_ptr()), 0x42);

            PageAllocator::dealloc(ptr, 1);
        }
    }

    #[test]
    fn test_alloc_multiple_pages() {
        // SAFETY: Testing multi-page allocation
        unsafe {
            let ptr = PageAllocator::alloc(4).expect("allocation should succeed");

            // Write to the last byte to verify all pages are accessible
            let last_byte = ptr.as_ptr().add(4 * PAGE_SIZE - 1);
            core::ptr::write_volatile(last_byte, 0xFF);
            assert_eq!(core::ptr::read_volatile(last_byte), 0xFF);

            PageAllocator::dealloc(ptr, 4);
        }
    }

    #[test]
    fn test_alloc_zero_pages() {
        // SAFETY: Zero-page allocation should return a dangling pointer
        unsafe {
            let ptr = PageAllocator::alloc(0);
            assert!(ptr.is_some());
        }
    }

    #[test]
    fn test_memory_is_zeroed() {
        // SAFETY: Testing that mmap returns zeroed memory
        unsafe {
            let ptr = PageAllocator::alloc(1).expect("allocation should succeed");

            // Check that first 256 bytes are zeroed
            for i in 0..256 {
                assert_eq!(
                    core::ptr::read_volatile(ptr.as_ptr().add(i)),
                    0,
                    "byte {} should be zeroed",
                    i
                );
            }

            PageAllocator::dealloc(ptr, 1);
        }
    }
}
