# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [0.2.0] - 2026-03-18

### Fixed
- Critical: `PageHeader.num_pages` changed from `u16` to `u32` to support >256MB allocations
- Added missing `posix_memalign` C ABI export (was causing heap corruption)

### Added
- Stress test suite: tail latency, massive allocations, corruption tests
- GitHub Actions workflow with full benchmark suite
- Weekly scheduled benchmark runs with GitHub Summary output

### Performance
- Tail latency P99: 116ns (comparable to glibc's 103ns)
- Massive allocations: 2GB contiguous blocks with 2MB alignment supported
- Corruption test: 100,000+ operations without heap corruption

## [0.1.0] - 2026-03-18

### Added
- Thread-local caching with 14 size classes (16B - 64KB)
- SIMD-safe 16-byte alignment for AVX/SSE
- O(1) anti-hoarding batch transfer to global pool
- Lock-free global free lists (Treiber stacks)
- Magazine caching mode (experimental Hoard-style)
- C ABI exports for LD_PRELOAD injection
- Prometheus metrics exporter
- Comprehensive benchmark suite

### Performance
- Producer-Consumer: 622K ops/s (+26% vs glibc)
- KV Store: 365K ops/s (+11% vs glibc)
- Packet Churn: 245K ops/s (+3% vs glibc)
- Fragmentation: 24 MB RSS (9x better than glibc)
