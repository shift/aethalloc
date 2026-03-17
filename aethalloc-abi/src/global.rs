//! Global allocator implementation with thread-local caching
//!
//! Uses page-level metadata for large allocations and thread-local
//! free lists for small allocations.

use alloc::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};

use aethalloc_core::page::PageAllocator;
use aethalloc_core::size_class::round_up_pow2;

const PAGE_SIZE: usize = aethalloc_core::page::PAGE_SIZE;
const PAGE_MASK: usize = !(PAGE_SIZE - 1);
const MAX_CACHE_SIZE: usize = 8192;
const NUM_SIZE_CLASSES: usize = 10;
const CACHE_CAPACITY: usize = 256;

const MAGIC: u32 = 0xA7E8A110;

#[repr(C)]
struct PageHeader {
    magic: u32,
    num_pages: u16,
    requested_size: usize,
}

const PAGE_HEADER_SIZE: usize = core::mem::size_of::<PageHeader>();
const CACHE_HEADER_SIZE: usize = core::mem::size_of::<usize>();

#[no_mangle]
pub static CACHE_HITS: AtomicUsize = AtomicUsize::new(0);
#[no_mangle]
pub static CACHE_MISSES: AtomicUsize = AtomicUsize::new(0);
#[no_mangle]
pub static DIRECT_ALLOCS: AtomicUsize = AtomicUsize::new(0);

/// Thread-local cache of free blocks per size class
struct ThreadCache {
    heads: [*mut u8; NUM_SIZE_CLASSES],
    counts: [usize; NUM_SIZE_CLASSES],
}

unsafe impl Send for ThreadCache {}

impl ThreadCache {
    const fn new() -> Self {
        Self {
            heads: [core::ptr::null_mut(); NUM_SIZE_CLASSES],
            counts: [0; NUM_SIZE_CLASSES],
        }
    }

    #[inline]
    fn size_to_class(size: usize) -> Option<usize> {
        let rounded = round_up_pow2(size).max(16);
        match rounded {
            16 => Some(0),
            32 => Some(1),
            64 => Some(2),
            128 => Some(3),
            256 => Some(4),
            512 => Some(5),
            1024 => Some(6),
            2048 => Some(7),
            4096 => Some(8),
            8192 => Some(9),
            _ => None,
        }
    }
}

/// Thread-local storage
static mut THREAD_CACHE: UnsafeCell<ThreadCache> = UnsafeCell::new(ThreadCache::new());

unsafe fn get_thread_cache() -> &'static mut ThreadCache {
    &mut *THREAD_CACHE.get()
}

pub struct AethAlloc;

impl AethAlloc {
    pub const fn new() -> Self {
        AethAlloc
    }

    #[inline]
    fn align_up(addr: usize, align: usize) -> usize {
        (addr + align - 1) & !(align - 1)
    }

    #[inline]
    unsafe fn page_header_from_ptr(ptr: *mut u8) -> *mut PageHeader {
        let page_start = (ptr as usize) & PAGE_MASK;
        page_start as *mut PageHeader
    }
}

unsafe impl GlobalAlloc for AethAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align();

        if size == 0 {
            return core::ptr::null_mut();
        }

        // Small allocation with standard alignment - use thread cache
        if size <= MAX_CACHE_SIZE && align <= 8 {
            let cache = get_thread_cache();
            let cache_size = round_up_pow2(size).max(16);

            if let Some(class) = ThreadCache::size_to_class(cache_size) {
                let head = cache.heads[class];

                if !head.is_null() {
                    // Pop from free list
                    let next = core::ptr::read(head as *mut *mut u8);
                    cache.heads[class] = next;
                    cache.counts[class] -= 1;
                    CACHE_HITS.fetch_add(1, Ordering::Relaxed);

                    // Store size and return
                    core::ptr::write(head as *mut usize, size);
                    return head.add(CACHE_HEADER_SIZE);
                }

                CACHE_MISSES.fetch_add(1, Ordering::Relaxed);

                // Allocate new block
                let alloc_size = cache_size + CACHE_HEADER_SIZE;
                let pages = alloc_size.div_ceil(PAGE_SIZE).max(1);

                if let Some(base) = PageAllocator::alloc(pages) {
                    let size_ptr = base.as_ptr() as *mut usize;
                    core::ptr::write(size_ptr, size);
                    return size_ptr.add(1) as *mut u8;
                }
                return core::ptr::null_mut();
            }
        }

        DIRECT_ALLOCS.fetch_add(1, Ordering::Relaxed);

        let min_size = PAGE_HEADER_SIZE + size + align;
        let pages = min_size.div_ceil(PAGE_SIZE).max(1);

        match PageAllocator::alloc(pages) {
            Some(base) => {
                let base_addr = base.as_ptr() as usize;

                let header = PageHeader {
                    magic: MAGIC,
                    num_pages: pages as u16,
                    requested_size: size,
                };
                let header_ptr = base.as_ptr() as *mut PageHeader;
                core::ptr::write(header_ptr, header);

                let user_addr = Self::align_up(base_addr + PAGE_HEADER_SIZE, align);
                user_addr as *mut u8
            }
            None => core::ptr::null_mut(),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        if ptr.is_null() {
            return;
        }

        let size_ptr = ptr.sub(CACHE_HEADER_SIZE) as *mut usize;
        let maybe_size = core::ptr::read(size_ptr);

        // Check if this is a small allocation
        if maybe_size > 0 && maybe_size <= MAX_CACHE_SIZE {
            let potential_header = size_ptr as *mut PageHeader;
            if core::ptr::read(potential_header).magic != MAGIC {
                let cache = get_thread_cache();
                let cache_size = round_up_pow2(maybe_size).max(16);

                if let Some(class) = ThreadCache::size_to_class(cache_size) {
                    if cache.counts[class] < CACHE_CAPACITY {
                        // Push to free list
                        let head_ptr = size_ptr as *mut *mut u8;
                        core::ptr::write(head_ptr, cache.heads[class]);
                        cache.heads[class] = size_ptr as *mut u8;
                        cache.counts[class] += 1;
                        return;
                    }
                }

                // Cache full - free directly
                let alloc_size = cache_size + CACHE_HEADER_SIZE;
                let pages = alloc_size.div_ceil(PAGE_SIZE).max(1);
                let base = NonNull::new_unchecked(size_ptr as *mut u8);
                PageAllocator::dealloc(base, pages);
                return;
            }
        }

        // Large allocation - find page header and free
        let header = Self::page_header_from_ptr(ptr);
        let header_ref = core::ptr::read(header);

        if header_ref.magic == MAGIC && header_ref.num_pages > 0 {
            let base = NonNull::new_unchecked(header as *mut u8);
            PageAllocator::dealloc(base, header_ref.num_pages as usize);
        }
    }
}

pub unsafe fn get_alloc_size(ptr: *mut u8) -> usize {
    if ptr.is_null() {
        return 0;
    }

    let size_ptr = ptr.sub(CACHE_HEADER_SIZE) as *mut usize;
    let maybe_size = core::ptr::read(size_ptr);

    if maybe_size > 0 && maybe_size <= MAX_CACHE_SIZE {
        let potential_header = size_ptr as *mut PageHeader;
        if core::ptr::read(potential_header).magic != MAGIC {
            return maybe_size;
        }
    }

    let header = AethAlloc::page_header_from_ptr(ptr);
    let header_ref = core::ptr::read(header);

    if header_ref.magic == MAGIC {
        header_ref.requested_size
    } else {
        0
    }
}
