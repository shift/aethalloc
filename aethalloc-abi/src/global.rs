//! Global allocator implementation with thread-local caching
//!
//! Two modes available via feature flags:
//! - magazine-caching (default): Hoard-style magazines for cross-thread transfers
//! - simple-cache: Thread-local free-list per size class

use alloc::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, Ordering};

#[cfg(feature = "metrics")]
use aethalloc_amo::command::StatsReportPayload;
use aethalloc_amo::command::{FreeBlockPayload, RingCommand, RingEntry, RingPayload};
use aethalloc_amo::ring_buffer::RingBuffer;
use aethalloc_core::page::PageAllocator;
use aethalloc_core::size_class::round_up_pow2;

#[cfg(feature = "magazine-caching")]
use aethalloc_core::magazine::{GlobalMagazinePools, Magazine, MetadataAllocator};

#[cfg(feature = "metrics")]
use core::sync::atomic::AtomicU64;

/// AMO ring buffer capacity (power of 2)
const AMO_RING_CAPACITY: usize = 1024;

/// Static ring buffer for async metadata offloading
static AMO_RING: RingBuffer<AMO_RING_CAPACITY> = RingBuffer::new();

/// Track if support core thread has been spawned
static SUPPORT_CORE_STARTED: AtomicBool = AtomicBool::new(false);

/// Start the support core worker thread (called once)
pub fn ensure_support_core() {
    if !SUPPORT_CORE_STARTED.load(Ordering::Acquire) {
        SUPPORT_CORE_STARTED.store(true, Ordering::Release);
        use aethalloc_amo::support_core::spawn_support_core;
        unsafe {
            spawn_support_core(&AMO_RING);
        }
    }
}

/// Push a FreeBlock command to the AMO ring buffer
///
/// Only pushes when the ring buffer has room. Non-blocking - drops
/// entries if the buffer is full to avoid impacting the hot path.
/// This is intentional: AMO is best-effort telemetry, not a critical path.
#[inline]
unsafe fn amo_push_free_block(ptr: *mut u8, size: usize, size_class: u8) {
    let payload = RingPayload {
        free_block: FreeBlockPayload {
            ptr,
            size,
            size_class,
        },
    };
    let entry = RingEntry::new(RingCommand::FreeBlock, payload);
    // Non-blocking: if ring is full, skip. The support core will catch up.
    // This avoids stalling the dealloc hot path.
    let _ = AMO_RING.try_push(entry);
}

/// Push a batch of free blocks to the AMO ring buffer
///
/// Called when the thread-local cache flushes to global.
/// More efficient than individual pushes.
#[inline]
#[allow(dead_code)]
unsafe fn amo_push_free_batch(ptr: *mut u8, count: u32) {
    // Encode count in the size_class field (reuse FreeBlock command)
    let payload = RingPayload {
        free_block: FreeBlockPayload {
            ptr,
            size: 0,
            size_class: count as u8,
        },
    };
    let entry = RingEntry::new(RingCommand::FreeBlock, payload);
    let _ = AMO_RING.try_push(entry);
}

/// Push a StatsReport command to the AMO ring buffer
#[cfg(feature = "metrics")]
#[inline]
fn amo_push_stats(thread_id: u64, allocs: u64, frees: u64) {
    let payload = RingPayload {
        stats: StatsReportPayload {
            thread_id,
            allocs,
            frees,
        },
    };
    let entry = RingEntry::new(RingCommand::StatsReport, payload);
    let _ = AMO_RING.try_push(entry);
}

pub const PAGE_SIZE: usize = aethalloc_core::page::PAGE_SIZE;
const PAGE_MASK: usize = !(PAGE_SIZE - 1);
pub const MAX_CACHE_SIZE: usize = 65536;
const NUM_SIZE_CLASSES: usize = 14;
#[cfg(feature = "metrics")]
const METRICS_FLUSH_THRESHOLD: usize = 4096;
#[cfg(not(feature = "magazine-caching"))]
const MAX_FREE_LIST_LENGTH: usize = 4096;
#[cfg(not(feature = "magazine-caching"))]
const GLOBAL_FREE_BATCH: usize = 128;

pub const MAGIC: u32 = 0xA7E8A110;

#[repr(C)]
pub struct PageHeader {
    pub magic: u32,
    pub num_pages: u32,
    pub requested_size: usize,
    pub tag: aethalloc_core::Tag,
}

pub const PAGE_HEADER_SIZE: usize = core::mem::size_of::<PageHeader>();
pub const CACHE_HEADER_SIZE: usize = 16;
pub const LARGE_HEADER_SIZE: usize = 16;
pub const LARGE_MAGIC: u32 = 0xA7E8A11F;

#[repr(C)]
pub struct LargeAllocHeader {
    pub magic: u32,
    pub base_ptr: *mut u8,
}

#[cfg(not(feature = "magazine-caching"))]
struct GlobalFreeList {
    head: core::sync::atomic::AtomicPtr<u8>,
}

#[cfg(not(feature = "magazine-caching"))]
impl GlobalFreeList {
    const fn new() -> Self {
        Self {
            head: core::sync::atomic::AtomicPtr::new(core::ptr::null_mut()),
        }
    }

    #[inline]
    unsafe fn push_batch(&self, batch_head: *mut u8, batch_tail: *mut u8) {
        use core::sync::atomic::Ordering;
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
        use core::sync::atomic::Ordering;
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

#[cfg(feature = "metrics")]
pub static GLOBAL_METRICS: GlobalMetrics = GlobalMetrics::new();

#[cfg(feature = "magazine-caching")]
pub static GLOBAL_MAGAZINES: GlobalMagazinePools = GlobalMagazinePools::new();

#[cfg(feature = "magazine-caching")]
pub static METADATA_ALLOCATOR: MetadataAllocator = MetadataAllocator::new();

#[cfg(feature = "metrics")]
pub struct GlobalMetrics {
    pub allocs: AtomicU64,
    pub frees: AtomicU64,
    pub cache_hits: AtomicU64,
    pub cache_misses: AtomicU64,
    pub direct_allocs: AtomicU64,
}

#[cfg(feature = "metrics")]
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

#[cfg(feature = "metrics")]
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct MetricsSnapshot {
    pub allocs: u64,
    pub frees: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub direct_allocs: u64,
}

#[cfg(feature = "metrics")]
struct ThreadMetrics {
    allocs: usize,
    frees: usize,
    cache_hits: usize,
    cache_misses: usize,
    direct_allocs: usize,
}

#[cfg(not(feature = "metrics"))]
struct ThreadMetrics;

#[cfg(feature = "metrics")]
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
            let thread_id = unsafe { libc::pthread_self() as u64 };
            amo_push_stats(thread_id, self.allocs as u64, self.frees as u64);
            self.allocs = 0;
            self.frees = 0;
            self.cache_hits = 0;
            self.cache_misses = 0;
            self.direct_allocs = 0;
        }
    }

    #[inline]
    fn record_alloc(&mut self) {
        self.allocs += 1;
    }
    #[inline]
    fn record_free(&mut self) {
        self.frees += 1;
    }
    #[inline]
    fn record_cache_hit(&mut self) {
        self.cache_hits += 1;
    }
    #[inline]
    fn record_cache_miss(&mut self) {
        self.cache_misses += 1;
    }
    #[inline]
    fn record_direct_alloc(&mut self) {
        self.direct_allocs += 1;
    }
}

#[cfg(not(feature = "metrics"))]
impl ThreadMetrics {
    const fn new() -> Self {
        Self
    }
    #[inline]
    fn maybe_flush(&mut self) {}
    #[inline]
    fn record_alloc(&mut self) {}
    #[inline]
    fn record_free(&mut self) {}
    #[inline]
    fn record_cache_hit(&mut self) {}
    #[inline]
    fn record_cache_miss(&mut self) {}
    #[inline]
    fn record_direct_alloc(&mut self) {}
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
    pub fn align_up(addr: usize, align: usize) -> usize {
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
                    cache.metrics.record_cache_hit();
                    cache.metrics.record_alloc();
                    cache.metrics.maybe_flush();
                    core::ptr::write(head as *mut usize, size);
                    return head.add(CACHE_HEADER_SIZE);
                }
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
                        cache.metrics.record_cache_hit();
                        cache.metrics.record_alloc();
                        cache.metrics.maybe_flush();
                        core::ptr::write(block as *mut usize, size);
                        return block.add(CACHE_HEADER_SIZE);
                    }
                }
                cache.metrics.record_cache_miss();
                cache.metrics.record_alloc();
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
        cache.metrics.record_direct_alloc();
        cache.metrics.record_alloc();
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
                    tag: 0,
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
        let large_header_addr = ptr.sub(LARGE_HEADER_SIZE) as *const LargeAllocHeader;
        if core::ptr::read(large_header_addr).magic == LARGE_MAGIC {
            let base_ptr = core::ptr::read(large_header_addr).base_ptr;
            let page_header = core::ptr::read(base_ptr as *const PageHeader);
            if page_header.magic == MAGIC && page_header.num_pages > 0 {
                let size = page_header.num_pages as usize * PAGE_SIZE;
                let base_ptr_nn = NonNull::new_unchecked(base_ptr);
                use aethalloc_core::try_compact_region;
                let _compacted = try_compact_region(base_ptr_nn, size);
                PageAllocator::dealloc(base_ptr_nn, page_header.num_pages as usize);
            }
            let cache = get_thread_cache();
            cache.metrics.record_free();
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
                    cache.metrics.record_free();
                    cache.metrics.maybe_flush();
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
        cache.metrics.record_free();
        cache.metrics.maybe_flush();
        let alloc_size = get_alloc_size(ptr);
        let size_class = size_to_class(round_up_pow2(alloc_size).max(16)).unwrap_or(0) as u8;
        amo_push_free_block(ptr, alloc_size, size_class);
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
                if let Some(block) = cache.alloc_mags[class].pop() {
                    cache.metrics.record_cache_hit();
                    cache.metrics.record_alloc();
                    cache.metrics.maybe_flush();
                    core::ptr::write(block as *mut usize, size);
                    return block.add(CACHE_HEADER_SIZE);
                }
                if !cache.free_mags[class].is_empty() {
                    core::mem::swap(&mut cache.alloc_mags[class], &mut cache.free_mags[class]);
                    if let Some(block) = cache.alloc_mags[class].pop() {
                        cache.metrics.record_cache_hit();
                        cache.metrics.record_alloc();
                        cache.metrics.maybe_flush();
                        core::ptr::write(block as *mut usize, size);
                        return block.add(CACHE_HEADER_SIZE);
                    }
                }
                if let Some(node_ptr) = GLOBAL_MAGAZINES.get(class).pop_full() {
                    let node = &mut *node_ptr;
                    core::mem::swap(&mut cache.alloc_mags[class], &mut node.magazine);
                    node.magazine.clear();
                    GLOBAL_MAGAZINES.get(class).push_empty(node_ptr);
                    if let Some(block) = cache.alloc_mags[class].pop() {
                        cache.metrics.record_cache_hit();
                        cache.metrics.record_alloc();
                        cache.metrics.maybe_flush();
                        core::ptr::write(block as *mut usize, size);
                        return block.add(CACHE_HEADER_SIZE);
                    }
                }
                cache.metrics.record_cache_miss();
                cache.metrics.record_alloc();
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
        cache.metrics.record_direct_alloc();
        cache.metrics.record_alloc();
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
                    tag: 0,
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
        let large_header_addr = ptr.sub(LARGE_HEADER_SIZE) as *const LargeAllocHeader;
        if core::ptr::read(large_header_addr).magic == LARGE_MAGIC {
            let base_ptr = core::ptr::read(large_header_addr).base_ptr;
            let page_header = core::ptr::read(base_ptr as *const PageHeader);
            if page_header.magic == MAGIC && page_header.num_pages > 0 {
                let size = page_header.num_pages as usize * PAGE_SIZE;
                let base_ptr_nn = NonNull::new_unchecked(base_ptr);
                use aethalloc_core::try_compact_region;
                let _compacted = try_compact_region(base_ptr_nn, size);
                PageAllocator::dealloc(base_ptr_nn, page_header.num_pages as usize);
            }
            let cache = get_thread_cache();
            cache.metrics.record_free();
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
                    if cache.free_mags[class].push(block_ptr) {
                        cache.metrics.record_free();
                        cache.metrics.maybe_flush();
                        return;
                    }
                    let node = METADATA_ALLOCATOR.alloc_node();
                    if !node.is_null() {
                        (*node).magazine = core::mem::take(&mut cache.free_mags[class]);
                        (*node).next = core::ptr::null_mut();
                        GLOBAL_MAGAZINES.get(class).push_full(node);
                    }
                    let _ = cache.free_mags[class].push(block_ptr);
                    cache.metrics.record_free();
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
        cache.metrics.record_free();
        cache.metrics.maybe_flush();
        let alloc_size = get_alloc_size(ptr);
        let size_class = size_to_class(round_up_pow2(alloc_size).max(16)).unwrap_or(0) as u8;
        amo_push_free_block(ptr, alloc_size, size_class);
    }
}

pub unsafe fn get_alloc_size(ptr: *mut u8) -> usize {
    if ptr.is_null() {
        return 0;
    }
    let large_header_addr = ptr.sub(LARGE_HEADER_SIZE) as *const LargeAllocHeader;
    if core::ptr::read(large_header_addr).magic == LARGE_MAGIC {
        let base_ptr = core::ptr::read(large_header_addr).base_ptr;
        let page_header = core::ptr::read(base_ptr as *const PageHeader);
        if page_header.magic == MAGIC {
            return page_header.requested_size;
        }
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

#[cfg(feature = "metrics")]
#[no_mangle]
#[allow(improper_ctypes_definitions)]
pub extern "C" fn aethalloc_get_metrics() -> MetricsSnapshot {
    GLOBAL_METRICS.snapshot()
}

#[cfg(feature = "metrics")]
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
