//! Thread-local allocation cache

use crate::page::{PageAllocator, PAGE_SIZE};
use crate::size_class::{round_up_pow2, SizeClass};
use core::cell::UnsafeCell;
use core::ptr::NonNull;

/// Number of tiny size classes (16B, 32B, 64B, 128B = 4 classes)
const TINY_CLASSES: usize = 4;
/// Number of small size classes (256B, 512B, 1KB, 2KB, 4KB, 8KB = 6 classes)  
const SMALL_CLASSES: usize = 6;

/// Maximum number of cached pointers per size class
const CACHE_SIZE: usize = 32;

/// A cache slot containing either a free list head or None
#[repr(align(8))]
struct CacheSlot {
    /// Free list head pointer
    head: UnsafeCell<Option<NonNull<u8>>>,
    /// Number of cached items
    count: UnsafeCell<usize>,
}

/// Thread-local cache for small allocations
/// Each thread gets its own cache, no synchronization needed
pub struct ThreadLocalCache {
    /// Cache slots for tiny sizes: 16B, 32B, 64B, 128B
    tiny: [CacheSlot; TINY_CLASSES],
    /// Cache slots for small sizes: 256B, 512B, 1KB, 2KB, 4KB, 8KB
    small: [CacheSlot; SMALL_CLASSES],
}

impl CacheSlot {
    const fn new() -> Self {
        Self {
            head: UnsafeCell::new(None),
            count: UnsafeCell::new(0),
        }
    }

    fn size_for_index(is_tiny: bool, idx: usize) -> usize {
        if is_tiny {
            16usize << idx // 16, 32, 64, 128
        } else {
            256usize << idx // 256, 512, 1024, 2048, 4096, 8192
        }
    }
}

impl Default for ThreadLocalCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ThreadLocalCache {
    /// Create a new thread-local cache
    pub const fn new() -> Self {
        Self {
            tiny: [
                CacheSlot::new(),
                CacheSlot::new(),
                CacheSlot::new(),
                CacheSlot::new(),
            ],
            small: [
                CacheSlot::new(),
                CacheSlot::new(),
                CacheSlot::new(),
                CacheSlot::new(),
                CacheSlot::new(),
                CacheSlot::new(),
            ],
        }
    }

    /// Get the cache slot for a given size
    fn get_slot(&self, size: usize) -> Option<(&CacheSlot, bool, usize)> {
        let size_class = SizeClass::classify(size);

        match size_class {
            SizeClass::Tiny => {
                let alloc_size = round_up_pow2(size).max(16);
                if alloc_size > 128 {
                    return None;
                }
                let idx = (alloc_size / 16).trailing_zeros() as usize;
                if idx < TINY_CLASSES {
                    Some((&self.tiny[idx], true, idx))
                } else {
                    None
                }
            }
            SizeClass::Small => {
                let alloc_size = round_up_pow2(size);
                if !(256..=8192).contains(&alloc_size) {
                    return None;
                }
                let idx = (alloc_size / 256).trailing_zeros() as usize;
                if idx < SMALL_CLASSES {
                    Some((&self.small[idx], false, idx))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Allocate from thread-local cache
    pub fn alloc(&self, size: usize) -> Option<NonNull<u8>> {
        let (slot, is_tiny, idx) = self.get_slot(size)?;

        // SAFETY: This is thread-local, only one thread accesses this
        let head = unsafe { &mut *slot.head.get() };
        let count = unsafe { &mut *slot.count.get() };

        if let Some(ptr) = *head {
            // Read next pointer from the cached block
            // SAFETY: ptr points to a cached block that was previously allocated
            // and freed back to this cache. The first usize bytes store the next pointer.
            let next = unsafe { core::ptr::read(ptr.as_ptr() as *const Option<NonNull<u8>>) };

            *head = next;
            *count -= 1;

            Some(ptr)
        } else {
            // Cache empty, allocate a new block from the page allocator
            let alloc_size = CacheSlot::size_for_index(is_tiny, idx);

            // SAFETY: allocating memory from the system
            unsafe {
                let pages = alloc_size.div_ceil(PAGE_SIZE);
                let page_ptr = PageAllocator::alloc(pages.max(1))?;
                Some(page_ptr)
            }
        }
    }

    /// Return to thread-local cache
    ///
    /// # Safety
    /// - ptr must have been allocated from this cache
    /// - size must match the original allocation
    pub unsafe fn dealloc(&self, ptr: NonNull<u8>, size: usize) {
        let (slot, _, _) = match self.get_slot(size) {
            Some(s) => s,
            None => {
                // Size doesn't fit in cache, free directly
                let alloc_size = round_up_pow2(size);
                let pages = alloc_size.div_ceil(PAGE_SIZE);
                if pages > 0 {
                    PageAllocator::dealloc(ptr, pages);
                }
                return;
            }
        };

        // SAFETY: This is thread-local, only one thread accesses this
        let head = &mut *slot.head.get();
        let count = &mut *slot.count.get();

        if *count >= CACHE_SIZE {
            // Cache full, free directly
            let alloc_size = round_up_pow2(size);
            let pages = alloc_size.div_ceil(PAGE_SIZE);
            if pages > 0 {
                PageAllocator::dealloc(ptr, pages);
            }
            return;
        }

        // Store current head in this block and make this block the new head
        // SAFETY: ptr is valid and we're using the first usize bytes for the next pointer
        core::ptr::write(ptr.as_ptr() as *mut Option<NonNull<u8>>, *head);
        *head = Some(ptr);
        *count += 1;
    }

    /// Clear all cached allocations
    ///
    /// # Safety
    /// - Must only be called when no cached allocations are in use
    pub unsafe fn clear(&self) {
        for i in 0..TINY_CLASSES {
            self.clear_slot(&self.tiny[i], true, i);
        }
        for i in 0..SMALL_CLASSES {
            self.clear_slot(&self.small[i], false, i);
        }
    }

    unsafe fn clear_slot(&self, slot: &CacheSlot, is_tiny: bool, idx: usize) {
        let head = &mut *slot.head.get();
        let count = &mut *slot.count.get();

        let mut current = *head;
        while let Some(ptr) = current {
            let next = core::ptr::read(ptr.as_ptr() as *const Option<NonNull<u8>>);

            let alloc_size = CacheSlot::size_for_index(is_tiny, idx);
            let pages = alloc_size.div_ceil(PAGE_SIZE);
            if pages > 0 {
                PageAllocator::dealloc(ptr, pages);
            }

            current = next;
        }

        *head = None;
        *count = 0;
    }
}

impl Drop for ThreadLocalCache {
    fn drop(&mut self) {
        // SAFETY: Cache is being dropped, no allocations can be in use
        unsafe { self.clear() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    #[test]
    fn test_cache_alloc_dealloc() {
        let cache = ThreadLocalCache::new();

        let ptr = cache.alloc(64).expect("allocation should succeed");

        // SAFETY: ptr was allocated with size 64
        unsafe { cache.dealloc(ptr, 64) };
    }

    #[test]
    fn test_cache_reuse() {
        let cache = ThreadLocalCache::new();

        let ptr1 = cache.alloc(64).expect("allocation should succeed");

        // SAFETY: ptr1 was allocated with size 64
        unsafe { cache.dealloc(ptr1, 64) };

        let ptr2 = cache.alloc(64).expect("allocation should succeed");

        // Should reuse the cached pointer
        assert_eq!(ptr1, ptr2);

        // SAFETY: ptr2 was allocated with size 64
        unsafe { cache.dealloc(ptr2, 64) };
    }

    #[test]
    fn test_cache_tiny_sizes() {
        let cache = ThreadLocalCache::new();

        for &size in &[16, 32, 64, 128] {
            let ptr = cache.alloc(size).expect("allocation should succeed");
            assert_eq!(ptr.as_ptr() as usize % size, 0, "should be aligned to size");

            // SAFETY: ptr was allocated with this size
            unsafe { cache.dealloc(ptr, size) };
        }
    }

    #[test]
    fn test_cache_small_sizes() {
        let cache = ThreadLocalCache::new();

        for &size in &[256, 512, 1024, 2048, 4096, 8192] {
            let ptr = cache.alloc(size).expect("allocation should succeed");
            // Only guarantee page alignment (4KB), not size alignment for larger sizes
            assert_eq!(
                ptr.as_ptr() as usize % PAGE_SIZE,
                0,
                "should be page-aligned"
            );

            // SAFETY: ptr was allocated with this size
            unsafe { cache.dealloc(ptr, size) };
        }
    }

    #[test]
    fn test_cache_rejects_large_sizes() {
        let cache = ThreadLocalCache::new();

        // Medium and large sizes should return None
        assert!(cache.alloc(16384).is_none());
        assert!(cache.alloc(262145).is_none());
    }

    #[test]
    fn test_cache_full_frees_directly() {
        let cache = ThreadLocalCache::new();

        let mut ptrs = Vec::new();

        // Fill the cache beyond its capacity
        for _ in 0..CACHE_SIZE + 5 {
            let ptr = cache.alloc(64).expect("allocation should succeed");
            ptrs.push(ptr);
        }

        // SAFETY: all ptrs were allocated with size 64
        unsafe {
            for ptr in ptrs {
                cache.dealloc(ptr, 64);
            }
        }
    }
}
