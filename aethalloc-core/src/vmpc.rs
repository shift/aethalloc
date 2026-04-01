//! VMPC integration - Virtual Memory Page Compaction
//!
//! Provides page compaction for memory defragmentation:
//! - Page table tracking via /proc/self/pagemap
//! - mremap-based page migration
//! - Compaction triggers on fragmentation detection

use core::ptr::NonNull;

#[cfg(feature = "vmpc")]
pub use aethalloc_vmpc::compactor::{CompactConfig, CompactResult, Compactor};
#[cfg(feature = "vmpc")]
pub use aethalloc_vmpc::page_table::{PageMapEntry, PageTableTracker, PageUtilization};

/// Default compaction configuration
#[cfg(feature = "vmpc")]
pub const fn default_compact_config() -> CompactConfig {
    CompactConfig {
        utilization_threshold: 0.5,
        min_pages_to_compact: 2,
        max_pages_per_pass: 256,
        strategy: aethalloc_vmpc::compactor::CompactStrategy::Auto,
    }
}

/// Try to compact a memory region if it appears fragmented
///
/// Returns true if compaction was attempted, false if skipped.
///
/// # Safety
/// - ptr must point to valid mapped memory
/// - size must be the total size of the region
#[inline]
#[cfg(feature = "vmpc")]
pub unsafe fn try_compact_region(ptr: NonNull<u8>, size: usize) -> bool {
    let page_size = aethalloc_vmpc::page_table::PAGE_SIZE;
    if size < page_size * 2 {
        return false;
    }

    let tracker = PageTableTracker::new();
    let mut sparse_count = 0usize;
    let mut total_pages = 0usize;

    let mut addr = ptr.as_ptr() as usize;
    let end = addr + size;
    while addr < end {
        if let Some(entry) = tracker.query_page(addr) {
            total_pages += 1;
            if !entry.is_present() || entry.is_swapped() {
                sparse_count += 1;
            }
        }
        addr += page_size;
    }

    if total_pages == 0 {
        return false;
    }

    let sparse_ratio = sparse_count as f32 / total_pages as f32;
    if sparse_ratio > 0.3 {
        let compactor = Compactor::new(default_compact_config());
        let _ = compactor.compact_pages(ptr, size);
        return true;
    }

    false
}

/// No-op fallback when VMPC feature is disabled
///
/// # Safety
/// This function is safe to call with any pointer - it does nothing.
#[inline]
#[cfg(not(feature = "vmpc"))]
pub unsafe fn try_compact_region(_ptr: NonNull<u8>, _size: usize) -> bool {
    false
}
