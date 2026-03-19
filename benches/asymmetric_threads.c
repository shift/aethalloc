#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <pthread.h>
#include <time.h>

#define NUM_OBJECTS 10000
#define NUM_PRODUCER_THREADS 4
#define NUM_CONSUMER_THREADS 4
#define OBJECT_SIZE 128

typedef struct {
    void *data;
    volatile int ready;
} Object;

typedef struct {
    Object *objects;
    int count;
    int producer_idx;
    int consumer_idx;
    pthread_mutex_t lock;
    pthread_cond_t not_empty;
    pthread_cond_t not_full;
    volatile int done;
} SharedQueue;

static uint64_t get_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1000000000ULL + ts.tv_nsec;
}

static long get_rss_kb(void) {
    FILE *f = fopen("/proc/self/statm", "r");
    if (!f) return -1;
    long size, rss;
    if (fscanf(f, "%ld %ld", &size, &rss) != 2) {
        fclose(f);
        return -1;
    }
    fclose(f);
    return rss * 4;
}

static void *producer_thread(void *arg) {
    SharedQueue *queue = (SharedQueue *)arg;
    uint64_t alloc_time = 0;
    int allocs = 0;
    
    for (int i = 0; i < queue->count / NUM_PRODUCER_THREADS; i++) {
        uint64_t t0 = get_ns();
        void *obj = malloc(OBJECT_SIZE);
        uint64_t t1 = get_ns();
        
        if (!obj) continue;
        
        memset(obj, 0xAA, OBJECT_SIZE);
        alloc_time += (t1 - t0);
        allocs++;
        
        pthread_mutex_lock(&queue->lock);
        while (((queue->producer_idx + 1) % queue->count) == queue->consumer_idx) {
            pthread_cond_wait(&queue->not_full, &queue->lock);
        }
        
        queue->objects[queue->producer_idx].data = obj;
        queue->objects[queue->producer_idx].ready = 1;
        queue->producer_idx = (queue->producer_idx + 1) % queue->count;
        
        pthread_cond_signal(&queue->not_empty);
        pthread_mutex_unlock(&queue->lock);
    }
    
    return (void *)alloc_time;
}

static void *consumer_thread(void *arg) {
    SharedQueue *queue = (SharedQueue *)arg;
    uint64_t free_time = 0;
    int frees = 0;
    
    int consumed = 0;
    int to_consume = queue->count / NUM_CONSUMER_THREADS;
    
    while (consumed < to_consume && !queue->done) {
        pthread_mutex_lock(&queue->lock);
        
        while (queue->consumer_idx == queue->producer_idx && !queue->done) {
            pthread_cond_wait(&queue->not_empty, &queue->lock);
        }
        
        if (queue->consumer_idx == queue->producer_idx) {
            pthread_mutex_unlock(&queue->lock);
            break;
        }
        
        void *obj = queue->objects[queue->consumer_idx].data;
        queue->objects[queue->consumer_idx].ready = 0;
        queue->consumer_idx = (queue->consumer_idx + 1) % queue->count;
        
        pthread_cond_signal(&queue->not_full);
        pthread_mutex_unlock(&queue->lock);
        
        if (obj) {
            uint64_t t0 = get_ns();
            free(obj);
            uint64_t t1 = get_ns();
            free_time += (t1 - t0);
            frees++;
        }
        consumed++;
    }
    
    return (void *)free_time;
}

int main(int argc, char **argv) {
    int num_objects = NUM_OBJECTS;
    int num_producers = NUM_PRODUCER_THREADS;
    int num_consumers = NUM_CONSUMER_THREADS;
    
    if (argc > 1) num_objects = atoi(argv[1]);
    if (argc > 2) num_producers = atoi(argv[2]);
    if (argc > 3) num_consumers = atoi(argv[3]);
    
    long baseline_rss = get_rss_kb();
    
    SharedQueue queue = {
        .count = num_objects * 2,
        .producer_idx = 0,
        .consumer_idx = 0,
        .done = 0
    };
    
    queue.objects = calloc(queue.count, sizeof(Object));
    if (!queue.objects) {
        fprintf(stderr, "Failed to allocate queue\n");
        return 1;
    }
    
    pthread_mutex_init(&queue.lock, NULL);
    pthread_cond_init(&queue.not_empty, NULL);
    pthread_cond_init(&queue.not_full, NULL);
    
    pthread_t producers[num_producers];
    pthread_t consumers[num_consumers];
    
    uint64_t start = get_ns();
    
    for (int i = 0; i < num_consumers; i++) {
        pthread_create(&consumers[i], NULL, consumer_thread, &queue);
    }
    
    for (int i = 0; i < num_producers; i++) {
        pthread_create(&producers[i], NULL, producer_thread, &queue);
    }
    
    uint64_t total_alloc_time = 0;
    uint64_t total_free_time = 0;
    
    for (int i = 0; i < num_producers; i++) {
        void *result;
        pthread_join(producers[i], &result);
        total_alloc_time += (uint64_t)result;
    }
    
    queue.done = 1;
    pthread_cond_broadcast(&queue.not_empty);
    
    for (int i = 0; i < num_consumers; i++) {
        void *result;
        pthread_join(consumers[i], &result);
        total_free_time += (uint64_t)result;
    }
    
    uint64_t end = get_ns();
    
    long peak_rss = get_rss_kb();
    
    long final_rss = get_rss_kb();
    
    double elapsed = (end - start) / 1e9;
    int total_ops = num_objects;
    
    printf("{\"benchmark\": \"asymmetric_threads\", ");
    printf("\"config\": {\"objects\": %d, \"producers\": %d, \"consumers\": %d}, ",
           num_objects, num_producers, num_consumers);
    printf("\"results\": {");
    printf("\"throughput_ops_per_sec\": %.0f, ", total_ops / elapsed);
    printf("\"avg_alloc_ns\": %.1f, ", (double)total_alloc_time / total_ops);
    printf("\"avg_free_ns\": %.1f, ", (double)total_free_time / total_ops);
    printf("\"total_time_sec\": %.3f, ", elapsed);
    printf("\"baseline_rss_kb\": %ld, ", baseline_rss);
    printf("\"peak_rss_kb\": %ld, ", peak_rss);
    printf("\"final_rss_kb\": %ld, ", final_rss);
    printf("\"memory_retained_kb\": %ld", final_rss - baseline_rss);
    printf("}}\n");
    
    free(queue.objects);
    pthread_mutex_destroy(&queue.lock);
    pthread_cond_destroy(&queue.not_empty);
    pthread_cond_destroy(&queue.not_full);
    
    return 0;
}
