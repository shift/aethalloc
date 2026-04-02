#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

static inline uint64_t rdtsc(void) {
    unsigned int lo, hi;
    __asm__ __volatile__("rdtsc" : "=a"(lo), "=d"(hi));
    return ((uint64_t)hi << 32) | lo;
}

int main(int argc, char *argv[]) {
    int iterations = 10000;
    if (argc > 1) iterations = atoi(argv[1]);

    void **ptrs = malloc(iterations * sizeof(void *));
    uint64_t *latencies = malloc(iterations * sizeof(uint64_t));
    int inplace = 0;
    uint64_t total_cycles = 0;

    srand(42);

    for (int i = 0; i < iterations; i++) {
        size_t base = 65536 + (rand() % 262144);
        void *ptr = malloc(base);
        if (!ptr) { fprintf(stderr, "malloc failed\n"); return 1; }
        memset(ptr, 0xAB, base);

        size_t new_size = base * 2;
        uint64_t start = rdtsc();
        void *new_ptr = realloc(ptr, new_size);
        uint64_t end = rdtsc();

        if (!new_ptr) { fprintf(stderr, "realloc failed\n"); free(ptr); return 1; }

        latencies[i] = end - start;
        total_cycles += (end - start);
        if (new_ptr == ptr) inplace++;

        memset(new_ptr, 0xCD, new_size);
        free(new_ptr);
        ptrs[i] = NULL;
    }

    uint64_t min_l = latencies[0], max_l = latencies[0], sum_l = 0;
    for (int i = 0; i < iterations; i++) {
        if (latencies[i] < min_l) min_l = latencies[i];
        if (latencies[i] > max_l) max_l = latencies[i];
        sum_l += latencies[i];
    }

    double cpu_ghz = 3.5;
    printf("{\"benchmark\": \"realloc_large\", \"iterations\": %d, ", iterations);
    printf("\"latency_ns\": {\"avg\": %.1f, \"min\": %.1f, \"max\": %.1f}, ",
           (double)(sum_l/iterations)/(cpu_ghz*1e9)*1e9,
           (double)min_l/(cpu_ghz*1e9)*1e9,
           (double)max_l/(cpu_ghz*1e9)*1e9);
    printf("\"inplace_pct\": %.1f}\n", (double)inplace/iterations*100.0);

    free(ptrs);
    free(latencies);
    return 0;
}
