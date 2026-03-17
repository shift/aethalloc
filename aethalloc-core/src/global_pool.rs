//! Global pool for slabs returned from thread-local caches
//!
//! Provides thread-safe global pools where thread-local caches can
//! return slabs when the local cache is full, and fetch batches when empty.

use core::ptr::NonNull;
use core::sync::atomic::{AtomicPtr, Ordering};

const NUM_SIZE_CLASSES: usize = 16;

/// Global slab pools
///
/// Each pool is a lock-free stack using atomic CAS.
/// Slabs are stored with their first 8 bytes used as a link pointer to the next slab in the stack.
pub struct GlobalPools {
    pools: [GlobalSlabPool; NUM_SIZE_CLASSES],
}

/// Lock-free stack for slab pointers
///
/// Uses atomic CAS for push and pop operations.
/// The first 8 bytes of each slab are used to store
/// the next pointer (or null for top of stack).
pub struct GlobalSlabPool {
    head: AtomicPtr<SlabLink>,
}

#[repr(C)]
struct SlabLink {
    next: *mut SlabLink,
}

impl GlobalSlabPool {
    pub const fn new() -> Self {
        Self {
            head: AtomicPtr::new(core::ptr::null_mut()),
        }
    }

    /// Push a slab to the global pool
    ///
    /// # Safety
    /// - ptr must point to valid memory with at least 8 bytes usable
    /// - ptr should not be currently in any other pool
    pub unsafe fn push(&self, ptr: NonNull<u8>) {
        let link = ptr.as_ptr() as *mut SlabLink;

        loop {
            let old_head = self.head.load(Ordering::Acquire);

            // SAFETY: We own this slot, and it has at least 8 bytes
            (*link).next = old_head;

            match self.head.compare_exchange_weak(
                old_head,
                link,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(_) => continue,
            }
        }
    }

    /// Pop a slab from the global pool
    ///
    /// Returns None if the pool is empty
    pub fn pop(&self) -> Option<NonNull<u8>> {
        loop {
            let head = self.head.load(Ordering::Acquire);

            if head.is_null() {
                return None;
            }

            // SAFETY: head was pushed to pool and has valid layout
            let next = unsafe { (*head).next };

            match self
                .head
                .compare_exchange_weak(head, next, Ordering::Release, Ordering::Relaxed)
            {
                Ok(_) => {
                    // SAFETY: head was in pool and has valid layout
                    return Some(unsafe { NonNull::new_unchecked(head as *mut u8) });
                }
                Err(_) => continue,
            }
        }
    }
}

impl Default for GlobalSlabPool {
    fn default() -> Self {
        Self::new()
    }
}

impl GlobalPools {
    pub const fn new() -> Self {
        Self {
            pools: [const { GlobalSlabPool::new() }; NUM_SIZE_CLASSES],
        }
    }

    /// Get the pool for a given size class index
    pub fn get_pool(&self, size_class_idx: usize) -> Option<&GlobalSlabPool> {
        self.pools.get(size_class_idx)
    }

    /// Push a slab to the appropriate pool
    ///
    /// # Safety
    /// - size_class_idx must be valid (< NUM_SIZE_CLASSES)
    /// - ptr must point to valid memory with at least 8 bytes usable
    /// - ptr should not be currently in any other pool
    pub unsafe fn push(&self, size_class_idx: usize, ptr: NonNull<u8>) {
        if let Some(pool) = self.get_pool(size_class_idx) {
            pool.push(ptr);
        }
    }

    /// Pop a slab from the appropriate pool
    pub fn pop(&self, size_class_idx: usize) -> Option<NonNull<u8>> {
        self.get_pool(size_class_idx).and_then(|pool| pool.pop())
    }
}

impl Default for GlobalPools {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl Sync for GlobalPools {}
unsafe impl Send for GlobalPools {}
