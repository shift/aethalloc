#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <stdint.h>
#include <pthread.h>
#include <stdatomic.h>

#define NSEC_PER_SEC 1000000000ULL

static inline uint64_t get_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * NSEC_PER_SEC + ts.tv_nsec;
}

static int compare_u64(const void *a, const void *b) {
    uint64_t va = *(const uint64_t *)a;
    uint64_t vb = *(const uint64_t *)b;
    if (va < vb) return -1;
    if (va > vb) return 1;
    return 0;
}

typedef struct {
    int thread_id;
    int iterations;
    uint64_t *latencies;
    int sample_count;
    atomic_int *stop;
} thread_args_t;

static void *latency_worker(void *arg) {
    thread_args_t *args = (thread_args_t *)arg;
    uint64_t *local_latencies = malloc(args->iterations * sizeof(uint64_t));
    int local_count = 0;
    
    for (int i = 0; i < args->iterations && !atomic_load(args->stop); i++) {
        uint64_t start = get_ns();
        void *p = malloc(64);
        uint64_t after_alloc = get_ns();
        free(p);
        uint64_t end = get_ns();
        
        local_latencies[local_count++] = after_alloc - start;
    }
    
    args->latencies = local_latencies;
    args->sample_count = local_count;
    return NULL;
}

int main(int argc, char **argv) {
    int num_threads = 8;
    int iterations_per_thread = 100000;
    
    if (argc > 1) num_threads = atoi(argv[1]);
    if (argc > 2) iterations_per_thread = atoi(argv[2]);
    
    printf("{\"benchmark\": \"tail_latency\", \"threads\": %d, \"iterations_per_thread\": %d",
           num_threads, iterations_per_thread);
    
    pthread_t threads[num_threads];
    thread_args_t args[num_threads];
    atomic_int stop = 0;
    
    for (int i = 0; i < num_threads; i++) {
        args[i].thread_id = i;
        args[i].iterations = iterations_per_thread;
        args[i].stop = &stop;
        pthread_create(&threads[i], NULL, latency_worker, &args[i]);
    }
    
    for (int i = 0; i < num_threads; i++) {
        pthread_join(threads[i], NULL);
    }
    
    int total_samples = 0;
    for (int i = 0; i < num_threads; i++) {
        total_samples += args[i].sample_count;
    }
    
    uint64_t *all_latencies = malloc(total_samples * sizeof(uint64_t));
    int idx = 0;
    for (int i = 0; i < num_threads; i++) {
        memcpy(all_latencies + idx, args[i].latencies, args[i].sample_count * sizeof(uint64_t));
        idx += args[i].sample_count;
        free(args[i].latencies);
    }
    
    qsort(all_latencies, total_samples, sizeof(uint64_t), compare_u64);
    
    uint64_t p50 = all_latencies[total_samples / 2];
    uint64_t p90 = all_latencies[total_samples * 90 / 100];
    uint64_t p95 = all_latencies[total_samples * 95 / 100];
    uint64_t p99 = all_latencies[total_samples * 99 / 100];
    uint64_t p999 = all_latencies[total_samples * 999 / 1000];
    uint64_t p9999 = all_latencies[total_samples * 9999 / 10000];
    uint64_t max_lat = all_latencies[total_samples - 1];
    
    printf(", \"samples\": %d", total_samples);
    printf(", \"latency_ns\": {");
    printf("\"min\": %llu", (unsigned long long)all_latencies[0]);
    printf(", \"p50\": %llu", (unsigned long long)p50);
    printf(", \"p90\": %llu", (unsigned long long)p90);
    printf(", \"p95\": %llu", (unsigned long long)p95);
    printf(", \"p99\": %llu", (unsigned long long)p99);
    printf(", \"p99.9\": %llu", (unsigned long long)p999);
    printf(", \"p99.99\": %llu", (unsigned long long)p9999);
    printf(", \"max\": %llu", (unsigned long long)max_lat);
    printf("}}\n");
    
    free(all_latencies);
    return 0;
}
