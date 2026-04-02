#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <stdint.h>
#include <unistd.h>
#include <pthread.h>

static inline uint64_t rdtsc(void) {
    unsigned int lo, hi;
    __asm__ __volatile__("rdtsc" : "=a"(lo), "=d"(hi));
    return ((uint64_t)hi << 32) | lo;
}

typedef struct {
    int thread_id;
    int iterations;
    uint64_t total_cycles;
    int alloc_count;
    int free_count;
    int realloc_count;
} bench_thread_t;

void *worker(void *arg) {
    bench_thread_t *t = (bench_thread_t *)arg;
    srand(42 + t->thread_id);

    void *ptrs[1000];
    size_t sizes[1000];
    int active = 0;

    for (int i = 0; i < t->iterations; i++) {
        int action = rand() % 100;
        uint64_t start = rdtsc();

        if (action < 35 && active < 1000) {
            size_t sz = 16 + (rand() % 8192);
            void *ptr = malloc(sz);
            if (ptr) {
                memset(ptr, rand() & 0xFF, sz);
                ptrs[active] = ptr;
                sizes[active] = sz;
                active++;
                t->alloc_count++;
            }
        } else if (action < 70 && active > 0) {
            int idx = rand() % active;
            free(ptrs[idx]);
            ptrs[idx] = ptrs[active - 1];
            sizes[idx] = sizes[active - 1];
            active--;
            t->free_count++;
        } else if (action < 85 && active > 0) {
            int idx = rand() % active;
            size_t new_sz = sizes[idx] * 2;
            void *new_ptr = realloc(ptrs[idx], new_sz);
            if (new_ptr) {
                ptrs[idx] = new_ptr;
                sizes[idx] = new_sz;
                t->realloc_count++;
            }
        } else if (active > 0) {
            int idx = rand() % active;
            void *ptr = malloc(sizes[idx]);
            if (ptr) {
                memcpy(ptr, ptrs[idx], sizes[idx]);
                free(ptrs[idx]);
                ptrs[idx] = ptr;
            }
        }

        uint64_t end = rdtsc();
        t->total_cycles += (end - start);
    }

    for (int i = 0; i < active; i++) {
        free(ptrs[i]);
    }

    return NULL;
}

int main(int argc, char *argv[]) {
    int threads = 8;
    int iterations = 50000;
    if (argc > 1) threads = atoi(argv[1]);
    if (argc > 2) iterations = atoi(argv[2]);

    bench_thread_t *tdata = calloc(threads, sizeof(bench_thread_t));
    pthread_t *pth = malloc(threads * sizeof(pthread_t));

    uint64_t start = rdtsc();

    for (int i = 0; i < threads; i++) {
        tdata[i].thread_id = i;
        tdata[i].iterations = iterations;
        pthread_create(&pth[i], NULL, worker, &tdata[i]);
    }

    for (int i = 0; i < threads; i++) {
        pthread_join(pth[i], NULL);
    }

    uint64_t end = rdtsc();
    uint64_t total_cycles = end - start;
    uint64_t total_ops = 0;
    int total_allocs = 0, total_frees = 0, total_reallocs = 0;

    for (int i = 0; i < threads; i++) {
        total_ops += tdata[i].alloc_count + tdata[i].free_count + tdata[i].realloc_count;
        total_allocs += tdata[i].alloc_count;
        total_frees += tdata[i].free_count;
        total_reallocs += tdata[i].realloc_count;
    }

    double cpu_freq_ghz = 3.5;
    double elapsed_ns = (double)total_cycles / (cpu_freq_ghz * 1e9) * 1e9;
    double ops_per_sec = (double)total_ops / (elapsed_ns / 1e9);
    double avg_ns_per_op = elapsed_ns / total_ops;

    printf("{\"benchmark\": \"mixed_workload\", \"threads\": %d, \"iterations_per_thread\": %d, ", threads, iterations);
    printf("\"total_ops\": %d, \"allocs\": %d, \"frees\": %d, \"reallocs\": %d, ", total_ops, total_allocs, total_frees, total_reallocs);
    printf("\"throughput_ops_per_sec\": %.0f, \"avg_latency_ns\": %.1f, \"elapsed_ns\": %.0f}\n", ops_per_sec, avg_ns_per_op, elapsed_ns);

    free(tdata);
    free(pth);
    return 0;
}
