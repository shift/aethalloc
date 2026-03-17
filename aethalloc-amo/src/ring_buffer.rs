//! SPSC lock-free ring buffer for AMO
//!
//! This module implements a Single-Producer/Single-Consumer lock-free queue
//! for sending commands from the application core to the support core.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::command::RingEntry;

/// SPSC ring buffer with power-of-2 capacity
///
/// This is a wait-free single-producer/single-consumer queue.
/// Producer: push() - only one thread may call this
/// Consumer: pop() - only one thread may call this
#[repr(C, align(64))]
pub struct RingBuffer<const CAPACITY: usize> {
    /// Buffer storage
    buffer: [UnsafeCell<RingEntry>; CAPACITY],
    /// Producer index (only modified by producer)
    head: AtomicUsize,
    /// Consumer index (only modified by consumer)
    tail: AtomicUsize,
    /// Cache-line padding to prevent false sharing
    _pad: [u8; 64],
}

// SAFETY: SPSC ring buffer is Sync because:
// - Only one producer thread modifies head
// - Only one consumer thread modifies tail
// - Buffer slots are written by producer before head is incremented (Release)
// - Buffer slots are read by consumer after head is observed (Acquire)
// - No two threads access the same slot simultaneously
unsafe impl<const CAPACITY: usize> Sync for RingBuffer<CAPACITY> {}

// SAFETY: SPSC ring buffer is Send because it can be safely transferred
// between threads. The Sync impl guarantees thread-safe access once shared.
unsafe impl<const CAPACITY: usize> Send for RingBuffer<CAPACITY> {}

impl<const CAPACITY: usize> Default for RingBuffer<CAPACITY> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAPACITY: usize> RingBuffer<CAPACITY> {
    /// Create a new ring buffer
    ///
    /// # Panics
    /// Panics at compile time if CAPACITY is not a power of 2
    pub const fn new() -> Self {
        assert!(CAPACITY.is_power_of_two(), "capacity must be power of 2");

        // SAFETY: Zeroed memory is valid for RingEntry because:
        // - RingCommand::NoOp = 255, but zeroed bytes will be interpreted as 0 (FreeBlock)
        // - This is fine because entries are only read after being written
        // - The buffer starts empty, so zeroed entries are never read
        Self {
            buffer: unsafe { core::mem::zeroed() },
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            _pad: [0; 64],
        }
    }

    /// Get the number of entries currently in the buffer
    pub fn len(&self) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Relaxed);
        head.wrapping_sub(tail) & (CAPACITY - 1)
    }

    /// Check if the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Check if the buffer is full
    pub fn is_full(&self) -> bool {
        self.len() == CAPACITY - 1
    }

    /// Try to push an entry (non-blocking)
    /// Returns None if buffer is full
    pub fn try_push(&self, entry: RingEntry) -> Option<()> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);

        let next_head = (head + 1) & (CAPACITY - 1);

        // Check if full (head catches up to tail)
        if next_head == tail {
            return None;
        }

        // SAFETY: We have exclusive write access to slots[head] because:
        // - Producer owns the head position
        // - Consumer never reads slots[head] because head != tail implies not empty
        // - We write before incrementing head, so consumer won't see partial data
        unsafe {
            core::ptr::write(self.buffer[head].get(), entry);
        }

        // Publish the entry (Release ensures write is visible before head update)
        self.head.store(next_head, Ordering::Release);

        Some(())
    }

    /// Push an entry, spinning until space is available
    /// WARNING: This can spin forever if consumer is dead
    pub fn push(&self, entry: RingEntry) {
        while self.try_push(entry).is_none() {
            core::hint::spin_loop();
        }
    }

    /// Try to pop an entry (non-blocking)
    /// Returns None if buffer is empty
    pub fn try_pop(&self) -> Option<RingEntry> {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);

        // Check if empty
        if tail == head {
            return None;
        }

        // SAFETY: We have exclusive read access to slots[tail] because:
        // - Consumer owns the tail position
        // - Producer never writes to slots[tail] because tail != head implies not full
        // - We read after observing head update (Acquire), so data is visible
        let entry = unsafe { core::ptr::read(self.buffer[tail].get()) };

        // Advance tail (Release ensures read completes before tail update)
        let next_tail = (tail + 1) & (CAPACITY - 1);
        self.tail.store(next_tail, Ordering::Release);

        Some(entry)
    }

    /// Pop an entry, spinning until one is available
    /// WARNING: This can spin forever if producer is dead
    pub fn pop(&self) -> RingEntry {
        loop {
            if let Some(entry) = self.try_pop() {
                return entry;
            }
            core::hint::spin_loop();
        }
    }

    /// Get capacity
    pub const fn capacity(&self) -> usize {
        CAPACITY
    }
}

// Verify RingEntry is exactly 64 bytes at compile time
const _: () = assert!(core::mem::size_of::<RingEntry>() == 64);
const _: () = assert!(core::mem::align_of::<RingEntry>() == 64);
