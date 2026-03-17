#!/bin/bash
set -e

cargo build --release -p aethalloc-abi

gcc -o tests/integration/preload_test tests/integration/preload_test.c

LD_PRELOAD=./target/release/libaethalloc_abi.so ./tests/integration/preload_test

echo "LD_PRELOAD integration test passed!"
