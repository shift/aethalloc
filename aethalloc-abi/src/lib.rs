//! AethAlloc ABI - C-compatible allocator interface for LD_PRELOAD injection

#![cfg_attr(not(test), no_std)]

extern crate alloc;

#[cfg(test)]
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

    // Get old size from header
    let old_size = unsafe { global::get_alloc_size(ptr) };

    let new_ptr = malloc(size);
    if !new_ptr.is_null() {
        // Copy min(old_size, new_size) bytes
        let copy_size = old_size.min(size);
        unsafe {
            core::ptr::copy_nonoverlapping(ptr, new_ptr, copy_size);
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

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
