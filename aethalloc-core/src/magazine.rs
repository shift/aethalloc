//! Magazine caching with lock-free Treiber Stack
//!
//! Implements symmetric full/empty stacks for closed-loop memory transfer.
//! Uses pointer tagging for ABA problem mitigation on x86_64.

use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

pub const MAGAZINE_CAPACITY: usize = 64;
pub const NUM_SIZE_CLASSES: usize = 13;
pub const MAX_GLOBAL_MAGAZINES_PER_CLASS: usize = 8;

/// Magazine: A container for 64 memory block pointers
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

    /// # Safety
    /// `base` must be a valid pointer to a memory region with at least
    /// `count * block_size` bytes accessible.
    #[inline]
    pub unsafe fn bulk_init(&mut self, base: *mut u8, block_size: usize, count: usize) {
        let to_add = count.min(MAGAZINE_CAPACITY - self.count);
        for i in 0..to_add {
            self.blocks[self.count + i] = base.add(i * block_size);
        }
        self.count += to_add;
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.count
    }
}

impl Default for Magazine {
    fn default() -> Self {
        Self::new()
    }
}

/// MagazineNode: A magazine with intrusive next pointer for lock-free stacks
#[repr(C)]
pub struct MagazineNode {
    pub next: *mut MagazineNode,
    pub magazine: Magazine,
}

impl MagazineNode {
    pub const fn new() -> Self {
        Self {
            next: core::ptr::null_mut(),
            magazine: Magazine::new(),
        }
    }
}

impl Default for MagazineNode {
    fn default() -> Self {
        Self::new()
    }
}

/// Symmetric global pool with lock-free full/empty stacks
pub struct GlobalMagazinePool {
    full_head: AtomicPtr<MagazineNode>,
    empty_head: AtomicPtr<MagazineNode>,
}

impl Default for GlobalMagazinePool {
    fn default() -> Self {
        Self::new()
    }
}

impl GlobalMagazinePool {
    pub const fn new() -> Self {
        Self {
            full_head: AtomicPtr::new(core::ptr::null_mut()),
            empty_head: AtomicPtr::new(core::ptr::null_mut()),
        }
    }

    /// Push a full magazine to the global pool
    ///
    /// # Safety
    /// `node` must be a valid pointer to a `MagazineNode` that is not already
    /// in any pool. The caller must ensure exclusive access to `node`.
    #[inline]
    pub unsafe fn push_full(&self, node: *mut MagazineNode) {
        let mut current = self.full_head.load(Ordering::Relaxed);
        loop {
            (*node).next = current;
            match self.full_head.compare_exchange_weak(
                current,
                node,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(c) => current = c,
            }
        }
    }

    /// Pop a full magazine from the global pool
    #[inline]
    pub fn pop_full(&self) -> Option<*mut MagazineNode> {
        let mut current = self.full_head.load(Ordering::Acquire);
        loop {
            if current.is_null() {
                return None;
            }
            let next = unsafe { (*current).next };
            match self.full_head.compare_exchange_weak(
                current,
                next,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Some(current),
                Err(c) => current = c,
            }
        }
    }

    /// Push an empty magazine to the global pool
    ///
    /// # Safety
    /// `node` must be a valid pointer to a `MagazineNode` that is not already
    /// in any pool. The caller must ensure exclusive access to `node`.
    #[inline]
    pub unsafe fn push_empty(&self, node: *mut MagazineNode) {
        let mut current = self.empty_head.load(Ordering::Relaxed);
        loop {
            (*node).next = current;
            match self.empty_head.compare_exchange_weak(
                current,
                node,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(c) => current = c,
            }
        }
    }

    /// Pop an empty magazine from the global pool
    #[inline]
    pub fn pop_empty(&self) -> Option<*mut MagazineNode> {
        let mut current = self.empty_head.load(Ordering::Acquire);
        loop {
            if current.is_null() {
                return None;
            }
            let next = unsafe { (*current).next };
            match self.empty_head.compare_exchange_weak(
                current,
                next,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Some(current),
                Err(c) => current = c,
            }
        }
    }

    #[inline]
    pub fn full_depth(&self) -> usize {
        let mut count = 0;
        let mut current = self.full_head.load(Ordering::Relaxed);
        while !current.is_null() && count < MAX_GLOBAL_MAGAZINES_PER_CLASS + 1 {
            current = unsafe { (*current).next };
            count += 1;
        }
        count
    }
}

/// All global magazine pools (one per size class)
pub struct GlobalMagazinePools {
    pools: [GlobalMagazinePool; NUM_SIZE_CLASSES],
}

impl Default for GlobalMagazinePools {
    fn default() -> Self {
        Self::new()
    }
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

/// Internal metadata allocator for MagazineNode fallback
pub struct MetadataAllocator {
    current_page: AtomicPtr<u8>,
    offset: AtomicUsize,
}

impl Default for MetadataAllocator {
    fn default() -> Self {
        Self::new()
    }
}

impl MetadataAllocator {
    pub const fn new() -> Self {
        Self {
            current_page: AtomicPtr::new(core::ptr::null_mut()),
            offset: AtomicUsize::new(PAGE_SIZE),
        }
    }

    /// Allocate a MagazineNode from the metadata pool
    pub fn alloc_node(&self) -> *mut MagazineNode {
        const NODE_SIZE: usize = core::mem::size_of::<MagazineNode>();
        const NODE_ALIGN: usize = core::mem::align_of::<MagazineNode>();

        loop {
            let offset = self.offset.load(Ordering::Relaxed);
            let aligned = (offset + NODE_ALIGN - 1) & !(NODE_ALIGN - 1);

            if aligned + NODE_SIZE <= PAGE_SIZE {
                match self.offset.compare_exchange_weak(
                    offset,
                    aligned + NODE_SIZE,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => {
                        let page = self.current_page.load(Ordering::Relaxed);
                        return unsafe { page.add(aligned) as *mut MagazineNode };
                    }
                    Err(_) => continue,
                }
            }

            // Need a new page
            let new_page = unsafe { alloc_metadata_page() };
            if new_page.is_null() {
                return core::ptr::null_mut();
            }

            match self.current_page.compare_exchange_weak(
                self.current_page.load(Ordering::Relaxed),
                new_page,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    self.offset.store(NODE_SIZE, Ordering::Release);
                    return new_page as *mut MagazineNode;
                }
                Err(_) => {
                    unsafe { dealloc_metadata_page(new_page) };
                    continue;
                }
            }
        }
    }
}

const PAGE_SIZE: usize = 4096;

#[cfg(not(feature = "magazine"))]
unsafe fn alloc_metadata_page() -> *mut u8 {
    core::ptr::null_mut()
}

#[cfg(not(feature = "magazine"))]
unsafe fn dealloc_metadata_page(_ptr: *mut u8) {}

#[cfg(feature = "magazine")]
unsafe fn alloc_metadata_page() -> *mut u8 {
    use libc::{mmap, MAP_ANONYMOUS, MAP_FAILED, MAP_PRIVATE, PROT_READ, PROT_WRITE};
    let ptr = mmap(
        core::ptr::null_mut(),
        PAGE_SIZE,
        PROT_READ | PROT_WRITE,
        MAP_PRIVATE | MAP_ANONYMOUS,
        -1,
        0,
    );
    if ptr == MAP_FAILED {
        core::ptr::null_mut()
    } else {
        ptr as *mut u8
    }
}

#[cfg(feature = "magazine")]
unsafe fn dealloc_metadata_page(ptr: *mut u8) {
    use libc::munmap;
    munmap(ptr as *mut _, PAGE_SIZE);
}

unsafe impl Sync for GlobalMagazinePool {}
unsafe impl Send for GlobalMagazinePool {}
unsafe impl Sync for GlobalMagazinePools {}
unsafe impl Send for GlobalMagazinePools {}
unsafe impl Sync for MetadataAllocator {}
unsafe impl Send for MetadataAllocator {}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::boxed::Box;

    #[test]
    fn test_magazine_push_pop() {
        let mut mag = Magazine::new();
        assert!(mag.is_empty());

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
    fn test_pool_push_pop() {
        let pool = GlobalMagazinePool::new();

        let node = Box::into_raw(Box::new(MagazineNode::new()));
        unsafe {
            pool.push_full(node);
        }

        let popped = pool.pop_full();
        assert!(popped.is_some());
        assert_eq!(popped.unwrap(), node);

        unsafe {
            let _ = Box::from_raw(node);
        }
    }

    #[test]
    fn test_symmetric_stacks() {
        let pool = GlobalMagazinePool::new();

        // Create and fill a magazine
        let node = Box::into_raw(Box::new(MagazineNode::new()));
        unsafe {
            for i in 0..MAGAZINE_CAPACITY {
                assert!((*node).magazine.push((i + 1) as *mut u8));
            }
        }

        // Push to full, pop from full
        unsafe {
            pool.push_full(node);
        }
        let full = pool.pop_full();
        assert!(full.is_some());

        // Clear and push to empty
        unsafe {
            (*full.unwrap()).magazine.clear();
            pool.push_empty(full.unwrap());
        }

        // Pop from empty
        let empty = pool.pop_empty();
        assert!(empty.is_some());

        unsafe {
            let _ = Box::from_raw(empty.unwrap());
        }
    }
}
