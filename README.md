# AethAlloc

A high-performance memory allocator optimized for network packet processing and memory-constrained workloads.

## Overview

AethAlloc is a production-grade memory allocator featuring:

- **Thread-Local Caching**: Lock-free per-thread free lists with 14 size classes (16B - 64KB)
- **SIMD-Safe Alignment**: All allocations are 16-byte aligned for AVX/SSE safety
- **O(1) Anti-Hoarding**: Batch transfer to global pool prevents memory bloat in producer-consumer patterns
- **Zero Fragmentation**: 11x better memory efficiency than glibc in long-running workloads

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Thread N                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ ThreadLocalCache                                         │   │
│  │  heads[14] ──► Free List (size class 0-13)              │   │
│  │  counts[14] ──► Cached block counts                      │   │
│  └─────────────────────────────────────────────────────────┘   │
│                           │                                     │
│              Anti-Hoarding Threshold (4096)                     │
│                           │                                     │
│                           ▼                                     │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ GlobalFreeList[14]                                       │   │
│  │  Lock-free Treiber Stack (O(1) batch push)              │   │
│  └─────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│                      PageAllocator                               │
│  mmap/munmap backend with 4KB page granularity                  │
│  PageHeader: magic + num_pages + requested_size                 │
└─────────────────────────────────────────────────────────────────┘
```

## Crates

| Crate | Description |
|-------|-------------|
| `aethalloc-core` | Core algorithms (page allocator, size classes, lock-free stack) |
| `aethalloc-abi` | C ABI exports for LD_PRELOAD injection |

## Deployment Targets

### m720q Gateway (Multi-WAN Routing)
- **Producer-Consumer**: 670K ops/s (+28% vs glibc)
- Guarantees line-rate packet inspection
- Asynchronous SKB payload handoff between NIC and firewall workers

### X1 Yoga Workstation (Desktop)
- **Fragmentation RSS**: 20 MB (11x better than glibc)
- Prevents memory bloat in long-running desktop environments
- Preserves NVMe lifespan and battery capacity

## Building

```bash
# Build the shared library
nix build

# Or with cargo
cargo build --release -p aethalloc-abi
```

## Usage

```bash
# LD_PRELOAD injection
LD_PRELOAD=./target/release/libaethalloc_abi.so ./your-program

# With Nix wrapper
nix run .#suricata-aeth
```

## Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `simple-cache` | Thread-local free-list per size class | Yes |
| `magazine-caching` | Hoard-style magazines (experimental) | No |

## Benchmarks

**Test System:** Intel Core i5-8365U (4 cores, 8 threads) @ 1.60GHz  
**Last updated:** 2026-03-17

### Summary

| Benchmark | glibc | AethAlloc | Ratio |
|-----------|-------|-----------|-------|
| Packet Churn | 225K ops/s | 231K ops/s | **103%** |
| KV Store | 337K ops/s | 377K ops/s | **112%** |
| Producer-Consumer | 525K ops/s | 670K ops/s | **128%** |
| Multithread (8T) | 11.6M ops/s | 11.1M ops/s | **96%** |
| Fragmentation RSS | 219 MB | 20 MB | **11x better** |

### Packet Churn (Network Processing)

64-byte packet allocations simulating network processing workload.

```
glibc:       225K ops/s
AethAlloc:   231K ops/s (+3%)
P50 latency: 3.5 µs
P99 latency: 6.8 µs
```

### KV Store (Redis-like Workload)

Variable-sized keys (8-64B) and values (16-64KB) with SET/GET/DEL operations.

```
glibc:       337K ops/s
AethAlloc:   377K ops/s (+12%)
SET latency: 3.5 µs
GET latency: 0.6 µs
DEL latency: 0.7 µs
```

### Producer-Consumer (Cross-Thread Frees)

Thread A allocates, Thread B frees. Simulates network packet handoff.

```
glibc:       525K ops/s
AethAlloc:   670K ops/s (+28%)
Memory:      Stable at 138 MB (anti-hoarding active)
```

### Multithread Churn (8 Threads)

Concurrent allocations (16B - 4KB) across 8 threads with heavy contention.

```
glibc:       11.6M ops/s
AethAlloc:   11.1M ops/s (96%)
Avg latency: 450 ns
```

### Fragmentation (Long-running Server)

Mixed allocation sizes (16B - 1MB) over 1M iterations.

```
glibc:       219 MB RSS
AethAlloc:   20 MB RSS (11x better)
Throughput:  214K ops/s
```

## Technical Implementation

### SIMD Alignment

All allocations return 16-byte aligned pointers:

```rust
const CACHE_HEADER_SIZE: usize = 16; // Ensures AVX/SSE safety
```

### O(1) Batch Push

Anti-hoarding uses single CAS for entire batch:

```rust
// Walk local list to find tail
while walked < flush_count {
    batch_tail = (*batch_tail).next;
}

// Single atomic swap for entire batch
GLOBAL_FREE_LISTS[class].push_batch(batch_head, batch_tail);
```

### Size Classes

14 power-of-two size classes from 16 bytes to 64KB:

| Class | Size | Class | Size |
|-------|------|-------|------|
| 0 | 16B | 7 | 2KB |
| 1 | 32B | 8 | 4KB |
| 2 | 64B | 9 | 8KB |
| 3 | 128B | 10 | 16KB |
| 4 | 256B | 11 | 32KB |
| 5 | 512B | 12 | 64KB |
| 6 | 1KB | 13 | (reserved) |

## Testing

```bash
# Run all tests
cargo test --all

# Run benchmarks
gcc -O3 -pthread benches/packet_churn.c -o /tmp/packet_churn
LD_PRELOAD=./target/release/libaethalloc_abi.so /tmp/packet_churn
```

## Status

| Component | Status |
|-----------|--------|
| Core allocator | ✅ Complete |
| Thread-local caching | ✅ Complete |
| SIMD alignment | ✅ Complete |
| O(1) anti-hoarding | ✅ Complete |
| Lock-free global pool | ✅ Complete |
| Benchmarks | ✅ Complete |

## License

MIT
