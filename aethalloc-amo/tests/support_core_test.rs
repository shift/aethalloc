//! Integration test for ring buffer + support core
//!
//! Tests the full AMO pipeline with concurrent producer/consumer.

#![cfg(feature = "std")]

use aethalloc_amo::command::{FreeBlockPayload, RingCommand, RingEntry, RingPayload};
use aethalloc_amo::ring_buffer::RingBuffer;
use aethalloc_amo::support_core::SupportCore;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[test]
fn test_support_core_processes_commands() {
    static RING: RingBuffer<256> = RingBuffer::new();

    let mut support_core = SupportCore::new(&RING);

    let entry = RingEntry::new(RingCommand::NoOp, RingPayload::default());
    RING.try_push(entry).unwrap();

    support_core.handle_command(entry);
}

#[test]
fn test_producer_consumer_threads() {
    static RING: RingBuffer<1024> = RingBuffer::new();

    let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let running_clone = running.clone();

    let consumer = thread::spawn(move || {
        let mut core = SupportCore::new(&RING);
        while running_clone.load(std::sync::atomic::Ordering::Relaxed) {
            if let Some(entry) = RING.try_pop() {
                core.handle_command(entry);
            }
            thread::yield_now();
        }
    });

    let producer = thread::spawn(move || {
        for i in 0..100 {
            let payload = FreeBlockPayload {
                ptr: i as *mut u8,
                size: i * 16,
                size_class: (i % 16) as u8,
            };
            let entry = RingEntry::new(
                RingCommand::FreeBlock,
                RingPayload {
                    free_block: payload,
                },
            );
            while RING.try_push(entry).is_none() {
                thread::yield_now();
            }
        }
    });

    producer.join().unwrap();
    thread::sleep(Duration::from_millis(50));

    running.store(false, std::sync::atomic::Ordering::Relaxed);
    consumer.join().unwrap();
}

#[test]
fn test_high_throughput() {
    static RING: RingBuffer<4096> = RingBuffer::new();

    let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let running_clone = running.clone();

    let counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let counter_clone = counter.clone();

    let consumer = thread::spawn(move || {
        let mut processed = 0u64;
        while running_clone.load(std::sync::atomic::Ordering::Relaxed) || !RING.is_empty() {
            if let Some(_entry) = RING.try_pop() {
                processed += 1;
            }
            thread::yield_now();
        }
        counter_clone.store(processed, std::sync::atomic::Ordering::Relaxed);
    });

    let producer = thread::spawn(move || {
        for _ in 0..10_000 {
            let entry = RingEntry::new(RingCommand::NoOp, RingPayload::default());
            while RING.try_push(entry).is_none() {
                thread::yield_now();
            }
        }
    });

    producer.join().unwrap();
    running.store(false, std::sync::atomic::Ordering::Relaxed);
    consumer.join().unwrap();

    assert_eq!(counter.load(std::sync::atomic::Ordering::Relaxed), 10_000);
}
