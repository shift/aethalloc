//! Global allocator implementation with thread-local caching
//!
//! Two modes available via feature flags:
//! - simple-cache (default): Thread-local free-list per size class
//! - magazine-caching: Hoard-style magazines for cross-thread transfers

use alloc::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;
use core::sync::atomic::{AtomicU64, Ordering};

use aethalloc_core::page::PageAllocator;
use aethalloc_core::size_class::round_up_pow2;

#[cfg(feature = "magazine-caching")]
use aethalloc_core::magazine::{GlobalMagazinePools, Magazine, MetadataAllocator};

const PAGE_SIZE: usize = aethalloc_core::page::PAGE_SIZE;
const PAGE_MASK: usize = !(PAGE_SIZE - 1);
const MAX_CACHE_SIZE: usize = 65536;
const NUM_SIZE_CLASSES: usize = 14;
const METRICS_FLUSH_THRESHOLD: usize = 4096;
#[cfg(not(feature = "magazine-caching"))]
const MAX_FREE_LIST_LENGTH: usize = 4096;
#[cfg(not(feature = "magazine-caching"))]
const GLOBAL_FREE_BATCH: usize = 128;

const MAGIC: u32 = 0xA7E8A110;

#[repr(C)]
struct PageHeader {
    magic: u32,
    num_pages: u32,
    requested_size: usize,
}

const PAGE_HEADER_SIZE: usize = core::mem::size_of::<PageHeader>();
const CACHE_HEADER_SIZE: usize = 16;
const LARGE_HEADER_SIZE: usize = 16;
const LARGE_MAGIC: u32 = 0xA7E8A11F;

#[repr(C)]
struct LargeAllocHeader {
    magic: u32,
    base_ptr: *mut u8,
}

#[cfg(not(feature = "magazine-caching"))]
struct GlobalFreeList {
    head: AtomicPtr<u8>,
}

#[cfg(not(feature = "magazine-caching"))]
impl GlobalFreeList {
    const fn new() -> Self {
        Self {
            head: AtomicPtr::new(core::ptr::null_mut()),
        }
    }

    #[inline]
    unsafe fn push_batch(&self, batch_head: *mut u8, batch_tail: *mut u8) {
        let mut current = self.head.load(Ordering::Relaxed);
        loop {
            core::ptr::write(batch_tail as *mut *mut u8, current);
            match self.head.compare_exchange_weak(
                current,
                batch_head,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(actual) => current = actual,
            }
        }
    }

    #[inline]
    unsafe fn pop(&self) -> Option<*mut u8> {
        let mut current = self.head.load(Ordering::Relaxed);
        loop {
            if current.is_null() {
                return None;
            }
            let next = core::ptr::read(current as *mut *mut u8);
            match self.head.compare_exchange_weak(
                current,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Some(current),
                Err(actual) => current = actual,
            }
        }
    }

    #[inline]
    unsafe fn pop_batch(
        &self,
        count: usize,
        heads: &mut [*mut u8; NUM_SIZE_CLASSES],
        counts: &mut [usize; NUM_SIZE_CLASSES],
        class: usize,
    ) -> usize {
        let mut transferred = 0;
        while transferred < count {
            match self.pop() {
                Some(block) => {
                    core::ptr::write(block as *mut *mut u8, heads[class]);
                    heads[class] = block;
                    counts[class] += 1;
                    transferred += 1;
                }
                None => break,
            }
        }
        transferred
    }
}

#[cfg(not(feature = "magazine-caching"))]
static GLOBAL_FREE_LISTS: [GlobalFreeList; NUM_SIZE_CLASSES] = [
    GlobalFreeList::new(),
    GlobalFreeList::new(),
    GlobalFreeList::new(),
    GlobalFreeList::new(),
    GlobalFreeList::new(),
    GlobalFreeList::new(),
    GlobalFreeList::new(),
    GlobalFreeList::new(),
    GlobalFreeList::new(),
    GlobalFreeList::new(),
    GlobalFreeList::new(),
    GlobalFreeList::new(),
    GlobalFreeList::new(),
    GlobalFreeList::new(),
];

pub static GLOBAL_METRICS: GlobalMetrics = GlobalMetrics::new();

#[cfg(feature = "magazine-caching")]
pub static GLOBAL_MAGAZINES: GlobalMagazinePools = GlobalMagazinePools::new();

#[cfg(feature = "magazine-caching")]
pub static METADATA_ALLOCATOR: MetadataAllocator = MetadataAllocator::new();

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

#[inline(always)]
fn size_to_class(size: usize) -> Option<usize> {
    let rounded = round_up_pow2(size).max(16);
    if rounded > 65536 {
        return None;
    }
    let tz = rounded.trailing_zeros() as usize;
    let class = tz.saturating_sub(4);
    if class < NUM_SIZE_CLASSES {
        Some(class)
    } else {
        None
    }
}

// ============================================================================
// SIMPLE-CACHE MODE: Thread-local free-list per size class
// ============================================================================

#[cfg(not(feature = "magazine-caching"))]
struct ThreadCache {
    heads: [*mut u8; NUM_SIZE_CLASSES],
    counts: [usize; NUM_SIZE_CLASSES],
    metrics: ThreadMetrics,
}

#[cfg(not(feature = "magazine-caching"))]
unsafe impl Send for ThreadCache {}

#[cfg(not(feature = "magazine-caching"))]
impl ThreadCache {
    const fn new() -> Self {
        Self {
            heads: [core::ptr::null_mut(); NUM_SIZE_CLASSES],
            counts: [0; NUM_SIZE_CLASSES],
            metrics: ThreadMetrics::new(),
        }
    }
}

#[cfg(not(feature = "magazine-caching"))]
#[thread_local]
static mut THREAD_CACHE: ThreadCache = ThreadCache::new();

#[cfg(not(feature = "magazine-caching"))]
#[inline(always)]
unsafe fn get_thread_cache() -> &'static mut ThreadCache {
    &mut *core::ptr::addr_of_mut!(THREAD_CACHE)
}

// ============================================================================
// MAGAZINE-CACHING MODE: Hoard-style magazines with global pool
// ============================================================================

#[cfg(feature = "magazine-caching")]
struct ThreadCache {
    alloc_mags: [Magazine; NUM_SIZE_CLASSES],
    free_mags: [Magazine; NUM_SIZE_CLASSES],
    metrics: ThreadMetrics,
}

#[cfg(feature = "magazine-caching")]
unsafe impl Send for ThreadCache {}

#[cfg(feature = "magazine-caching")]
impl ThreadCache {
    const fn new() -> Self {
        Self {
            alloc_mags: [const { Magazine::new() }; NUM_SIZE_CLASSES],
            free_mags: [const { Magazine::new() }; NUM_SIZE_CLASSES],
            metrics: ThreadMetrics::new(),
        }
    }
}

#[cfg(feature = "magazine-caching")]
#[thread_local]
static mut THREAD_CACHE: ThreadCache = ThreadCache::new();

#[cfg(feature = "magazine-caching")]
#[inline(always)]
unsafe fn get_thread_cache() -> &'static mut ThreadCache {
    &mut *core::ptr::addr_of_mut!(THREAD_CACHE)
}

// ============================================================================
// Common allocator struct
// ============================================================================

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

// ============================================================================
// SIMPLE-CACHE MODE: GlobalAlloc implementation
// ============================================================================

#[cfg(not(feature = "magazine-caching"))]
unsafe impl GlobalAlloc for AethAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align();

        if size == 0 {
            return core::ptr::null_mut();
        }

        if size <= MAX_CACHE_SIZE && align <= 8 {
            let cache = get_thread_cache();
            let cache_size = round_up_pow2(size).max(16);

            if let Some(class) = size_to_class(cache_size) {
                let head = cache.heads[class];

                if !head.is_null() {
                    let next = core::ptr::read(head as *mut *mut u8);
                    cache.heads[class] = next;
                    cache.counts[class] -= 1;
                    cache.metrics.cache_hits += 1;
                    cache.metrics.allocs += 1;
                    cache.metrics.maybe_flush();
                    core::ptr::write(head as *mut usize, size);
                    return head.add(CACHE_HEADER_SIZE);
                }

                // Try global free list before allocating new pages (only if non-empty)
                if !GLOBAL_FREE_LISTS[class]
                    .head
                    .load(Ordering::Relaxed)
                    .is_null()
                {
                    let transferred = GLOBAL_FREE_LISTS[class].pop_batch(
                        GLOBAL_FREE_BATCH,
                        &mut cache.heads,
                        &mut cache.counts,
                        class,
                    );
                    if transferred > 0 {
                        let block = cache.heads[class];
                        let next = core::ptr::read(block as *mut *mut u8);
                        cache.heads[class] = next;
                        cache.counts[class] -= 1;
                        cache.metrics.cache_hits += 1;
                        cache.metrics.allocs += 1;
                        cache.metrics.maybe_flush();
                        core::ptr::write(block as *mut usize, size);
                        return block.add(CACHE_HEADER_SIZE);
                    }
                }

                cache.metrics.cache_misses += 1;
                cache.metrics.allocs += 1;

                let block_size = cache_size + CACHE_HEADER_SIZE;
                let blocks_per_page = PAGE_SIZE / block_size;

                if blocks_per_page > 1 {
                    if let Some(base) = PageAllocator::alloc(1) {
                        let base_ptr = base.as_ptr();
                        for i in 1..blocks_per_page {
                            let block_ptr = base_ptr.add(i * block_size);
                            core::ptr::write(block_ptr as *mut *mut u8, cache.heads[class]);
                            cache.heads[class] = block_ptr;
                            cache.counts[class] += 1;
                        }
                        core::ptr::write(base_ptr as *mut usize, size);
                        cache.metrics.maybe_flush();
                        return base_ptr.add(CACHE_HEADER_SIZE);
                    }
                }

                let pages = block_size.div_ceil(PAGE_SIZE).max(1);
                if let Some(base) = PageAllocator::alloc(pages) {
                    let base_ptr = base.as_ptr();
                    core::ptr::write(base_ptr as *mut usize, size);
                    cache.metrics.maybe_flush();
                    return base_ptr.add(CACHE_HEADER_SIZE);
                }
                return core::ptr::null_mut();
            }
        }

        let cache = get_thread_cache();
        cache.metrics.direct_allocs += 1;
        cache.metrics.allocs += 1;
        cache.metrics.maybe_flush();

        let min_size = PAGE_HEADER_SIZE + LARGE_HEADER_SIZE + size + align;
        let pages = min_size.div_ceil(PAGE_SIZE).max(1);

        match PageAllocator::alloc(pages) {
            Some(base) => {
                let base_addr = base.as_ptr() as usize;

                let page_header = PageHeader {
                    magic: MAGIC,
                    num_pages: pages as u32,
                    requested_size: size,
                };
                let header_ptr = base.as_ptr() as *mut PageHeader;
                core::ptr::write(header_ptr, page_header);

                let user_addr =
                    Self::align_up(base_addr + PAGE_HEADER_SIZE + LARGE_HEADER_SIZE, align);

                let large_header = LargeAllocHeader {
                    magic: LARGE_MAGIC,
                    base_ptr: base.as_ptr(),
                };
                core::ptr::write(
                    (user_addr - LARGE_HEADER_SIZE) as *mut LargeAllocHeader,
                    large_header,
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

        // Check for large allocation first (LargeAllocHeader immediately before ptr)
        let large_header_addr = ptr.sub(LARGE_HEADER_SIZE) as *const LargeAllocHeader;
        if core::ptr::read(large_header_addr).magic == LARGE_MAGIC {
            let base_ptr = core::ptr::read(large_header_addr).base_ptr;
            let page_header = core::ptr::read(base_ptr as *const PageHeader);

            if page_header.magic == MAGIC && page_header.num_pages > 0 {
                PageAllocator::dealloc(
                    NonNull::new_unchecked(base_ptr),
                    page_header.num_pages as usize,
                );
            }

            let cache = get_thread_cache();
            cache.metrics.frees += 1;
            cache.metrics.maybe_flush();
            return;
        }

        let size_ptr = ptr.sub(CACHE_HEADER_SIZE) as *mut usize;
        let maybe_size = core::ptr::read(size_ptr);

        if maybe_size > 0 && maybe_size <= MAX_CACHE_SIZE {
            let potential_header = size_ptr as *mut PageHeader;
            if core::ptr::read(potential_header).magic != MAGIC {
                let cache = get_thread_cache();
                let cache_size = round_up_pow2(maybe_size).max(16);

                if let Some(class) = size_to_class(cache_size) {
                    let head_ptr = size_ptr as *mut *mut u8;
                    core::ptr::write(head_ptr, cache.heads[class]);
                    cache.heads[class] = size_ptr as *mut u8;
                    cache.counts[class] += 1;
                    cache.metrics.frees += 1;
                    cache.metrics.maybe_flush();

                    // Anti-hoarding: flush excess to global free list with O(1) batch push
                    if cache.counts[class] >= MAX_FREE_LIST_LENGTH {
                        let flush_count = cache.counts[class] / 2;

                        let batch_head = cache.heads[class];
                        let mut batch_tail = batch_head;
                        let mut walked = 1usize;

                        while walked < flush_count && !batch_tail.is_null() {
                            batch_tail = core::ptr::read(batch_tail as *mut *mut u8);
                            walked += 1;
                        }

                        if !batch_tail.is_null() {
                            let new_local_head = core::ptr::read(batch_tail as *mut *mut u8);
                            core::ptr::write(batch_tail as *mut *mut u8, core::ptr::null_mut());

                            cache.heads[class] = new_local_head;
                            cache.counts[class] -= flush_count;

                            GLOBAL_FREE_LISTS[class].push_batch(batch_head, batch_tail);
                        }
                    }
                    return;
                }
            }
        }

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

// ============================================================================
// MAGAZINE-CACHING MODE: GlobalAlloc implementation
// ============================================================================

#[cfg(feature = "magazine-caching")]
unsafe impl GlobalAlloc for AethAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align();

        if size == 0 {
            return core::ptr::null_mut();
        }

        if size <= MAX_CACHE_SIZE && align <= 8 {
            let cache = get_thread_cache();
            let cache_size = round_up_pow2(size).max(16);

            if let Some(class) = size_to_class(cache_size) {
                // Try local alloc magazine
                if let Some(block) = cache.alloc_mags[class].pop() {
                    cache.metrics.cache_hits += 1;
                    cache.metrics.allocs += 1;
                    cache.metrics.maybe_flush();
                    core::ptr::write(block as *mut usize, size);
                    return block.add(CACHE_HEADER_SIZE);
                }

                // Try swap with local free_mag for reuse
                if !cache.free_mags[class].is_empty() {
                    core::mem::swap(&mut cache.alloc_mags[class], &mut cache.free_mags[class]);
                    if let Some(block) = cache.alloc_mags[class].pop() {
                        cache.metrics.cache_hits += 1;
                        cache.metrics.allocs += 1;
                        cache.metrics.maybe_flush();
                        core::ptr::write(block as *mut usize, size);
                        return block.add(CACHE_HEADER_SIZE);
                    }
                }

                // Try to get a full magazine from global pool
                if let Some(node_ptr) = GLOBAL_MAGAZINES.get(class).pop_full() {
                    let node = &mut *node_ptr;
                    core::mem::swap(&mut cache.alloc_mags[class], &mut node.magazine);
                    node.magazine.clear();
                    unsafe {
                        GLOBAL_MAGAZINES.get(class).push_empty(node_ptr);
                    }

                    if let Some(block) = cache.alloc_mags[class].pop() {
                        cache.metrics.cache_hits += 1;
                        cache.metrics.allocs += 1;
                        cache.metrics.maybe_flush();
                        core::ptr::write(block as *mut usize, size);
                        return block.add(CACHE_HEADER_SIZE);
                    }
                }

                // Cache miss - allocate fresh blocks
                cache.metrics.cache_misses += 1;
                cache.metrics.allocs += 1;

                let block_size = cache_size + CACHE_HEADER_SIZE;
                let blocks_per_page = PAGE_SIZE / block_size;

                if blocks_per_page > 1 {
                    if let Some(base) = PageAllocator::alloc(1) {
                        let base_ptr = base.as_ptr();
                        let remaining = blocks_per_page.saturating_sub(1);
                        if remaining > 0 {
                            cache.alloc_mags[class].bulk_init(
                                base_ptr.add(block_size),
                                block_size,
                                remaining,
                            );
                        }
                        core::ptr::write(base_ptr as *mut usize, size);
                        cache.metrics.maybe_flush();
                        return base_ptr.add(CACHE_HEADER_SIZE);
                    }
                }

                let pages = block_size.div_ceil(PAGE_SIZE).max(1);
                if let Some(base) = PageAllocator::alloc(pages) {
                    let base_ptr = base.as_ptr();
                    core::ptr::write(base_ptr as *mut usize, size);
                    cache.metrics.maybe_flush();
                    return base_ptr.add(CACHE_HEADER_SIZE);
                }
                return core::ptr::null_mut();
            }
        }

        let cache = get_thread_cache();
        cache.metrics.direct_allocs += 1;
        cache.metrics.allocs += 1;
        cache.metrics.maybe_flush();

        // Large allocation with LargeAllocHeader (same as simple-cache mode)
        let min_size = PAGE_HEADER_SIZE + LARGE_HEADER_SIZE + size + align;
        let pages = min_size.div_ceil(PAGE_SIZE).max(1);

        match PageAllocator::alloc(pages) {
            Some(base) => {
                let base_addr = base.as_ptr() as usize;

                let page_header = PageHeader {
                    magic: MAGIC,
                    num_pages: pages as u32,
                    requested_size: size,
                };
                core::ptr::write(base.as_ptr() as *mut PageHeader, page_header);

                let user_addr =
                    Self::align_up(base_addr + PAGE_HEADER_SIZE + LARGE_HEADER_SIZE, align);

                let large_header = LargeAllocHeader {
                    magic: LARGE_MAGIC,
                    base_ptr: base.as_ptr(),
                };
                core::ptr::write(
                    (user_addr - LARGE_HEADER_SIZE) as *mut LargeAllocHeader,
                    large_header,
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

        // Check for large allocation first (LargeAllocHeader immediately before ptr)
        let large_header_addr = ptr.sub(LARGE_HEADER_SIZE) as *const LargeAllocHeader;
        if core::ptr::read(large_header_addr).magic == LARGE_MAGIC {
            let base_ptr = core::ptr::read(large_header_addr).base_ptr;
            let page_header = core::ptr::read(base_ptr as *const PageHeader);

            if page_header.magic == MAGIC && page_header.num_pages > 0 {
                PageAllocator::dealloc(
                    NonNull::new_unchecked(base_ptr),
                    page_header.num_pages as usize,
                );
            }

            let cache = get_thread_cache();
            cache.metrics.frees += 1;
            cache.metrics.maybe_flush();
            return;
        }

        let size_ptr = ptr.sub(CACHE_HEADER_SIZE) as *mut usize;
        let maybe_size = core::ptr::read(size_ptr);

        if maybe_size > 0 && maybe_size <= MAX_CACHE_SIZE {
            let potential_header = size_ptr as *mut PageHeader;
            if core::ptr::read(potential_header).magic != MAGIC {
                let cache = get_thread_cache();
                let cache_size = round_up_pow2(maybe_size).max(16);

                if let Some(class) = size_to_class(cache_size) {
                    let block_ptr = size_ptr as *mut u8;

                    // Try local free magazine
                    if cache.free_mags[class].push(block_ptr) {
                        cache.metrics.frees += 1;
                        cache.metrics.maybe_flush();
                        return;
                    }

                    // Magazine full - push to global pool using metadata allocator
                    let node = METADATA_ALLOCATOR.alloc_node();

                    if !node.is_null() {
                        (*node).magazine = core::mem::take(&mut cache.free_mags[class]);
                        (*node).next = core::ptr::null_mut();
                        unsafe {
                            GLOBAL_MAGAZINES.get(class).push_full(node);
                        }
                    }

                    // Push to now-empty magazine
                    let _ = cache.free_mags[class].push(block_ptr);
                    cache.metrics.frees += 1;
                    cache.metrics.maybe_flush();
                    return;
                }
            }
        }

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

    // Check for large allocation first (LargeAllocHeader immediately before ptr)
    let large_header_addr = ptr.sub(LARGE_HEADER_SIZE) as *const LargeAllocHeader;
    if core::ptr::read(large_header_addr).magic == LARGE_MAGIC {
        let base_ptr = core::ptr::read(large_header_addr).base_ptr;
        let page_header = core::ptr::read(base_ptr as *const PageHeader);
        if page_header.magic == MAGIC {
            return page_header.requested_size;
        }
        return 0;
    }

    // Check for small cached allocation
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

#[no_mangle]
#[allow(improper_ctypes_definitions)]
pub extern "C" fn aethalloc_get_metrics() -> MetricsSnapshot {
    GLOBAL_METRICS.snapshot()
}

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
