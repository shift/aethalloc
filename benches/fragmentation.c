/*
 * Benchmark 2: Memory Fragmentation Simulation
 * 
 * Simulates long-running server workload with variable-sized allocations.
 * Measures memory efficiency after sustained allocation churn.
 * 
 * Tests AethAlloc's ability to maintain memory efficiency over time.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <time.h>

#define NUM_SLOTS 10000
#define ITERATIONS 1000000

typedef struct {
    void *ptr;
    size_t size;
} Slot;

static uint64_t get_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1000000000ULL + ts.tv_nsec;
}

// Get current RSS (resident set size) in KB
static long get_rss_kb(void) {
    FILE *f = fopen("/proc/self/statm", "r");
    if (!f) return -1;
    
    long size, rss;
    if (fscanf(f, "%ld %ld", &size, &rss) != 2) {
        fclose(f);
        return -1;
    }
    fclose(f);
    
    // rss is in pages, convert to KB
    return rss * 4;  // Assuming 4KB pages
}

int main(int argc, char **argv) {
    int iterations = ITERATIONS;
    int report_interval = 100000;
    
    if (argc > 1) iterations = atoi(argv[1]);
    if (argc > 2) report_interval = atoi(argv[2]);
    
    Slot *slots = calloc(NUM_SLOTS, sizeof(Slot));
    if (!slots) {
        fprintf(stderr, "Failed to allocate slots\n");
        return 1;
    }
    
    srand(42);  // Deterministic for reproducibility
    
    long initial_rss = get_rss_kb();
    uint64_t start = get_ns();
    
    printf("{\"benchmark\": \"fragmentation\", \"iterations\": %d, \"samples\": [\n", iterations);
    
    size_t total_allocated = 0;
    size_t peak_allocated = 0;
    
    for (int i = 0; i < iterations; i++) {
        int idx = rand() % NUM_SLOTS;
        
        // Free existing allocation at this slot
        if (slots[idx].ptr) {
            total_allocated -= slots[idx].size;
            free(slots[idx].ptr);
            slots[idx].ptr = NULL;
            slots[idx].size = 0;
        }
        
        // Allocate new variable-sized block
        // Simulate realistic size distribution:
        // - 40% tiny (16-128 bytes) - small strings, objects
        // - 30% small (256-2KB) - small buffers
        // - 20% medium (4KB-64KB) - medium buffers
        // - 10% large (128KB-1MB) - large buffers
        size_t size;
        int r = rand() % 100;
        if (r < 40) {
            size = 16 + (rand() % 112);
        } else if (r < 70) {
            size = 256 + (rand() % 1792);
        } else if (r < 90) {
            size = 4096 + (rand() % 61440);
        } else {
            size = 131072 + (rand() % 900000);
        }
        
        void *ptr = malloc(size);
        if (ptr) {
            // Touch the memory to ensure it's really allocated
            memset(ptr, 0x42, size < 256 ? size : 256);
            slots[idx].ptr = ptr;
            slots[idx].size = size;
            total_allocated += size;
            if (total_allocated > peak_allocated) {
                peak_allocated = total_allocated;
            }
        }
        
        // Report periodically
        if ((i + 1) % report_interval == 0) {
            long rss = get_rss_kb();
            double efficiency = (double)total_allocated / (rss * 1024) * 100.0;
            
            if (i + 1 < iterations) {
                printf("  {\"iteration\": %d, \"rss_kb\": %ld, \"allocated_bytes\": %zu, \"efficiency_pct\": %.1f},\n",
                       i + 1, rss, total_allocated, efficiency);
            } else {
                printf("  {\"iteration\": %d, \"rss_kb\": %ld, \"allocated_bytes\": %zu, \"efficiency_pct\": %.1f}\n",
                       i + 1, rss, total_allocated, efficiency);
            }
        }
    }
    
    uint64_t end = get_ns();
    double elapsed_sec = (end - start) / 1000000000.0;
    
    // Final cleanup
    for (int i = 0; i < NUM_SLOTS; i++) {
        if (slots[i].ptr) {
            free(slots[i].ptr);
        }
    }
    
    long final_rss = get_rss_kb();
    
    printf("], \"summary\": {");
    printf("\"total_time_sec\": %.3f, ", elapsed_sec);
    printf("\"ops_per_sec\": %.0f, ", iterations / elapsed_sec);
    printf("\"initial_rss_kb\": %ld, ", initial_rss);
    printf("\"final_rss_kb\": %ld, ", final_rss);
    printf("\"peak_allocated_bytes\": %zu, ", peak_allocated);
    printf("\"rss_growth_kb\": %ld", final_rss - initial_rss);
    printf("}}\n");
    
    free(slots);
    return 0;
}
