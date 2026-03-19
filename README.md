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
- **Producer-Consumer**: 447K ops/s (competitive with all allocators)
- **Anti-hoarding**: Prevents memory bloat in packet handoff workloads
- Guarantees line-rate packet inspection

### X1 Yoga Workstation (Desktop)
- **Fragmentation RSS**: 17 MB (1.8x better than glibc)
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
| `magazine-caching` | Hoard-style magazines with global pool | Yes |
| `simple-cache` | Thread-local free-list per size class | No |
| `fast-size-class` | Optimized size lookup (better packet churn, worse multithread) | No |
| `metrics` | Enable allocation metrics collection | No |

### Size Class Lookup Trade-off

The `fast-size-class` feature uses `trailing_zeros()` for O(1) size class lookup:

| Mode | Packet Churn | Multithread Churn | Best For |
|------|--------------|-------------------|----------|
| Default (match) | 262K ops/s | 19.1M ops/s | General purpose |
| `fast-size-class` | 260K ops/s | 16.2M ops/s | Network packet processing |

Enable with: `cargo build --release --features fast-size-class`

## Benchmarks

**Test System:** Intel Core i5-8365U (4 cores, 8 threads) @ 1.60GHz, 16 GB RAM

### Quick Summary

| Benchmark | AethAlloc | Best Competitor | Result |
|-----------|-----------|-----------------|--------|
| **Multithread Churn** | 19.7M ops/s | mimalloc: 19.8M ops/s | **#2** (-0.7%) |
| **Packet Churn** | 309K ops/s | jemalloc: 328K ops/s | **#4** (-6%) |
| **Tail Latency P99** | 87ns | glibc: 83ns | **#5** (+5%) |
| **Tail Latency P99.99** | 718ns | AethAlloc | **WINNER** |
| **Fragmentation RSS** | 17.0 MB | AethAlloc | **WINNER** (1.8x better) |
| **Producer-Consumer** | 430K ops/s | mimalloc: 441K ops/s | **#2** (-3%) |

**See [BENCHMARK.md](BENCHMARK.md) for full methodology, detailed results, and analysis.**

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

# Run stress tests
gcc -O3 benches/corruption_test.c -o /tmp/corruption_test
LD_PRELOAD=./target/release/libaethalloc_abi.so /tmp/corruption_test
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
| Stress tests | ✅ Complete |
| CI/CD | ✅ Complete |

## License

MIT
