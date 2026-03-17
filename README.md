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

## Building

```bash
# Build the shared library
nix build

# Or with cargo directly
cargo build --release -p aethalloc-abi
```

## Usage

```bash
# LD_PRELOAD injection
LD_PRELOAD=./target/release/libaethalloc_abi.so ./your-program

# With Nix wrapper
nix run .#suricata-aeth
```

## Benchmarks

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

## License

MIT

## References

- [ARM MTE Documentation](https://developer.arm.com/documentation/ddi0595/2021-12/Base-Architecture/Memory-Tagging-Extension--)
- [CHERI Capability Hardware](https://www.cl.cam.ac.uk/research/security/ctsrd/cheri/)
- [mremap(2)](https://man7.org/linux/man-pages/man2/mremap.2.html)
- [process_vm_writev(2)](https://man7.org/linux/man-pages/man2/process_vm_writev.2.html)
