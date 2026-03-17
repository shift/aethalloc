//! Slab allocator for tiny/small allocations
//!
//! This allocator manages fixed-size slots across multiple pages. Free slots
//! are linked together using inline free lists stored within the free slots
//! themselves.

use crate::page::{PageAllocator, PAGE_SIZE};
use core::ptr::NonNull;

/// Number of size classes in slab (16B to 8KB, power of 2)
pub const SLAB_CLASSES: usize = 11;

/// Maximum number of pages a slab can grow to
const MAX_SLAB_PAGES: usize = 4;

/// Slab allocator for a single size class
///
/// Uses an inline free list where each free slot contains a pointer to the next
/// free slot. This avoids the multi-page indexing bug by storing absolute pointers.
pub struct Slab {
    /// Size of each slot
    slot_size: usize,
    /// Number of slots per slab page
    slots_per_page: usize,
    /// Free list head: absolute pointer to first free slot, or None
    /// Stored as raw usize to avoid lifetime issues
    free_head: Option<usize>,
    /// Number of allocated slots
    allocated: usize,
    /// All slab pages (for deallocation tracking)
    pages: [Option<NonNull<u8>>; MAX_SLAB_PAGES],
    /// Number of pages in use
    num_pages: usize,
    /// Next slot to allocate in current page (bump allocator within page)
    next_slot: usize,
}

impl Slab {
    /// Create a new slab for the given slot size
    pub fn new(slot_size: usize) -> Self {
        debug_assert!(slot_size >= 16, "slot_size must be at least 16 bytes");
        debug_assert!(slot_size.is_power_of_two(), "slot_size must be power of 2");

        let slots_per_page = PAGE_SIZE / slot_size;

        Self {
            slot_size,
            slots_per_page,
            free_head: None,
            allocated: 0,
            pages: [None; MAX_SLAB_PAGES],
            num_pages: 0,
            next_slot: 0,
        }
    }

    /// Allocate a slot from this slab
    pub fn alloc(&mut self) -> Option<NonNull<u8>> {
        // Try to allocate from free list first
        if let Some(free_ptr) = self.free_head {
            // SAFETY: free_ptr came from a previous alloc/dealloc cycle
            // and points to valid memory owned by this slab
            let next_free = unsafe {
                let slot_ptr = free_ptr as *const Option<usize>;
                core::ptr::read(slot_ptr)
            };

            self.free_head = next_free;
            self.allocated += 1;

            // SAFETY: free_ptr is a valid pointer to a slot we own
            return Some(unsafe { NonNull::new_unchecked(free_ptr as *mut u8) });
        }

        // No free slots, need a new page
        if self.num_pages == 0 {
            self.alloc_new_page()?;
        }

        // Check if current page is full
        if self.next_slot >= self.slots_per_page {
            // Need another page
            if self.num_pages >= MAX_SLAB_PAGES {
                return None; // Slab is full
            }
            self.alloc_new_page()?;
        }

        // Allocate a fresh slot from current page using bump allocation
        // SAFETY: current_page_idx is valid (we just checked/guaranteed it)
        let page = unsafe { self.pages[self.num_pages - 1].unwrap_unchecked() };
        let offset = self.next_slot * self.slot_size;
        self.next_slot += 1;
        self.allocated += 1;

        // SAFETY: offset is within page bounds (next_slot < slots_per_page)
        Some(unsafe { NonNull::new_unchecked(page.as_ptr().add(offset)) })
    }

    /// Allocate a new page for this slab
    fn alloc_new_page(&mut self) -> Option<()> {
        // SAFETY: allocating a single page
        let page = unsafe { PageAllocator::alloc(1) }?;

        self.pages[self.num_pages] = Some(page);
        self.num_pages += 1;
        self.next_slot = 0;

        Some(())
    }

    /// Return a slot to this slab
    ///
    /// # Safety
    /// - ptr must have been allocated from this slab
    /// - ptr must not be used after this call (until re-allocated)
    pub unsafe fn dealloc(&mut self, ptr: NonNull<u8>) {
        let ptr_addr = ptr.as_ptr() as usize;

        // Verify pointer belongs to one of our pages
        let mut found = false;
        for i in 0..self.num_pages {
            let page = self.pages[i].unwrap();
            let page_start = page.as_ptr() as usize;
            let page_end = page_start + PAGE_SIZE;

            if ptr_addr >= page_start && ptr_addr < page_end {
                let offset = ptr_addr - page_start;
                // Verify proper slot alignment
                debug_assert_eq!(offset % self.slot_size, 0, "pointer is not slot-aligned");
                found = true;
                break;
            }
        }

        if !found {
            debug_assert!(false, "deallocated pointer not from this slab");
            return;
        }

        // SAFETY: ptr is valid and we just verified it belongs to this slab
        // Store current free_head in this slot, then make this slot the new head
        let slot_ptr = ptr.as_ptr() as *mut Option<usize>;
        core::ptr::write(slot_ptr, self.free_head);

        self.free_head = Some(ptr_addr);
        self.allocated -= 1;
    }
}

impl Default for Slab {
    fn default() -> Self {
        Self::new(64) // Default to 64-byte slots
    }
}

impl Drop for Slab {
    fn drop(&mut self) {
        // SAFETY: all pages were allocated by PageAllocator
        unsafe {
            for i in 0..self.num_pages {
                if let Some(page) = self.pages[i] {
                    PageAllocator::dealloc(page, 1);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::vec::Vec;

    #[test]
    fn test_slab_alloc_dealloc() {
        let mut slab = Slab::new(64);

        let ptr1 = slab.alloc().expect("allocation should succeed");
        let ptr2 = slab.alloc().expect("allocation should succeed");

        assert_ne!(ptr1, ptr2);

        // SAFETY: pointers were allocated from this slab
        unsafe {
            slab.dealloc(ptr1);
            slab.dealloc(ptr2);
        }
    }

    #[test]
    fn test_slab_reuse_freed_slots() {
        let mut slab = Slab::new(64);

        let ptr1 = slab.alloc().expect("allocation should succeed");

        // SAFETY: ptr1 was allocated from this slab
        unsafe { slab.dealloc(ptr1) };

        let ptr2 = slab.alloc().expect("allocation should succeed");

        // Should reuse the freed slot
        assert_eq!(ptr1, ptr2);
    }

    #[test]
    fn test_slab_multiple_pages() {
        let mut slab = Slab::new(64);
        let slots_per_page = PAGE_SIZE / 64;

        let mut ptrs = Vec::new();

        // Allocate more than one page worth
        for _ in 0..(slots_per_page + 10) {
            let ptr = slab.alloc().expect("allocation should succeed");
            ptrs.push(ptr);
        }

        // SAFETY: all pointers were allocated from this slab
        unsafe {
            for ptr in ptrs {
                slab.dealloc(ptr);
            }
        }
    }

    #[test]
    fn test_slab_reuse_across_pages() {
        let mut slab = Slab::new(64);
        let slots_per_page = PAGE_SIZE / 64;

        let mut ptrs = Vec::new();

        // Allocate two pages worth
        for _ in 0..(slots_per_page * 2) {
            let ptr = slab.alloc().expect("allocation should succeed");
            ptrs.push(ptr);
        }

        // Free a slot from the first page (index 0)
        let first_page_ptr = ptrs[0];
        // SAFETY: pointer was allocated from this slab
        unsafe { slab.dealloc(first_page_ptr) };

        // Allocate again - should get the same pointer back
        let new_ptr = slab.alloc().expect("allocation should succeed");
        assert_eq!(
            first_page_ptr, new_ptr,
            "should reuse freed slot from first page"
        );

        // SAFETY: clean up remaining pointers
        unsafe {
            slab.dealloc(new_ptr);
            for ptr in ptrs.into_iter().skip(1) {
                slab.dealloc(ptr);
            }
        }
    }

    #[test]
    fn test_slab_various_sizes() {
        // Only test sizes that fit in a page (slab allocator doesn't handle >4KB)
        for &size in &[16, 32, 64, 128, 256, 512, 1024, 2048, 4096] {
            let mut slab = Slab::new(size);

            let ptr = slab.alloc().expect("allocation should succeed");
            assert_eq!(
                ptr.as_ptr() as usize % size,
                0,
                "should be aligned to slot_size"
            );

            // SAFETY: ptr was allocated from this slab
            unsafe { slab.dealloc(ptr) };
        }
    }
}
