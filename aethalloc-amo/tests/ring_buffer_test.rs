//! Tests for SPSC ring buffer

extern crate std;
use std::sync::Arc;
use std::thread;

use aethalloc_amo::{RingBuffer, RingCommand, RingEntry, RingPayload};

#[test]
fn test_push_pop_single() {
    let rb: RingBuffer<16> = RingBuffer::new();

    let entry = RingEntry::default();

    assert!(rb.try_push(entry).is_some());
    assert_eq!(rb.len(), 1);

    let popped = rb.try_pop();
    assert!(popped.is_some());
    assert_eq!(rb.len(), 0);
}

#[test]
fn test_fill_drain() {
    let rb: RingBuffer<16> = RingBuffer::new();

    for i in 0..15 {
        let entry = RingEntry::default();
        assert!(rb.try_push(entry).is_some(), "push {} failed", i);
    }

    assert!(rb.is_full());

    let entry = RingEntry::default();
    assert!(rb.try_push(entry).is_none());

    for i in 0..15 {
        assert!(rb.try_pop().is_some(), "pop {} failed", i);
    }

    assert!(rb.is_empty());
}

#[test]
fn test_wrap_around() {
    let rb: RingBuffer<4> = RingBuffer::new();

    for round in 0..5 {
        for i in 0..3 {
            let entry = RingEntry::default();
            assert!(
                rb.try_push(entry).is_some(),
                "round {} push {} failed",
                round,
                i
            );
        }
        for i in 0..3 {
            assert!(rb.try_pop().is_some(), "round {} pop {} failed", round, i);
        }
    }
}

#[test]
fn test_multithreaded_spsc() {
    let rb = Arc::new(RingBuffer::<1024>::new());

    const NUM_MESSAGES: usize = 100_000;

    let producer_rb = Arc::clone(&rb);
    let producer = thread::spawn(move || {
        for _ in 0..NUM_MESSAGES {
            let entry = RingEntry::default();
            producer_rb.push(entry);
        }
    });

    let consumer_rb = Arc::clone(&rb);
    let consumer = thread::spawn(move || {
        let mut count = 0;
        while count < NUM_MESSAGES {
            if consumer_rb.try_pop().is_some() {
                count += 1;
            }
        }
        count
    });

    producer.join().unwrap();
    let received = consumer.join().unwrap();

    assert_eq!(received, NUM_MESSAGES);
}

#[test]
fn test_len_accuracy() {
    let rb: RingBuffer<16> = RingBuffer::new();

    assert_eq!(rb.len(), 0);

    for i in 1..=7 {
        let entry = RingEntry::default();
        rb.try_push(entry).unwrap();
        assert_eq!(rb.len(), i);
    }

    for i in (0..7).rev() {
        rb.try_pop().unwrap();
        assert_eq!(rb.len(), i);
    }

    assert_eq!(rb.len(), 0);
}

#[test]
fn test_entry_new() {
    let payload = RingPayload::default();
    let entry = RingEntry::new(RingCommand::FreeBlock, payload);
    assert_eq!(entry.command, RingCommand::FreeBlock);
}
