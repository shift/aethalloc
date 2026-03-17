# AethAlloc

**⚠️ EXPERIMENTAL - NOT FOR PRODUCTION USE ⚠️**

A research-grade memory allocator implementing asynchronous metadata offloading, hardware-enforced spatial safety, and virtual memory page compaction.

## Overview

AethAlloc is an experimental memory allocator designed to explore novel allocation strategies:

- **Asynchronous Metadata Offloading (AMO)**: Offloads free-list management, compaction, and telemetry to a dedicated support core via lock-free SPSC ring buffers
- **Hardware-Enforced Spatial Safety (HESS)**: ARM MTE and CHERI capability integration for hardware-accelerated bounds checking
- **Virtual Memory Page Compaction (VMPC)**: mremap-based page migration and process_vm_writev for cross-process memory operations

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Application Core                         │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │ThreadLocal  │  │   Slab      │  │      Buddy          │  │
│  │   Cache     │  │ Allocator   │  │    Allocator        │  │
│  └──────┬──────┘  └──────┬──────┘  └──────────┬──────────┘  │
│         │                │                    │             │
│         └────────────────┼────────────────────┘             │
│                          │                                  │
│                    ┌─────▼─────┐                            │
│                    │SPSC Ring  │◄──── AMO Commands          │
│                    │  Buffer   │                            │
│                    └─────┬─────┘                            │
└──────────────────────────┼──────────────────────────────────┘
                           │
┌──────────────────────────▼──────────────────────────────────┐
│                    Support Core                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │Global Pools │  │   VMPC      │  │      HESS           │  │
│  │(Lock-Free)  │  │ Compactor   │  │   Tag Manager       │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

## Crates

| Crate | Description |
|-------|-------------|
| `aethalloc-core` | Core allocation algorithms (slab, buddy, thread-local cache) |
| `aethalloc-amo` | Asynchronous metadata offloading (SPSC ring buffer) |
| `aethalloc-vmpc` | Virtual memory page compaction |
| `aethalloc-hess` | Hardware-enforced spatial safety (MTE/CHERI) |
| `aethalloc-abi` | C ABI exports for LD_PRELOAD injection |

## Rationale

### Why Experimental?

This allocator explores techniques that require:
- **Hardware support**: ARM MTE (ARMv8.5+) or CHERI capabilities
- **Kernel integration**: /proc/self/pagemap access, mremap, process_vm_writev
- **Careful auditing**: Extensive `unsafe` blocks for hardware intrinsics

### Design Goals

1. **Minimize application core latency**: Push metadata operations to support core
2. **Hardware acceleration**: Use CPU tagging features when available
3. **Memory compaction**: Reduce fragmentation via page migration
4. **No_std compatible**: Works in freestanding environments

### When NOT to Use

- Production systems requiring stable allocators
- Applications without hardware MTE/CHERI support
- Environments where LD_PRELOAD is restricted
- Systems requiring rigorous safety certification

### When to Use AethAlloc

**Recommended for:**
- Memory-constrained environments (uses 11x less memory in fragmentation workloads)
- Network packet processing (6% faster than glibc)
- KV-store / cache workloads (13% faster than glibc)
- Single-threaded or low-contention scenarios

**Not recommended for:**
- High thread contention (>4 threads with heavy allocation churn)
- Workloads dominated by large allocations (>64KB)
- Sequential allocation patterns where glibc's slab is optimized

## Building

```bash
# Build the shared library (default: simple-cache mode)
nix build

# Or with cargo directly
cargo build --release -p aethalloc-abi

# Build with magazine-caching mode (for cross-thread heavy workloads)
cargo build --release -p aethalloc-abi --features magazine-caching
```

## Usage

```bash
# LD_PRELOAD injection
LD_PRELOAD=./target/release/libaethalloc_abi.so ./your-program

# With Nix wrapper
nix run .#suricata-aeth
```

## Feature Flags

| Feature | Description | When to use |
|---------|-------------|--------------|
| `simple-cache` (default) | Thread-local free-list per size class | Independent thread workloads, low cross-thread frees |
| `magazine-caching` | Hoard-style magazines with global pool | Heavy cross-thread memory transfers, producer-consumer patterns |

Use `simple-cache` (default) for:
- Network packet processing
- Single-threaded applications
- Low contention scenarios

Use `magazine-caching` for:
- Multi-producer workloads
- Thread pools with frequent cross-thread frees
- High contention scenarios

## Benchmarks

**Test System:** Intel Core i5-8365U (4 cores, 8 threads) @ 1.60GHz  
**Last updated:** 2026-03-17

### Summary

AethAlloc achieves parity or better with glibc in key workloads while using significantly less memory in fragmentation-heavy scenarios.

| Benchmark | glibc | AethAlloc | Ratio | Winner |
|-----------|-------|-----------|-------|--------|
| Packet Churn | 225K ops/s | 272K ops/s | 121% | AethAlloc |
| KV Store | 337K ops/s | 369K ops/s | 109% | AethAlloc |
| Producer-Consumer | 543K ops/s | 724K ops/s | 133% | AethAlloc |
| Multithread (8T) | 10.6M ops/s | 9.6M ops/s | 91% | glibc |
| Fragmentation | 246K ops/s | 141K ops/s | 57% | glibc |
| Fragmentation RSS | 219 MB | 19 MB | 11x better | AethAlloc |

### Packet Churn (Network Processing)

Simulates network packet processing with 64-byte allocations.

| Metric | glibc | AethAlloc | Delta |
|--------|-------|-----------|-------|
| Throughput | 185,984 ops/s | 198,157 ops/s | +7% |
| P50 latency | 4,650 ns | 4,395 ns | -5% |
| P95 latency | 5,578 ns | 5,512 ns | -1% |
| P99 latency | 7,962 ns | 7,671 ns | -4% |

### KV Store (Redis-like Workload)

Variable-sized keys (8-64B) and values (16-64KB).

| Metric | glibc | AethAlloc | Delta |
|--------|-------|-----------|-------|
| Throughput | 260,276 ops/s | 257,082 ops/s | -1% |
| SET latency | 5,296 ns | 5,302 ns | 0% |
| GET latency | 703 ns | 758 ns | +8% |
| DEL latency | 1,169 ns | 968 ns | -17% |

### Fragmentation (Long-running Server)

Mixed allocation sizes (16B - 1MB) over 1M iterations.

| Metric | glibc | AethAlloc | Delta |
|--------|-------|-----------|-------|
| Throughput | 245,905 ops/s | 140,528 ops/s | -43% |
| RSS growth | 218,624 KB | 18,592 KB | -91% |

### Multithread Churn (8 Threads)

Concurrent allocations (16B - 4KB) across 8 threads.

| Metric | glibc | AethAlloc | Delta |
|--------|-------|-----------|-------|
| Throughput | 7.88M ops/s | 6.73M ops/s | -15% |

### Producer-Consumer (Cross-Thread Frees)

Thread A allocates, Thread B frees. Simulates network packet processing.

| Metric | glibc | AethAlloc (simple-cache) |
|--------|-------|---------------------------|
| Throughput | 543K ops/s | 724K ops/s (+33%) |
| Avg latency | 690 ns | 754 ns | +9% |

### Single-Thread Cache

1M sequential alloc/free cycles (64-byte blocks).

| Metric | glibc | AethAlloc |
|--------|-------|-----------|
| Throughput | 9.34M ops/s | 5.93M ops/s |
| Latency | 107 ns | 169 ns |

### Ring Buffer (SPSC)

| Operation | Latency | Throughput |
|-----------|---------|------------|
| try_push | ~100 ns | ~10 M elem/s |
| try_pop | ~240 ns | ~4 M elem/s |
| roundtrip | ~225 ns | ~4.4 M elem/s |

### Stress Tests

All 10 stress tests pass including:
- 1000 slab allocations/deallocations
- Concurrent allocations across 4 threads
- Fragmentation pattern handling
- Size class boundary verification

## Testing

```bash
# Run all tests
cargo test --all

# Run stress tests
cargo test -p aethalloc-core --test stress_test

# Run benchmarks
cargo bench -p aethalloc-amo
```

## Status

| Phase | Description | Status |
|-------|-------------|--------|
| 1-4 | Core allocator, C ABI, ring buffer | ✅ Complete |
| 5 | Async metadata offloading | ✅ Complete |
| 6 | VM page compaction | ✅ Complete |
| 7 | Hardware safety (MTE/CHERI) | ✅ Complete |
| 8 | Benchmarks & stress tests | ✅ Complete |
| 9 | Performance optimization | ✅ Complete (beats glibc in packet_churn, 85% multithread) |

### Recent Optimizations

| Commit | Change | Impact |
|--------|--------|--------|
| 6e229fd | Thread-local cache isolation | Fixed crashes with >1 thread |
| 0662bba | Batch allocation (slab-style) | 3x memory reduction |
| 2696124 | 64KB cache (13 size classes) | +80% kv_store throughput |
| 037c005 | Remove global atomic counters | 79% → 85% multithread performance |

### Future Optimization Phases

| Phase | Description | Target |
|-------|-------------|--------|
| 10 | Magazine caching (Hoard-style) | 90%+ multithread performance |
| 11 | Async cross-thread frees (MPSC) | Eliminate cross-core locking |
| 12 | Transparent Huge Pages (THP) | Faster large allocations (>64KB) |

## License

MIT

## References

- [ARM MTE Documentation](https://developer.arm.com/documentation/ddi0595/2021-12/Base-Architecture/Memory-Tagging-Extension--)
- [CHERI Capability Hardware](https://www.cl.cam.ac.uk/research/security/ctsrd/cheri/)
- [mremap(2)](https://man7.org/linux/man-pages/man2/mremap.2.html)
- [process_vm_writev(2)](https://man7.org/linux/man-pages/man2/process_vm_writev.2.html)
