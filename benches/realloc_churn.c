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
    int iterations = 100000;
    int grow_factor = 2;
    if (argc > 1) iterations = atoi(argv[1]);
    if (argc > 2) grow_factor = atoi(argv[2]);

    uint64_t *sizes = malloc(iterations * sizeof(uint64_t));
    uint64_t *latencies = malloc(iterations * sizeof(uint64_t));
    void **ptrs = malloc(iterations * sizeof(void *));

    srand(42);

    uint64_t total_cycles = 0;
    int inplace_count = 0;
    int realloc_count = 0;

    for (int i = 0; i < iterations; i++) {
        size_t base_size = 64 + (rand() % 4096);
        sizes[i] = base_size;

        void *ptr = malloc(base_size);
        if (!ptr) {
            fprintf(stderr, "malloc failed at iteration %d\n", i);
            return 1;
        }
        memset(ptr, 0xAB, base_size);

        size_t new_size = base_size * grow_factor;
        uint64_t start = rdtsc();
        void *new_ptr = realloc(ptr, new_size);
        uint64_t end = rdtsc();

        if (!new_ptr) {
            fprintf(stderr, "realloc failed at iteration %d\n", i);
            free(ptr);
            return 1;
        }

        latencies[i] = end - start;
        total_cycles += (end - start);

        if (new_ptr == ptr) {
            inplace_count++;
        }
        ptrs[realloc_count] = new_ptr;
        realloc_count++;

        memset(new_ptr, 0xCD, new_size);
        free(new_ptr);
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
    double inplace_pct = (double)inplace_count / iterations * 100.0;

    printf("{\"benchmark\": \"realloc_churn\", \"iterations\": %d, \"grow_factor\": %d, ", iterations, grow_factor);
    printf("\"latency_cycles\": {\"avg\": %lu, \"min\": %lu, \"max\": %lu}, ", avg_lat, min_lat, max_lat);
    printf("\"latency_ns\": {\"avg\": %.1f, \"min\": %.1f, \"max\": %.1f}, ", avg_ns, min_ns, max_ns);
    printf("\"inplace_expansion_pct\": %.1f}\n", inplace_pct);

    free(sizes);
    free(latencies);
    free(ptrs);
    return 0;
}
