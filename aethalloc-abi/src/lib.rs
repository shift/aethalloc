//! AethAlloc ABI - C-compatible allocator interface for LD_PRELOAD injection

#![feature(thread_local)]

extern crate alloc;
extern crate std;

use alloc::alloc::{GlobalAlloc, Layout};
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

mod global;

#[global_allocator]
static ALLOCATOR: global::AethAlloc = global::AethAlloc::new();

static INITIALIZED: AtomicBool = AtomicBool::new(false);

fn ensure_init() {
    if !INITIALIZED.load(Ordering::Acquire) {
        INITIALIZED.store(true, Ordering::Release);
        global::ensure_support_core();
    }
}

#[no_mangle]
pub extern "C" fn malloc(size: usize) -> *mut u8 {
    ensure_init();
    if size == 0 {
        return ptr::null_mut();
    }

    let layout = Layout::from_size_align(size, 8).ok();
    match layout {
        Some(l) => unsafe { ALLOCATOR.alloc(l) },
        None => ptr::null_mut(),
    }
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn free(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        ALLOCATOR.dealloc(ptr, Layout::new::<u8>());
    }
}

#[no_mangle]
pub extern "C" fn calloc(nmemb: usize, size: usize) -> *mut u8 {
    let total = match nmemb.checked_mul(size) {
        Some(t) => t,
        None => return ptr::null_mut(),
    };
    let ptr = malloc(total);
    if !ptr.is_null() {
        unsafe { core::ptr::write_bytes(ptr, 0, total) };
    }
    ptr
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn realloc(ptr: *mut u8, size: usize) -> *mut u8 {
    if ptr.is_null() {
        return malloc(size);
    }
    if size == 0 {
        free(ptr);
        return ptr::null_mut();
    }

    let old_size = unsafe { global::get_alloc_size(ptr) };
    if old_size == 0 {
        return ptr::null_mut();
    }

    if size <= old_size {
        return ptr;
    }

    // For large allocations, check if the new size fits in the padded allocation.
    // Large allocations are allocated with 2x padding, so reallocs up to 2x can
    // return the same pointer without any mremap or copy.
    if old_size > global::MAX_CACHE_SIZE {
        let large_header_addr =
            unsafe { ptr.sub(global::LARGE_HEADER_SIZE) as *const global::LargeAllocHeader };
        if unsafe { core::ptr::read(large_header_addr).magic } == global::LARGE_MAGIC {
            let base_ptr = unsafe { core::ptr::read(large_header_addr).base_ptr };
            let page_header = unsafe { core::ptr::read(base_ptr as *const global::PageHeader) };
            if page_header.magic == global::MAGIC {
                // Check if new size fits in padded allocation (2x old_size)
                let padded_capacity = page_header.num_pages as usize * global::PAGE_SIZE
                    - global::PAGE_HEADER_SIZE
                    - global::LARGE_HEADER_SIZE
                    - 8;
                if size <= padded_capacity {
                    // Fits in existing allocation - just update the header
                    let new_header_ptr = base_ptr as *mut global::PageHeader;
                    unsafe {
                        core::ptr::write(
                            new_header_ptr,
                            global::PageHeader {
                                magic: global::MAGIC,
                                num_pages: page_header.num_pages,
                                requested_size: size,
                                tag: page_header.tag,
                            },
                        );
                    }
                    return ptr;
                }
                // Doesn't fit - need to reallocate
                let new_ptr = malloc(size);
                if !new_ptr.is_null() {
                    unsafe {
                        core::ptr::copy_nonoverlapping(ptr, new_ptr, old_size);
                    }
                    free(ptr);
                }
                return new_ptr;
            }
        }
    }

    // Fallback: malloc + memcpy + free
    let new_ptr = malloc(size);
    if !new_ptr.is_null() {
        unsafe {
            core::ptr::copy_nonoverlapping(ptr, new_ptr, old_size);
        }
        free(ptr);
    }
    new_ptr
}

#[no_mangle]
pub extern "C" fn aligned_alloc(alignment: usize, size: usize) -> *mut u8 {
    if alignment == 0 || !alignment.is_power_of_two() {
        return ptr::null_mut();
    }

    let layout = Layout::from_size_align(size.max(1), alignment).ok();
    match layout {
        Some(l) => unsafe { ALLOCATOR.alloc(l) },
        None => ptr::null_mut(),
    }
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn posix_memalign(memptr: *mut *mut u8, alignment: usize, size: usize) -> i32 {
    if alignment == 0
        || !alignment.is_power_of_two()
        || !alignment.is_multiple_of(core::mem::size_of::<*mut u8>())
    {
        return 22; // EINVAL
    }

    let ptr = aligned_alloc(alignment, size);
    if ptr.is_null() && size != 0 {
        return 12; // ENOMEM
    }

    unsafe {
        *memptr = ptr;
    }
    0
}
