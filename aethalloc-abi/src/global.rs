//! Global allocator implementation with size tracking
//!
//! This allocator uses thread-local caching for small allocations (<=8KB)
//! and falls back to direct mmap for larger allocations.

use alloc::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use aethalloc_core::page::PageAllocator;
use aethalloc_core::size_class::round_up_pow2;
use aethalloc_core::thread_local::ThreadLocalCache;

const MAX_CACHE_SIZE: usize = 8192;
const PAGE_SIZE: usize = aethalloc_core::page::PAGE_SIZE;

#[repr(C)]
struct AllocHeader {
    size: usize,
    pages: usize,
    base_offset: usize,
}

const HEADER_SIZE: usize = core::mem::size_of::<AllocHeader>();
const HEADER_ALIGN: usize = core::mem::align_of::<AllocHeader>();

static TLS_KEY: AtomicUsize = AtomicUsize::new(0);
static TLS_KEY_INIT: AtomicBool = AtomicBool::new(false);

fn get_thread_cache() -> Option<*mut ThreadLocalCache> {
    let key = TLS_KEY.load(Ordering::Acquire);
    if key == 0 {
        return None;
    }
    let real_key = (key - 1) as libc::pthread_key_t;

    unsafe {
        let ptr = libc::pthread_getspecific(real_key);
        if ptr.is_null() {
            None
        } else {
            Some(ptr as *mut ThreadLocalCache)
        }
    }
}

fn ensure_thread_cache() -> Option<*mut ThreadLocalCache> {
    if !TLS_KEY_INIT.load(Ordering::Acquire) {
        let mut key: libc::pthread_key_t = 0;
        unsafe {
            if libc::pthread_key_create(&mut key, Some(tls_destructor)) != 0 {
                return None;
            }
        }
        TLS_KEY.store(key.wrapping_add(1) as usize, Ordering::Release);
        TLS_KEY_INIT.store(true, Ordering::Release);
    }

    let key = TLS_KEY.load(Ordering::Acquire);
    if key == 0 {
        return None;
    }
    let real_key = (key - 1) as libc::pthread_key_t;

    unsafe {
        let ptr = libc::pthread_getspecific(real_key);
        if !ptr.is_null() {
            return Some(ptr as *mut ThreadLocalCache);
        }

        let cache_ptr = libc::mmap(
            core::ptr::null_mut(),
            core::mem::size_of::<ThreadLocalCache>(),
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        );

        if cache_ptr == libc::MAP_FAILED {
            return None;
        }

        let cache = cache_ptr as *mut ThreadLocalCache;
        core::ptr::write(cache, ThreadLocalCache::new());

        if libc::pthread_setspecific(real_key, cache as *mut libc::c_void) != 0 {
            libc::munmap(cache_ptr, core::mem::size_of::<ThreadLocalCache>());
            return None;
        }

        Some(cache)
    }
}

unsafe extern "C" fn tls_destructor(ptr: *mut libc::c_void) {
    if ptr.is_null() {
        return;
    }
    let cache = ptr as *mut ThreadLocalCache;
    core::ptr::drop_in_place(cache);
    libc::munmap(ptr, core::mem::size_of::<ThreadLocalCache>());
}

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

        if size <= MAX_CACHE_SIZE && align <= 8 {
            if let Some(cache_ptr) = ensure_thread_cache() {
                let cache = &mut *cache_ptr;
                let total_cache_size = round_up_pow2(size + HEADER_SIZE).max(16);
                if let Some(ptr) = cache.alloc(total_cache_size) {
                    let header_ptr = ptr.as_ptr() as *mut AllocHeader;
                    core::ptr::write(
                        header_ptr,
                        AllocHeader {
                            size,
                            pages: 0,
                            base_offset: 0,
                        },
                    );
                    return header_ptr.add(1) as *mut u8;
                }
            }
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
            size,
            pages,
            base_offset,
        } = core::ptr::read(header);

        if pages == 0 && size <= MAX_CACHE_SIZE {
            if let Some(cache_ptr) = get_thread_cache() {
                let cache = &mut *cache_ptr;
                let total_cache_size = round_up_pow2(size + HEADER_SIZE).max(16);
                let cache_ptr = NonNull::new_unchecked(header as *mut u8);
                cache.dealloc(cache_ptr, total_cache_size);
                return;
            }
        }

        if pages == 0 {
            return;
        }

        let header_addr = header as usize;
        let base_addr = header_addr - base_offset;
        let base = NonNull::new_unchecked(base_addr as *mut u8);
        PageAllocator::dealloc(base, pages);
    }
}

pub unsafe fn get_alloc_size(ptr: *mut u8) -> usize {
    if ptr.is_null() {
        return 0;
    }
    let header = AethAlloc::header_from_ptr(ptr);
    core::ptr::read(header).size
}
