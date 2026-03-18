/*
 * Benchmark 4: Multi-threaded Allocation Churn
 * 
 * Tests thread-local cache efficiency under parallel load.
 * Demonstrates AethAlloc's lock-free thread-local caching.
 */

#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <time.h>
#include <pthread.h>
#include <stdatomic.h>

#define NUM_THREADS 8
#define OPS_PER_THREAD 500000

static atomic_int total_ops = 0;
static atomic_uint_least64_t total_latency = 0;

static uint64_t get_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1000000000ULL + ts.tv_nsec;
}

static void *worker(void *arg) {
    int thread_id = (int)(intptr_t)arg;
    unsigned int seed = thread_id * 12345;
    
    uint64_t thread_latency = 0;
    int thread_ops = 0;
    
    for (int i = 0; i < OPS_PER_THREAD; i++) {
        uint64_t start = get_ns();
        
        // Variable size allocations (simulate realistic workload)
        size_t size;
        int r = rand_r(&seed) % 100;
        if (r < 50) {
            size = 16 + (rand_r(&seed) % 48);      // 50% tiny
        } else if (r < 80) {
            size = 64 + (rand_r(&seed) % 192);     // 30% small
        } else if (r < 95) {
            size = 256 + (rand_r(&seed) % 768);    // 15% medium
        } else {
            size = 1024 + (rand_r(&seed) % 3072);  // 5% large
        }
        
        void *ptr = malloc(size);
        if (ptr) {
            memset(ptr, 0x42, size < 64 ? size : 64);
            free(ptr);
            thread_ops++;
        }
        
        thread_latency += get_ns() - start;
    }
    
    atomic_fetch_add(&total_ops, thread_ops);
    atomic_fetch_add(&total_latency, thread_latency);
    
    return NULL;
}

int main(int argc, char **argv) {
    int num_threads = NUM_THREADS;
    int ops_per_thread = OPS_PER_THREAD;
    
    if (argc > 1) num_threads = atoi(argv[1]);
    if (argc > 2) ops_per_thread = atoi(argv[2]);
    
    pthread_t *threads = malloc(num_threads * sizeof(pthread_t));
    if (!threads) {
        fprintf(stderr, "Failed to allocate thread array\n");
        return 1;
    }
    
    uint64_t start = get_ns();
    
    // Create threads
    for (int i = 0; i < num_threads; i++) {
        if (pthread_create(&threads[i], NULL, worker, (void *)(intptr_t)i) != 0) {
            fprintf(stderr, "Failed to create thread %d\n", i);
            num_threads = i;
            break;
        }
    }
    
    // Wait for completion
    for (int i = 0; i < num_threads; i++) {
        pthread_join(threads[i], NULL);
    }
    
    uint64_t end = get_ns();
    double elapsed = (end - start) / 1000000000.0;
    
    int ops = atomic_load(&total_ops);
    uint64_t latency = atomic_load(&total_latency);
    
    double throughput = ops / elapsed;
    double avg_latency = (double)latency / ops;
    
    printf("{\"benchmark\": \"multithread_churn\", ");
    printf("\"threads\": %d, ", num_threads);
    printf("\"total_ops\": %d, ", ops);
    printf("\"throughput_ops_per_sec\": %.0f, ", throughput);
    printf("\"avg_latency_ns\": %.1f, ", avg_latency);
    printf("\"elapsed_sec\": %.3f}\n", elapsed);
    
    free(threads);
    return 0;
}
