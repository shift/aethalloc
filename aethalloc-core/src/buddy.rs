//! Buddy allocator for medium allocations (16KB - 256KB)

use crate::page::{PageAllocator, PAGE_SIZE};
use core::ptr::NonNull;

/// Minimum order: 16KB = 2^14
pub const MIN_ORDER: usize = 14;
/// Maximum order: 256KB = 2^18
pub const MAX_ORDER: usize = 18;
/// Number of orders
pub const NUM_ORDERS: usize = MAX_ORDER - MIN_ORDER + 1;

/// Block header stored at the beginning of each buddy block
#[repr(C)]
struct BlockHeader {
    /// Order of this block (log2 of size)
    order: u8,
    /// Whether this block is free
    is_free: bool,
    /// Next free block in the free list
    next: Option<NonNull<BlockHeader>>,
}

/// Buddy allocator for a range of orders
pub struct BuddyAllocator {
    /// Free lists for each order
    free_lists: [Option<NonNull<BlockHeader>>; NUM_ORDERS],
    /// Base address of the managed region
    base: Option<NonNull<u8>>,
    /// Size of the managed region in bytes
    region_size: usize,
}

impl BuddyAllocator {
    /// Create a new buddy allocator
    pub fn new() -> Self {
        Self {
            free_lists: [None; NUM_ORDERS],
            base: None,
            region_size: 0,
        }
    }

    /// Initialize the allocator with a region of memory
    ///
    /// # Safety
    /// - ptr must point to a valid memory region of `size` bytes
    /// - size must be a power of 2 and at least 2^MAX_ORDER
    pub unsafe fn init(&mut self, ptr: NonNull<u8>, size: usize) {
        debug_assert!(size.is_power_of_two());
        debug_assert!(size >= (1usize << MAX_ORDER));

        self.base = Some(ptr);
        self.region_size = size;

        // Initialize the entire region as one free block at the highest order
        let max_order = size.trailing_zeros() as usize;
        let clamped_order = max_order.clamp(MIN_ORDER, MAX_ORDER);
        let order_idx = clamped_order - MIN_ORDER;

        // SAFETY: ptr is valid and we're initializing the header
        let header = ptr.as_ptr() as *mut BlockHeader;
        core::ptr::write(
            header,
            BlockHeader {
                order: clamped_order as u8,
                is_free: true,
                next: None,
            },
        );

        self.free_lists[order_idx] = Some(NonNull::new_unchecked(header));
    }

    /// Allocate `size` bytes (must be power of 2 in valid range)
    pub fn alloc(&mut self, size: usize) -> Option<NonNull<u8>> {
        let order = size.trailing_zeros() as usize;

        if !(MIN_ORDER..=MAX_ORDER).contains(&order) {
            return None;
        }

        let order_idx = order - MIN_ORDER;

        // Find a free block at this order or higher
        let mut found_order_idx = order_idx;
        while found_order_idx < NUM_ORDERS && self.free_lists[found_order_idx].is_none() {
            found_order_idx += 1;
        }

        if found_order_idx >= NUM_ORDERS {
            // No free blocks, allocate more memory
            return self.alloc_new_region(order);
        }

        // Remove block from free list
        // SAFETY: we verified the free list has a block
        let block = unsafe { self.free_lists[found_order_idx].unwrap_unchecked() };

        // SAFETY: block is a valid pointer to BlockHeader
        let header = unsafe { &mut *block.as_ptr() };
        self.free_lists[found_order_idx] = header.next;

        // Split down to the requested order
        if found_order_idx > order_idx {
            self.split(block, found_order_idx + MIN_ORDER, order);
        }

        // Mark as allocated
        // SAFETY: block is valid
        let header = unsafe { &mut *block.as_ptr() };
        header.is_free = false;
        header.next = None;

        // Return pointer after header
        let user_ptr = unsafe {
            NonNull::new_unchecked(
                block
                    .as_ptr()
                    .cast::<u8>()
                    .add(core::mem::size_of::<BlockHeader>()),
            )
        };
        Some(user_ptr)
    }

    /// Allocate a new region for the given order
    fn alloc_new_region(&mut self, order: usize) -> Option<NonNull<u8>> {
        let size = 1usize << MAX_ORDER;
        let pages = size / PAGE_SIZE;

        // SAFETY: allocating pages for buddy allocator
        let ptr = unsafe { PageAllocator::alloc(pages) }?;

        // SAFETY: we just allocated this memory
        unsafe { self.init(ptr, size) };

        // Now try allocation again
        self.alloc(1usize << order)
    }

    /// Free a block
    ///
    /// # Safety
    /// - ptr must have been allocated by this allocator
    /// - size must match the original allocation
    pub unsafe fn dealloc(&mut self, ptr: NonNull<u8>, size: usize) {
        let order = size.trailing_zeros() as usize;
        debug_assert!((MIN_ORDER..=MAX_ORDER).contains(&order));

        // SAFETY: ptr was returned from alloc(), which adds size_of::<BlockHeader>()
        // to a valid BlockHeader pointer. Subtracting that offset gives us back
        // the original valid BlockHeader pointer.
        let block = NonNull::new_unchecked(
            ptr.as_ptr().sub(core::mem::size_of::<BlockHeader>()) as *mut BlockHeader
        );

        // Mark as free
        let header = &mut *block.as_ptr();
        header.is_free = true;
        header.order = order as u8;

        // Try to coalesce with buddy
        self.coalesce(block, order);
    }

    /// Split a block from order `from` to order `to`
    fn split(&mut self, block: NonNull<BlockHeader>, from: usize, to: usize) {
        let mut current_order = from;

        while current_order > to {
            let current_idx = current_order - MIN_ORDER;
            let buddy_size = 1usize << (current_order - 1);

            // SAFETY: block is valid and we're computing buddy offset
            let buddy_ptr = unsafe {
                NonNull::new_unchecked(
                    (block.as_ptr() as *mut u8).add(buddy_size) as *mut BlockHeader
                )
            };

            // Initialize buddy as free
            // SAFETY: buddy_ptr is within the block's memory
            unsafe {
                core::ptr::write(
                    buddy_ptr.as_ptr(),
                    BlockHeader {
                        order: (current_order - 1) as u8,
                        is_free: true,
                        next: None,
                    },
                );
            }

            // Update current block's order
            // SAFETY: block is valid
            unsafe {
                (*block.as_ptr()).order = (current_order - 1) as u8;
            }

            // Add buddy to free list
            let buddy_idx = current_idx - 1;
            // SAFETY: buddy_ptr is valid
            unsafe {
                (*buddy_ptr.as_ptr()).next = self.free_lists[buddy_idx];
            }
            self.free_lists[buddy_idx] = Some(buddy_ptr);

            current_order -= 1;
        }
    }

    /// Coalesce buddy blocks
    fn coalesce(&mut self, block: NonNull<BlockHeader>, order: usize) {
        let mut current_block = block;
        let mut current_order = order;

        while current_order < MAX_ORDER {
            // Find buddy
            let block_addr = current_block.as_ptr() as usize;
            let buddy_addr = block_addr ^ (1usize << current_order);

            // Check if buddy is within our region and free
            if let Some(base) = self.base {
                if buddy_addr < base.as_ptr() as usize
                    || buddy_addr >= base.as_ptr() as usize + self.region_size
                {
                    break;
                }
            } else {
                break;
            }

            let buddy = buddy_addr as *mut BlockHeader;

            // SAFETY: buddy is within our region
            let buddy_header = unsafe { &*buddy };

            if !buddy_header.is_free || buddy_header.order as usize != current_order {
                break;
            }

            // Remove buddy from its free list
            let order_idx = current_order - MIN_ORDER;
            self.remove_from_free_list(buddy, order_idx);

            // Coalesce: use the lower address as the new block
            // SAFETY: both blocks are valid
            unsafe {
                let new_block = if block_addr < buddy_addr {
                    current_block
                } else {
                    NonNull::new_unchecked(buddy)
                };

                (*new_block.as_ptr()).order = (current_order + 1) as u8;
                current_block = new_block;
            }

            current_order += 1;
        }

        // Add final block to free list
        let order_idx = current_order - MIN_ORDER;
        // SAFETY: current_block is valid
        unsafe {
            (*current_block.as_ptr()).next = self.free_lists[order_idx];
        }
        self.free_lists[order_idx] = Some(current_block);
    }

    /// Remove a block from a free list
    fn remove_from_free_list(&mut self, block: *mut BlockHeader, order_idx: usize) {
        let mut current = self.free_lists[order_idx];
        let mut prev: Option<NonNull<BlockHeader>> = None;

        while let Some(curr) = current {
            if curr.as_ptr() == block {
                // SAFETY: curr is valid
                let next = unsafe { (*curr.as_ptr()).next };

                if let Some(p) = prev {
                    // SAFETY: p is valid
                    unsafe { (*p.as_ptr()).next = next };
                } else {
                    self.free_lists[order_idx] = next;
                }
                return;
            }

            // SAFETY: curr is valid
            prev = Some(curr);
            current = unsafe { (*curr.as_ptr()).next };
        }
    }
}

impl Default for BuddyAllocator {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for BuddyAllocator {
    fn drop(&mut self) {
        if let Some(base) = self.base {
            let pages = self.region_size / PAGE_SIZE;
            if pages > 0 {
                // SAFETY: base was allocated by PageAllocator with this size
                unsafe {
                    PageAllocator::dealloc(base, pages);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buddy_alloc_dealloc() {
        let mut buddy = BuddyAllocator::new();

        let ptr = buddy
            .alloc(16 * 1024)
            .expect("16KB allocation should succeed");

        // SAFETY: ptr was allocated with this size
        unsafe { buddy.dealloc(ptr, 16 * 1024) };
    }

    #[test]
    fn test_buddy_various_sizes() {
        let mut buddy = BuddyAllocator::new();

        for &size in &[16384, 32768, 65536, 131072, 262144] {
            let ptr = buddy.alloc(size).expect("allocation should succeed");
            // Just verify allocation succeeded
            let _ = ptr;

            // SAFETY: ptr was allocated with this size
            unsafe { buddy.dealloc(ptr, size) };
        }
    }

    #[test]
    fn test_buddy_split_and_coalesce() {
        let mut buddy = BuddyAllocator::new();

        // Allocate a large block
        let ptr1 = buddy
            .alloc(262144)
            .expect("256KB allocation should succeed");

        // Free it
        // SAFETY: ptr1 was allocated with 256KB
        unsafe { buddy.dealloc(ptr1, 262144) };

        // Now allocate smaller blocks - should reuse the coalesced space
        let ptr2 = buddy.alloc(16384).expect("16KB allocation should succeed");
        let ptr3 = buddy
            .alloc(16384)
            .expect("second 16KB allocation should succeed");

        assert_ne!(ptr2, ptr3);

        // SAFETY: ptrs were allocated with these sizes
        unsafe {
            buddy.dealloc(ptr2, 16384);
            buddy.dealloc(ptr3, 16384);
        }
    }

    #[test]
    fn test_buddy_invalid_sizes() {
        let mut buddy = BuddyAllocator::new();

        // Too small
        assert!(buddy.alloc(8192).is_none());

        // Too large
        assert!(buddy.alloc(524288).is_none());

        // Not power of 2 - but this actually rounds to a valid order
        // The allocator expects power of 2, so non-powers are undefined behavior
    }
}
