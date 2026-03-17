//! Benchmarks for SPSC ring buffer push/pop latency

use aethalloc_amo::{ring_buffer::RingEntry, RingBuffer, RingCommand, RingPayload};
use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};

fn bench_push_pop(c: &mut Criterion) {
    let mut group = c.benchmark_group("ring_buffer");

    const CAPACITY: usize = 4096;

    group.throughput(Throughput::Elements(1));

    // Benchmark push alone
    group.bench_function("try_push", |b| {
        let rb: RingBuffer<CAPACITY> = RingBuffer::new();
        let entry = RingEntry {
            command: RingCommand::NoOp,
            payload: RingPayload::default(),
        };

        b.iter(|| {
            // Push and pop to keep buffer from filling
            black_box(rb.try_push(entry.clone()));
            black_box(rb.try_pop());
        });
    });

    // Benchmark pop alone (with pre-filled buffer)
    group.bench_function("try_pop", |b| {
        let rb: RingBuffer<CAPACITY> = RingBuffer::new();
        let entry = RingEntry {
            command: RingCommand::NoOp,
            payload: RingPayload::default(),
        };

        // Pre-fill half the buffer
        for _ in 0..CAPACITY / 2 {
            rb.try_push(entry.clone()).unwrap();
        }

        b.iter(|| {
            let result = black_box(rb.try_pop());
            if result.is_some() {
                black_box(rb.try_push(entry.clone()));
            }
            result
        });
    });

    // Benchmark combined push + pop
    group.bench_function("push_pop_roundtrip", |b| {
        let rb: RingBuffer<CAPACITY> = RingBuffer::new();
        let entry = RingEntry {
            command: RingCommand::NoOp,
            payload: RingPayload::default(),
        };

        b.iter(|| {
            black_box(rb.try_push(entry.clone()));
            black_box(rb.try_pop())
        });
    });

    group.finish();
}

fn bench_latency_breakdown(c: &mut Criterion) {
    let mut group = c.benchmark_group("latency");

    const CAPACITY: usize = 4096;

    // Measure just the atomic operations
    group.bench_function("atomic_load_store", |b| {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let head = AtomicUsize::new(0);
        let mask = CAPACITY - 1;

        b.iter(|| {
            let h = black_box(head.load(Ordering::Relaxed));
            let next = (h + 1) & mask;
            black_box(head.store(next, Ordering::Release));
        });
    });

    group.finish();
}

criterion_group!(benches, bench_push_pop, bench_latency_breakdown);
criterion_main!(benches);
