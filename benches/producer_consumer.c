#include <stdio.h>
#include <stdlib.h>
#include <pthread.h>
#include <stdatomic.h>
#include <string.h>
#include <time.h>

// Producer-Consumer benchmark: Thread A allocates, Thread B frees
// This is the pattern in network packet processing:
// - RX thread allocates packet buffer
// - Worker thread processes and frees buffer

#define NUM_BLOCKS 1000000
#define BLOCK_SIZE 64

typedef struct {
    void **buffers;
    size_t capacity;
    size_t head;
    size_t tail;
    pthread_mutex_t mutex;
    pthread_cond_t not_empty;
    pthread_cond_t not_full;
} Queue;

void queue_init(Queue *q, size_t capacity) {
    q->buffers = malloc(capacity * sizeof(void*));
    q->capacity = capacity;
    q->head = 0;
    q->tail = 0;
    pthread_mutex_init(&q->mutex, NULL);
    pthread_cond_init(&q->not_empty, NULL);
    pthread_cond_init(&q->not_full, NULL);
}

void queue_push(Queue *q, void *ptr) {
    pthread_mutex_lock(&q->mutex);
    while (((q->head + 1) % q->capacity) == q->tail) {
        pthread_cond_wait(&q->not_full, &q->mutex);
    }
    q->buffers[q->head] = ptr;
    q->head = (q->head + 1) % q->capacity;
    pthread_cond_signal(&q->not_empty);
    pthread_mutex_unlock(&q->mutex);
}

void *queue_pop(Queue *q) {
    pthread_mutex_lock(&q->mutex);
    while (q->head == q->tail) {
        pthread_cond_wait(&q->not_empty, &q->mutex);
    }
    void *ptr = q->buffers[q->tail];
    q->tail = (q->tail + 1) % q->capacity;
    pthread_cond_signal(&q->not_full);
    pthread_mutex_unlock(&q->mutex);
    return ptr;
}

static atomic_int produced = 0;
static atomic_int consumed = 0;
static volatile int done = 0;

Queue queue;

void *producer(void *arg) {
    (void)arg;
    
    for (int i = 0; i < NUM_BLOCKS; i++) {
        void *block = malloc(BLOCK_SIZE);
        if (!block) {
            fprintf(stderr, "Producer malloc failed at %d\n", i);
            break;
        }
        
        // Simulate packet data write
        memset(block, 0xAB, BLOCK_SIZE);
        
        queue_push(&queue, block);
        atomic_fetch_add(&produced, 1);
    }
    
    // Signal completion
    queue_push(&queue, NULL);
    
    return NULL;
}

void *consumer(void *arg) {
    (void)arg;
    
    while (1) {
        void *block = queue_pop(&queue);
        if (block == NULL) {
            break;
        }
        
        // Simulate packet processing
        volatile unsigned char sum = 0;
        for (int i = 0; i < BLOCK_SIZE; i++) {
            sum += ((unsigned char*)block)[i];
        }
        (void)sum;
        
        free(block);
        atomic_fetch_add(&consumed, 1);
    }
    
    return NULL;
}

int main(int argc, char *argv[]) {
    int num_producers = 1;
    int num_consumers = 1;
    
    if (argc > 1) {
        num_producers = atoi(argv[1]);
    }
    if (argc > 2) {
        num_consumers = atoi(argv[2]);
    }
    
    printf("Producer-Consumer Benchmark\n");
    printf("Producers: %d, Consumers: %d\n", num_producers, num_consumers);
    printf("Blocks per producer: %d, Block size: %d bytes\n", NUM_BLOCKS, BLOCK_SIZE);
    
    queue_init(&queue, 10000);
    
    pthread_t producers[num_producers];
    pthread_t consumers[num_consumers];
    
    struct timespec start, end;
    clock_gettime(CLOCK_MONOTONIC, &start);
    
    // Start consumers first
    for (int i = 0; i < num_consumers; i++) {
        pthread_create(&consumers[i], NULL, consumer, NULL);
    }
    
    // Start producers
    for (int i = 0; i < num_producers; i++) {
        pthread_create(&producers[i], NULL, producer, NULL);
    }
    
    // Wait for producers
    for (int i = 0; i < num_producers; i++) {
        pthread_join(producers[i], NULL);
    }
    
    // Wait for consumers
    for (int i = 0; i < num_consumers; i++) {
        pthread_join(consumers[i], NULL);
    }
    
    clock_gettime(CLOCK_MONOTONIC, &end);
    
    double elapsed = (end.tv_sec - start.tv_sec) + (end.tv_nsec - start.tv_nsec) / 1e9;
    int total_ops = produced;
    double ops_per_sec = total_ops / elapsed;
    
    printf("{\n");
    printf("  \"benchmark\": \"producer_consumer\",\n");
    printf("  \"producers\": %d,\n", num_producers);
    printf("  \"consumers\": %d,\n", num_consumers);
    printf("  \"blocks_per_producer\": %d,\n", NUM_BLOCKS);
    printf("  \"block_size\": %d,\n", BLOCK_SIZE);
    printf("  \"total_ops\": %d,\n", total_ops);
    printf("  \"elapsed_sec\": %.3f,\n", elapsed);
    printf("  \"throughput_ops_per_sec\": %.0f\n", ops_per_sec);
    printf("}\n");
    
    return 0;
}
