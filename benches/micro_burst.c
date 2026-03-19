#define _POSIX_C_SOURCE 200809L
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <time.h>
#include <unistd.h>

#define BURST_SIZE 50000
#define IDLE_TIME_US 500000
#define NUM_CYCLES 10

static uint64_t get_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1000000000ULL + ts.tv_nsec;
}

int main(int argc, char **argv) {
    int burst_size = BURST_SIZE;
    int idle_us = IDLE_TIME_US;
    int cycles = NUM_CYCLES;
    
    if (argc > 1) burst_size = atoi(argv[1]);
    if (argc > 2) idle_us = atoi(argv[2]);
    if (argc > 3) cycles = atoi(argv[3]);
    
    void **pointers = malloc(burst_size * sizeof(void*));
    if (!pointers) {
        fprintf(stderr, "Failed to allocate pointer array\n");
        return 1;
    }
    
    uint64_t cold_latencies = 0;
    uint64_t warm_latencies = 0;
    uint64_t total_alloc_time = 0;
    uint64_t total_free_time = 0;
    int warm_count = 0;
    
    uint64_t benchmark_start = get_ns();
    
    for (int cycle = 0; cycle < cycles; cycle++) {
        struct timespec ts = { .tv_sec = idle_us / 1000000, .tv_nsec = (idle_us % 1000000) * 1000 };
        nanosleep(&ts, NULL);
        
        uint64_t alloc_start = get_ns();
        for (int i = 0; i < burst_size; i++) {
            pointers[i] = malloc(256);
        }
        uint64_t alloc_end = get_ns();
        
        uint64_t alloc_time = alloc_end - alloc_start;
        total_alloc_time += alloc_time;
        
        if (cycle == 0) {
            cold_latencies = alloc_time;
        } else {
            warm_latencies += alloc_time;
            warm_count++;
        }
        
        uint64_t free_start = get_ns();
        for (int i = 0; i < burst_size; i++) {
            free(pointers[i]);
        }
        uint64_t free_end = get_ns();
        total_free_time += (free_end - free_start);
    }
    
    uint64_t benchmark_end = get_ns();
    
    double cold_ns_per_op = (double)cold_latencies / burst_size;
    double warm_ns_per_op = (double)warm_latencies / (warm_count * burst_size);
    double warmup_penalty = ((cold_ns_per_op - warm_ns_per_op) / warm_ns_per_op) * 100.0;
    
    printf("{\"benchmark\": \"micro_burst\", ");
    printf("\"config\": {\"burst_size\": %d, \"idle_us\": %d, \"cycles\": %d}, ", 
           burst_size, idle_us, cycles);
    printf("\"results\": {");
    printf("\"cold_start_ns_per_op\": %.1f, ", cold_ns_per_op);
    printf("\"warm_ns_per_op\": %.1f, ", warm_ns_per_op);
    printf("\"warmup_penalty_pct\": %.1f, ", warmup_penalty);
    printf("\"avg_alloc_ns_per_op\": %.1f, ", 
           (double)total_alloc_time / (cycles * burst_size));
    printf("\"avg_free_ns_per_op\": %.1f, ", 
           (double)total_free_time / (cycles * burst_size));
    printf("\"total_time_sec\": %.3f", 
           (benchmark_end - benchmark_start) / 1e9);
    printf("}}\n");
    
    free(pointers);
    return 0;
}
