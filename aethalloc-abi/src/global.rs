//! Global allocator implementation with size tracking
//!
//! This allocator stores a header before each allocation containing the size.
//! The header is placed immediately before the aligned user pointer, ensuring
//! proper alignment for all allocation requests.
//!
//! TODO: Integrate ThreadLocalCache for small allocations (requires TLS support in no_std)

use alloc::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;

use aethalloc_core::page::PageAllocator;

/// Header stored before each allocation to track size and base
#[repr(C)]
struct AllocHeader {
    /// Size requested by the user (not including header)
    size: usize,
    /// Number of pages allocated
    pages: usize,
    /// Offset from header to allocation base (in bytes)
    base_offset: usize,
}

const HEADER_SIZE: usize = core::mem::size_of::<AllocHeader>();
const HEADER_ALIGN: usize = core::mem::align_of::<AllocHeader>();
const PAGE_SIZE: usize = aethalloc_core::page::PAGE_SIZE;

pub struct AethAlloc;

impl AethAlloc {
    pub const fn new() -> Self {
        AethAlloc
    }

    unsafe fn header_from_ptr(ptr: *mut u8) -> *mut AllocHeader {
        ptr.sub(HEADER_SIZE) as *mut AllocHeader
    }

    fn align_up(addr: usize, align: usize) -> usize {
        (addr + align - 1) & !(align - 1)
    }
}

unsafe impl GlobalAlloc for AethAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align().max(HEADER_ALIGN);

        if size == 0 {
            return core::ptr::null_mut();
        }

        let total_size = size + HEADER_SIZE + align - 1;
        let pages = total_size.div_ceil(PAGE_SIZE).max(1);

        match PageAllocator::alloc(pages) {
            Some(base) => {
                let base_addr = base.as_ptr() as usize;
                let user_addr = Self::align_up(base_addr + HEADER_SIZE, align);
                let header_addr = user_addr - HEADER_SIZE;
                let base_offset = header_addr - base_addr;

                let header_ptr = header_addr as *mut AllocHeader;
                core::ptr::write(
                    header_ptr,
                    AllocHeader {
                        size,
                        pages,
                        base_offset,
                    },
                );

                user_addr as *mut u8
            }
            None => core::ptr::null_mut(),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        if ptr.is_null() {
            return;
        }

        let header = Self::header_from_ptr(ptr);
        let AllocHeader {
            pages, base_offset, ..
        } = core::ptr::read(header);

        if pages == 0 {
            return;
        }

        let header_addr = header as usize;
        let base_addr = header_addr - base_offset;
        let base = NonNull::new_unchecked(base_addr as *mut u8);
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
