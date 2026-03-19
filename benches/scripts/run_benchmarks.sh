#!/usr/bin/env bash
#
# Allocator Benchmark Runner
# 
# Runs benchmarks against multiple allocators for comparison.
# Recommended pairings based on allocator design strengths:
#
#   packet_churn      -> mimalloc (fast-path performance, bounded latency)
#   tail_latency      -> mimalloc (extreme latency distribution control)
#   producer_consumer -> snmalloc (cross-thread deallocation efficiency)
#   fragmentation     -> jemalloc (RSS bounds, fragmentation avoidance)
#   multithread_churn -> tcmalloc (thread-local caching pioneer)
#
# Note: snmalloc requires manual installation from https://github.com/microsoft/snmalloc
#

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BENCH_DIR="$(dirname "$SCRIPT_DIR")"
RESULTS_DIR="${1:-./benchmark-results}"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
RUN_DIR="$RESULTS_DIR/$TIMESTAMP"

mkdir -p "$RUN_DIR"

ITERATIONS=${ITERATIONS:-100000}
WARMUP=${WARMUP:-10000}
THREADS=${THREADS:-8}

ALLOCATORS="${ALLOCATORS:-aethalloc,mimalloc,jemalloc,tcmalloc,glibc}"
BENCHMARKS="${BENCHMARKS:-packet_churn,tail_latency,producer_consumer,fragmentation,multithread_churn}"

AETHALLOC_LIB="${AETHALLOC_LIB:-}"
MIMALLOC_LIB="${MIMALLOC_LIB:-/usr/lib/x86_64-linux-gnu/libmimalloc.so}"
JEMALLOC_LIB="${JEMALLOC_LIB:-/usr/lib/x86_64-linux-gnu/libjemalloc.so.2}"
TCMALLOC_LIB="${TCMALLOC_LIB:-/usr/lib/x86_64-linux-gnu/libtcmalloc.so}"
SNMALLOC_LIB="${SNMALLOC_LIB:-/usr/lib/x86_64-linux-gnu/libsnmallocshim.so}"

get_allocator_lib() {
    local alloc="$1"
    case "$alloc" in
        aethalloc) echo "$AETHALLOC_LIB" ;;
        mimalloc)  echo "$MIMALLOC_LIB" ;;
        jemalloc)  echo "$JEMALLOC_LIB" ;;
        tcmalloc)  echo "$TCMALLOC_LIB" ;;
        snmalloc)  echo "$SNMALLOC_LIB" ;;
        glibc)     echo "" ;;
        *)         echo "" ;;
    esac
}

get_allocator_desc() {
    local alloc="$1"
    case "$alloc" in
        aethalloc) echo "AethAlloc (network workload optimized)" ;;
        mimalloc)  echo "Microsoft mimalloc (fast-path focused)" ;;
        jemalloc)  echo "jemalloc (fragmentation avoidance)" ;;
        tcmalloc)  echo "Google tcmalloc (thread-caching)" ;;
        snmalloc)  echo "Microsoft snmalloc (message passing)" ;;
        glibc)     echo "glibc ptmalloc2 (system default)" ;;
        *)         echo "$alloc" ;;
    esac
}

get_benchmark_args() {
    local bench="$1"
    case "$bench" in
        packet_churn)      echo "$ITERATIONS $WARMUP" ;;
        tail_latency)      echo "$THREADS $ITERATIONS" ;;
        producer_consumer) echo "4 4" ;;
        fragmentation)     echo "$ITERATIONS 100000" ;;
        multithread_churn) echo "$THREADS $ITERATIONS" ;;
        *)                 echo "$ITERATIONS" ;;
    esac
}

run_benchmark() {
    local bench="$1"
    local alloc="$2"
    local lib_path="$3"
    local output_file="$RUN_DIR/${bench}_${alloc}.json"
    
    local bench_bin="$BENCH_DIR/$bench"
    if [ ! -x "$bench_bin" ]; then
        bench_bin="$BENCH_DIR/bin/$bench"
    fi
    if [ ! -x "$bench_bin" ]; then
        echo "ERROR: Benchmark binary not found: $bench" >&2
        return 1
    fi
    
    local args
    args=$(get_benchmark_args "$bench")
    
    printf "  %-15s with %-12s ... " "$bench" "$alloc"
    
    local start_time
    start_time=$(date +%s.%N)
    
    if [ -n "$lib_path" ]; then
        if [ ! -f "$lib_path" ]; then
            echo "SKIP (lib not found)"
            return 0
        fi
        LD_PRELOAD="$lib_path" "$bench_bin" $args > "$output_file" 2>&1
    else
        "$bench_bin" $args > "$output_file" 2>&1
    fi
    
    local end_time
    end_time=$(date +%s.%N)
    local elapsed
    elapsed=$(echo "$end_time - $start_time" | bc)
    
    echo "done (${elapsed}s)"
}

get_target_allocators() {
    local bench="$1"
    case "$bench" in
        packet_churn)      echo "mimalloc,aethalloc,glibc" ;;
        tail_latency)      echo "mimalloc,aethalloc,glibc" ;;
        producer_consumer) echo "snmalloc,aethalloc,glibc" ;;
        fragmentation)     echo "jemalloc,aethalloc,glibc" ;;
        multithread_churn) echo "tcmalloc,aethalloc,glibc" ;;
        *)                 echo "$ALLOCATORS" ;;
    esac
}

main() {
    echo "========================================"
    echo "  Allocator Benchmark Suite"
    echo "========================================"
    echo ""
    echo "Configuration:"
    echo "  Iterations: $ITERATIONS"
    echo "  Warmup:     $WARMUP"
    echo "  Threads:    $THREADS"
    echo "  Results:    $RUN_DIR"
    echo ""
    
    IFS=',' read -ra BENCH_ARRAY <<< "$BENCHMARKS"
    IFS=',' read -ra ALLOC_ARRAY <<< "$ALLOCATORS"
    
    for bench in "${BENCH_ARRAY[@]}"; do
        echo ""
        echo "=== Benchmark: $bench ==="
        
        local target_allocs
        if [ "${FULL_COMPARISON:-0}" = "1" ]; then
            target_allocs="$ALLOCATORS"
        else
            target_allocs=$(get_target_allocators "$bench")
        fi
        
        IFS=',' read -ra TARGET_ARRAY <<< "$target_allocs"
        for alloc in "${TARGET_ARRAY[@]}"; do
            lib_path=$(get_allocator_lib "$alloc")
            run_benchmark "$bench" "$alloc" "$lib_path"
        done
    done
    
    echo ""
    echo "========================================"
    echo "  Results Summary"
    echo "========================================"
    
    for f in "$RUN_DIR"/*.json; do
        [ -f "$f" ] || continue
        local name
        name=$(basename "$f" .json)
        echo ""
        echo "$name:"
        if command -v jq &> /dev/null; then
            jq '.' "$f" 2>/dev/null || cat "$f"
        else
            cat "$f"
        fi
    done
    
    echo ""
    echo "Full results saved to: $RUN_DIR"
}

main "$@"
