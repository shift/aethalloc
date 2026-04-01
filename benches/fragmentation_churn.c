#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <stdint.h>
#include <unistd.h>

static inline uint64_t rdtsc(void) {
    unsigned int lo, hi;
    __asm__ __volatile__("rdtsc" : "=a"(lo), "=d"(hi));
    return ((uint64_t)hi << 32) | lo;
}

int main(int argc, char *argv[]) {
    int iterations = 50000;
    int max_allocs = 10000;
    if (argc > 1) iterations = atoi(argv[1]);
    if (argc > 2) max_allocs = atoi(argv[2]);

    void **allocs = calloc(max_allocs, sizeof(void *));
    size_t *sizes = calloc(max_allocs, sizeof(size_t));
    uint64_t *latencies = malloc(iterations * sizeof(uint64_t));

    srand(42);

    int active = 0;
    uint64_t total_cycles = 0;
    uint64_t rss_before = 0, rss_after = 0;

    for (int i = 0; i < iterations; i++) {
        int action = rand() % 100;

        uint64_t start = rdtsc();

        if (action < 40 && active < max_allocs) {
            size_t sz = 256 + (rand() % 65536);
            void *ptr = malloc(sz);
            if (ptr) {
                memset(ptr, rand() & 0xFF, sz);
                allocs[active] = ptr;
                sizes[active] = sz;
                active++;
            }
        } else if (action < 80 && active > 0) {
            int idx = rand() % active;
            free(allocs[idx]);
            allocs[idx] = allocs[active - 1];
            sizes[idx] = sizes[active - 1];
            active--;
        } else if (active > 0) {
            int idx = rand() % active;
            size_t new_sz = sizes[idx] * (1 + (rand() % 3));
            void *new_ptr = realloc(allocs[idx], new_sz);
            if (new_ptr) {
                allocs[idx] = new_ptr;
                sizes[idx] = new_sz;
            }
        }

        uint64_t end = rdtsc();
        latencies[i] = end - start;
        total_cycles += (end - start);
    }

    for (int i = 0; i < active; i++) {
        free(allocs[i]);
    }

    uint64_t min_lat = latencies[0], max_lat = latencies[0], sum_lat = 0;
    for (int i = 0; i < iterations; i++) {
        if (latencies[i] < min_lat) min_lat = latencies[i];
        if (latencies[i] > max_lat) max_lat = latencies[i];
        sum_lat += latencies[i];
    }
    uint64_t avg_lat = sum_lat / iterations;

    double cpu_freq_ghz = 3.5;
    double avg_ns = (double)avg_lat / (cpu_freq_ghz * 1e9) * 1e9;
    double min_ns = (double)min_lat / (cpu_freq_ghz * 1e9) * 1e9;
    double max_ns = (double)max_lat / (cpu_freq_ghz * 1e9) * 1e9;

    printf("{\"benchmark\": \"fragmentation_churn\", \"iterations\": %d, \"max_allocs\": %d, ", iterations, max_allocs);
    printf("\"latency_cycles\": {\"avg\": %lu, \"min\": %lu, \"max\": %lu}, ", avg_lat, min_lat, max_lat);
    printf("\"latency_ns\": {\"avg\": %.1f, \"min\": %.1f, \"max\": %.1f}}\n", avg_ns, min_ns, max_ns);

    free(allocs);
    free(sizes);
    free(latencies);
    return 0;
}
