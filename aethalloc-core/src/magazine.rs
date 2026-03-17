//! Magazine caching for reduced lock contention
//!
//! Hoard-style magazine allocator that batches individual frees into
//! magazines (arrays of 64 pointers) before returning to global pool.
//! This amortizes atomic contention by 64x.

use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

pub const MAGAZINE_CAPACITY: usize = 64;
pub const NUM_SIZE_CLASSES: usize = 13;

#[repr(C)]
pub struct Magazine {
    pub blocks: [*mut u8; MAGAZINE_CAPACITY],
    pub count: usize,
}

impl Magazine {
    pub const fn new() -> Self {
        Self {
            blocks: [core::ptr::null_mut(); MAGAZINE_CAPACITY],
            count: 0,
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    #[inline]
    pub fn is_full(&self) -> bool {
        self.count >= MAGAZINE_CAPACITY
    }

    #[inline]
    pub fn push(&mut self, ptr: *mut u8) -> bool {
        if self.count >= MAGAZINE_CAPACITY {
            return false;
        }
        self.blocks[self.count] = ptr;
        self.count += 1;
        true
    }

    #[inline]
    pub fn pop(&mut self) -> Option<*mut u8> {
        if self.count == 0 {
            return None;
        }
        self.count -= 1;
        Some(self.blocks[self.count])
    }

    #[inline]
    pub fn clear(&mut self) {
        self.count = 0;
    }
}

impl Default for Magazine {
    fn default() -> Self {
        Self::new()
    }
}

#[repr(C)]
pub struct MagazineNode {
    pub magazine: Magazine,
    pub next: *mut MagazineNode,
}

pub struct GlobalMagazinePool {
    head: AtomicPtr<MagazineNode>,
    allocated: AtomicUsize,
}

impl GlobalMagazinePool {
    pub const fn new() -> Self {
        Self {
            head: AtomicPtr::new(core::ptr::null_mut()),
            allocated: AtomicUsize::new(0),
        }
    }

    pub fn push(&self, node: *mut MagazineNode) {
        loop {
            let old_head = self.head.load(Ordering::Acquire);
            unsafe {
                (*node).next = old_head;
            }
            match self.head.compare_exchange_weak(
                old_head,
                node,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(_) => continue,
            }
        }
    }

    pub fn pop(&self) -> Option<*mut MagazineNode> {
        loop {
            let head = self.head.load(Ordering::Acquire);
            if head.is_null() {
                return None;
            }
            let next = unsafe { (*head).next };
            match self
                .head
                .compare_exchange_weak(head, next, Ordering::Release, Ordering::Relaxed)
            {
                Ok(_) => return Some(head),
                Err(_) => continue,
            }
        }
    }

    pub fn allocated_count(&self) -> usize {
        self.allocated.load(Ordering::Relaxed)
    }
}

pub struct GlobalMagazinePools {
    pools: [GlobalMagazinePool; NUM_SIZE_CLASSES],
}

impl GlobalMagazinePools {
    pub const fn new() -> Self {
        Self {
            pools: [const { GlobalMagazinePool::new() }; NUM_SIZE_CLASSES],
        }
    }

    #[inline]
    pub fn get(&self, class: usize) -> &GlobalMagazinePool {
        &self.pools[class]
    }
}

unsafe impl Sync for GlobalMagazinePool {}
unsafe impl Send for GlobalMagazinePool {}
unsafe impl Sync for GlobalMagazinePools {}
unsafe impl Send for GlobalMagazinePools {}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::boxed::Box;

    #[test]
    fn test_magazine_push_pop() {
        let mut mag = Magazine::new();
        assert!(mag.is_empty());
        assert!(!mag.is_full());

        let ptr1 = 0x1000 as *mut u8;
        let ptr2 = 0x2000 as *mut u8;

        assert!(mag.push(ptr1));
        assert!(mag.push(ptr2));
        assert_eq!(mag.count, 2);

        assert_eq!(mag.pop(), Some(ptr2));
        assert_eq!(mag.pop(), Some(ptr1));
        assert!(mag.is_empty());
    }

    #[test]
    fn test_magazine_capacity() {
        let mut mag = Magazine::new();
        for i in 0..MAGAZINE_CAPACITY {
            assert!(mag.push((i + 1) as *mut u8));
        }
        assert!(mag.is_full());
        assert!(!mag.push(0xFFFF as *mut u8));
    }

    #[test]
    fn test_global_pool_push_pop() {
        let pool = GlobalMagazinePool::new();
        assert!(pool.pop().is_none());

        let node = Box::into_raw(Box::new(MagazineNode {
            magazine: Magazine::new(),
            next: core::ptr::null_mut(),
        }));

        pool.push(node);
        let popped = pool.pop();
        assert!(popped.is_some());
        assert_eq!(popped.unwrap(), node);

        unsafe {
            let _ = Box::from_raw(node);
        }
    }
}
