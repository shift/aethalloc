//! Global allocator implementation with thread-local caching
//!
//! Uses page-level metadata for large allocations and thread-local
//! free lists for small allocations. Metrics are thread-local to avoid
//! MESI cache line bouncing.

use alloc::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;
use core::sync::atomic::{AtomicU64, Ordering};

use aethalloc_core::page::PageAllocator;
use aethalloc_core::size_class::round_up_pow2;

const PAGE_SIZE: usize = aethalloc_core::page::PAGE_SIZE;
const PAGE_MASK: usize = !(PAGE_SIZE - 1);
const MAX_CACHE_SIZE: usize = 65536;
const NUM_SIZE_CLASSES: usize = 14;
const METRICS_FLUSH_THRESHOLD: usize = 4096;

const MAGIC: u32 = 0xA7E8A110;

#[repr(C)]
struct PageHeader {
    magic: u32,
    num_pages: u16,
    requested_size: usize,
}

const PAGE_HEADER_SIZE: usize = core::mem::size_of::<PageHeader>();
const CACHE_HEADER_SIZE: usize = core::mem::size_of::<usize>();

/// Global aggregated metrics (updated periodically from thread-locals)
pub static GLOBAL_METRICS: GlobalMetrics = GlobalMetrics::new();

pub struct GlobalMetrics {
    pub allocs: AtomicU64,
    pub frees: AtomicU64,
    pub cache_hits: AtomicU64,
    pub cache_misses: AtomicU64,
    pub direct_allocs: AtomicU64,
}

impl GlobalMetrics {
    const fn new() -> Self {
        Self {
            allocs: AtomicU64::new(0),
            frees: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
            direct_allocs: AtomicU64::new(0),
        }
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            allocs: self.allocs.load(Ordering::Relaxed),
            frees: self.frees.load(Ordering::Relaxed),
            cache_hits: self.cache_hits.load(Ordering::Relaxed),
            cache_misses: self.cache_misses.load(Ordering::Relaxed),
            direct_allocs: self.direct_allocs.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
#[allow(dead_code)]
pub struct MetricsSnapshot {
    pub allocs: u64,
    pub frees: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub direct_allocs: u64,
}

/// Thread-local metrics (zero contention - plain usize, no atomics)
struct ThreadMetrics {
    allocs: usize,
    frees: usize,
    cache_hits: usize,
    cache_misses: usize,
    direct_allocs: usize,
}

impl ThreadMetrics {
    const fn new() -> Self {
        Self {
            allocs: 0,
            frees: 0,
            cache_hits: 0,
            cache_misses: 0,
            direct_allocs: 0,
        }
    }

    #[inline]
    fn maybe_flush(&mut self) {
        if self.allocs + self.frees >= METRICS_FLUSH_THRESHOLD {
            GLOBAL_METRICS
                .allocs
                .fetch_add(self.allocs as u64, Ordering::Relaxed);
            GLOBAL_METRICS
                .frees
                .fetch_add(self.frees as u64, Ordering::Relaxed);
            GLOBAL_METRICS
                .cache_hits
                .fetch_add(self.cache_hits as u64, Ordering::Relaxed);
            GLOBAL_METRICS
                .cache_misses
                .fetch_add(self.cache_misses as u64, Ordering::Relaxed);
            GLOBAL_METRICS
                .direct_allocs
                .fetch_add(self.direct_allocs as u64, Ordering::Relaxed);
            self.allocs = 0;
            self.frees = 0;
            self.cache_hits = 0;
            self.cache_misses = 0;
            self.direct_allocs = 0;
        }
    }
}

/// Thread-local cache of free blocks per size class
struct ThreadCache {
    heads: [*mut u8; NUM_SIZE_CLASSES],
    counts: [usize; NUM_SIZE_CLASSES],
    metrics: ThreadMetrics,
}

unsafe impl Send for ThreadCache {}

impl ThreadCache {
    const fn new() -> Self {
        Self {
            heads: [core::ptr::null_mut(); NUM_SIZE_CLASSES],
            counts: [0; NUM_SIZE_CLASSES],
            metrics: ThreadMetrics::new(),
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
            16384 => Some(10),
            32768 => Some(11),
            65536 => Some(12),
            _ => None,
        }
    }
}

/// Thread-local storage
#[thread_local]
static mut THREAD_CACHE: ThreadCache = ThreadCache::new();

#[inline(always)]
unsafe fn get_thread_cache() -> &'static mut ThreadCache {
    &mut *core::ptr::addr_of_mut!(THREAD_CACHE)
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
                    // Pop from free list (cache hit)
                    let next = core::ptr::read(head as *mut *mut u8);
                    cache.heads[class] = next;
                    cache.counts[class] -= 1;
                    cache.metrics.cache_hits += 1;
                    cache.metrics.allocs += 1;
                    cache.metrics.maybe_flush();

                    // Store size and return
                    core::ptr::write(head as *mut usize, size);
                    return head.add(CACHE_HEADER_SIZE);
                }

                // Cache miss - need fresh allocation
                cache.metrics.cache_misses += 1;
                cache.metrics.allocs += 1;

                // Batch-allocate: carve one page into multiple blocks
                let block_size = cache_size + CACHE_HEADER_SIZE;
                let blocks_per_page = PAGE_SIZE / block_size;

                if blocks_per_page > 1 {
                    if let Some(base) = PageAllocator::alloc(1) {
                        let base_ptr = base.as_ptr();

                        // Add all blocks except first to free list
                        for i in 1..blocks_per_page {
                            let block_ptr = base_ptr.add(i * block_size);
                            core::ptr::write(block_ptr as *mut *mut u8, cache.heads[class]);
                            cache.heads[class] = block_ptr;
                            cache.counts[class] += 1;
                        }

                        // Return first block
                        core::ptr::write(base_ptr as *mut usize, size);
                        cache.metrics.maybe_flush();
                        return base_ptr.add(CACHE_HEADER_SIZE);
                    }
                }

                // Fallback for large blocks that don't fit well in a page
                let pages = block_size.div_ceil(PAGE_SIZE).max(1);
                if let Some(base) = PageAllocator::alloc(pages) {
                    let size_ptr = base.as_ptr() as *mut usize;
                    core::ptr::write(size_ptr, size);
                    cache.metrics.maybe_flush();
                    return size_ptr.add(1) as *mut u8;
                }
                return core::ptr::null_mut();
            }
        }

        // Direct (large) allocation
        let cache = get_thread_cache();
        cache.metrics.direct_allocs += 1;
        cache.metrics.allocs += 1;
        cache.metrics.maybe_flush();

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

                if let Some(_class) = ThreadCache::size_to_class(cache_size) {
                    let head_ptr = size_ptr as *mut *mut u8;
                    core::ptr::write(head_ptr, cache.heads[_class]);
                    cache.heads[_class] = size_ptr as *mut u8;
                    cache.counts[_class] += 1;
                    cache.metrics.frees += 1;
                    cache.metrics.maybe_flush();
                    return;
                }
            }
        }

        // Large allocation - find page header and free
        let header = Self::page_header_from_ptr(ptr);
        let header_ref = core::ptr::read(header);

        if header_ref.magic == MAGIC && header_ref.num_pages > 0 {
            let base = NonNull::new_unchecked(header as *mut u8);
            PageAllocator::dealloc(base, header_ref.num_pages as usize);
        }

        let cache = get_thread_cache();
        cache.metrics.frees += 1;
        cache.metrics.maybe_flush();
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

/// Flush current thread's metrics to global counters
#[no_mangle]
#[allow(improper_ctypes_definitions)]
pub extern "C" fn aethalloc_get_metrics() -> MetricsSnapshot {
    GLOBAL_METRICS.snapshot()
}

/// Force flush current thread's pending metrics
#[allow(dead_code)]
pub unsafe fn flush_thread_metrics() {
    let cache = get_thread_cache();
    GLOBAL_METRICS
        .allocs
        .fetch_add(cache.metrics.allocs as u64, Ordering::Relaxed);
    GLOBAL_METRICS
        .frees
        .fetch_add(cache.metrics.frees as u64, Ordering::Relaxed);
    GLOBAL_METRICS
        .cache_hits
        .fetch_add(cache.metrics.cache_hits as u64, Ordering::Relaxed);
    GLOBAL_METRICS
        .cache_misses
        .fetch_add(cache.metrics.cache_misses as u64, Ordering::Relaxed);
    GLOBAL_METRICS
        .direct_allocs
        .fetch_add(cache.metrics.direct_allocs as u64, Ordering::Relaxed);
    cache.metrics.allocs = 0;
    cache.metrics.frees = 0;
    cache.metrics.cache_hits = 0;
    cache.metrics.cache_misses = 0;
    cache.metrics.direct_allocs = 0;
}
