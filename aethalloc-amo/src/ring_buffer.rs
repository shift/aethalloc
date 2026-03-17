//! SPSC lock-free ring buffer for AMO
//!
//! This module implements a Single-Producer/Single-Consumer lock-free queue
//! optimized for L1 cache locality using:
//! 1. Cache-line padding to prevent false sharing between head/tail
//! 2. Shadow indices to avoid cross-core atomic loads

use core::cell::Cell;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::command::RingEntry;

/// Cache line size on x86_64 and ARM
const CACHE_LINE: usize = 64;

/// Padded atomic to prevent false sharing
#[repr(C, align(64))]
struct PaddedAtomicUsize {
    value: AtomicUsize,
    _pad: [u8; CACHE_LINE - core::mem::size_of::<AtomicUsize>()],
}

impl PaddedAtomicUsize {
    const fn new(val: usize) -> Self {
        Self {
            value: AtomicUsize::new(val),
            _pad: [0; CACHE_LINE - core::mem::size_of::<AtomicUsize>()],
        }
    }

    #[inline]
    fn load(&self, order: Ordering) -> usize {
        self.value.load(order)
    }

    #[inline]
    fn store(&self, val: usize, order: Ordering) {
        self.value.store(val, order)
    }
}

/// Padded Cell for shadow indices
#[repr(C, align(64))]
struct PaddedCellUsize {
    value: Cell<usize>,
    _pad: [u8; CACHE_LINE - core::mem::size_of::<Cell<usize>>()],
}

impl PaddedCellUsize {
    const fn new(val: usize) -> Self {
        Self {
            value: Cell::new(val),
            _pad: [0; CACHE_LINE - core::mem::size_of::<Cell<usize>>()],
        }
    }

    #[inline]
    fn get(&self) -> usize {
        self.value.get()
    }

    #[inline]
    fn set(&self, val: usize) {
        self.value.set(val)
    }
}

/// SPSC ring buffer with power-of-2 capacity
///
/// Optimized for L1 cache locality:
/// - `head` and `tail` are on separate cache lines (no false sharing)
/// - Shadow indices avoid atomic loads on every operation
#[repr(C, align(64))]
pub struct RingBuffer<const CAPACITY: usize> {
    /// Producer index (only modified by producer)
    /// Padded to own entire cache line
    head: PaddedAtomicUsize,

    /// Consumer index (only modified by consumer)  
    /// Padded to own entire cache line
    tail: PaddedAtomicUsize,

    /// Producer's shadow copy of tail (kept in L1)
    shadow_tail: PaddedCellUsize,

    /// Consumer's shadow copy of head (kept in L1)
    shadow_head: PaddedCellUsize,

    /// Buffer storage (cache-aligned entries)
    buffer: [UnsafeCell<RingEntry>; CAPACITY],
}

unsafe impl<const CAPACITY: usize> Sync for RingBuffer<CAPACITY> {}
unsafe impl<const CAPACITY: usize> Send for RingBuffer<CAPACITY> {}

impl<const CAPACITY: usize> Default for RingBuffer<CAPACITY> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAPACITY: usize> RingBuffer<CAPACITY> {
    pub const fn new() -> Self {
        assert!(CAPACITY.is_power_of_two(), "capacity must be power of 2");

        Self {
            head: PaddedAtomicUsize::new(0),
            tail: PaddedAtomicUsize::new(0),
            shadow_tail: PaddedCellUsize::new(0),
            shadow_head: PaddedCellUsize::new(0),
            buffer: unsafe { core::mem::zeroed() },
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Relaxed);
        head.wrapping_sub(tail) & (CAPACITY - 1)
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn is_full(&self) -> bool {
        self.len() == CAPACITY - 1
    }

    /// Try to push an entry (non-blocking)
    /// Returns None if buffer is full
    ///
    /// Optimized: Only performs atomic load of tail when shadow indicates full
    #[inline]
    pub fn try_push(&self, entry: RingEntry) -> Option<()> {
        let head = self.head.load(Ordering::Relaxed);
        let next_head = (head + 1) & (CAPACITY - 1);

        // Fast path: check shadow tail (L1 cache, no cross-core traffic)
        let mut shadow_tail = self.shadow_tail.get();

        if next_head == shadow_tail {
            // Slow path: shadow says full, check actual tail (cache miss)
            shadow_tail = self.tail.load(Ordering::Acquire);
            self.shadow_tail.set(shadow_tail);

            if next_head == shadow_tail {
                return None;
            }
        }

        // SAFETY: Producer owns slots[head], consumer never reads it
        unsafe {
            core::ptr::write(self.buffer[head].get(), entry);
        }

        self.head.store(next_head, Ordering::Release);
        Some(())
    }

    #[inline]
    pub fn push(&self, entry: RingEntry) {
        while self.try_push(entry).is_none() {
            core::hint::spin_loop();
        }
    }

    /// Try to pop an entry (non-blocking)
    /// Returns None if buffer is empty
    ///
    /// Optimized: Only performs atomic load of head when shadow indicates empty
    #[inline]
    pub fn try_pop(&self) -> Option<RingEntry> {
        let tail = self.tail.load(Ordering::Relaxed);

        // Fast path: check shadow head (L1 cache, no cross-core traffic)
        let mut shadow_head = self.shadow_head.get();

        if tail == shadow_head {
            // Slow path: shadow says empty, check actual head (cache miss)
            shadow_head = self.head.load(Ordering::Acquire);
            self.shadow_head.set(shadow_head);

            if tail == shadow_head {
                return None;
            }
        }

        // SAFETY: Consumer owns slots[tail], producer never writes it
        let entry = unsafe { core::ptr::read(self.buffer[tail].get()) };

        let next_tail = (tail + 1) & (CAPACITY - 1);
        self.tail.store(next_tail, Ordering::Release);

        Some(entry)
    }

    #[inline]
    pub fn pop(&self) -> RingEntry {
        loop {
            if let Some(entry) = self.try_pop() {
                return entry;
            }
            core::hint::spin_loop();
        }
    }

    pub const fn capacity(&self) -> usize {
        CAPACITY
    }
}

const _: () = assert!(core::mem::size_of::<RingEntry>() == 64);
const _: () = assert!(core::mem::align_of::<RingEntry>() == 64);
