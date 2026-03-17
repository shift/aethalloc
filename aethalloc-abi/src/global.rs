//! Global allocator implementation with size tracking
//!
//! This allocator stores a header before each allocation containing the size.
//! This allows proper dealloc and realloc implementation.

use alloc::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;

use aethalloc_core::page::PageAllocator;
use aethalloc_core::size_class::{round_up_pow2, SizeClass};

/// Header stored before each allocation to track size
#[repr(C)]
struct AllocHeader {
    /// Size requested by the user (not including header)
    size: usize,
    /// Number of pages allocated
    pages: usize,
}

const HEADER_SIZE: usize = core::mem::size_of::<AllocHeader>();
const PAGE_SIZE: usize = aethalloc_core::page::PAGE_SIZE;

pub struct AethAlloc;

impl AethAlloc {
    pub const fn new() -> Self {
        AethAlloc
    }

    /// Get the header from a user pointer
    ///
    /// # Safety
    /// ptr must be a valid pointer returned from alloc()
    unsafe fn header_from_ptr(ptr: *mut u8) -> *mut AllocHeader {
        ptr.sub(HEADER_SIZE) as *mut AllocHeader
    }

    /// Get the user pointer from an allocation base
    ///
    /// # Safety
    /// base must be a valid pointer to the start of allocated memory
    unsafe fn ptr_from_base(base: NonNull<u8>) -> *mut u8 {
        base.as_ptr().add(HEADER_SIZE)
    }
}

// SAFETY: This allocator is thread-safe. All operations use the PageAllocator
// which internally uses atomic operations for mmap/munmap. The header-based
// size tracking ensures each pointer maps to exactly one allocation.
unsafe impl GlobalAlloc for AethAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align();

        if size == 0 {
            return core::ptr::null_mut();
        }

        // Calculate total size needed (header + user data, aligned)
        let total_size = size + HEADER_SIZE;

        // Determine allocation strategy based on size class
        let size_class = SizeClass::classify(size);
        let pages = match size_class {
            SizeClass::Tiny | SizeClass::Small => {
                let rounded = round_up_pow2(total_size.max(align));
                let page_aligned = rounded.max(PAGE_SIZE);
                let p = page_aligned / PAGE_SIZE;
                p.max(1)
            }
            SizeClass::Medium | SizeClass::Large => total_size.div_ceil(PAGE_SIZE),
        };

        // Allocate pages
        match PageAllocator::alloc(pages) {
            Some(base) => {
                // Store header at the beginning
                let header_ptr = base.as_ptr() as *mut AllocHeader;
                core::ptr::write(header_ptr, AllocHeader { size, pages });

                // Return pointer after header
                Self::ptr_from_base(base)
            }
            None => core::ptr::null_mut(),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        if ptr.is_null() {
            return;
        }

        // Read header to get allocation info
        let header = Self::header_from_ptr(ptr);
        let AllocHeader { pages, .. } = core::ptr::read(header);

        // Get base pointer (before header)
        let base = NonNull::new_unchecked(ptr.sub(HEADER_SIZE));

        // Free the pages
        PageAllocator::dealloc(base, pages);
    }
}

/// Get the size of an allocation from its pointer
///
/// # Safety
/// ptr must be a valid pointer returned from malloc()
pub unsafe fn get_alloc_size(ptr: *mut u8) -> usize {
    if ptr.is_null() {
        return 0;
    }
    let header = AethAlloc::header_from_ptr(ptr);
    core::ptr::read(header).size
}
