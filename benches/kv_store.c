/*
 * Benchmark 3: Key-Value Store Allocation Patterns
 * 
 * Simulates Redis-like workload with variable-sized keys and values.
 * Tests allocator efficiency for unpredictable size distributions.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <time.h>

#define NUM_KEYS 100000
#define OPERATIONS 1000000

typedef struct {
    char *key;
    size_t key_len;
    char *value;
    size_t value_len;
} KVEntry;

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

// Simulate realistic key/value size distributions
static size_t random_key_size(void) {
    // Keys: 8-64 bytes (mostly small)
    return 8 + (rand() % 56);
}

static size_t random_value_size(void) {
    // Values: highly variable
    // 30% tiny (16-64B), 40% small (128-512B), 20% medium (1-4KB), 10% large (8-64KB)
    int r = rand() % 100;
    if (r < 30) return 16 + (rand() % 48);
    if (r < 70) return 128 + (rand() % 384);
    if (r < 90) return 1024 + (rand() % 3072);
    return 8192 + (rand() % 57344);
}

int main(int argc, char **argv) {
    int ops = OPERATIONS;
    if (argc > 1) ops = atoi(argv[1]);
    
    KVEntry *store = calloc(NUM_KEYS, sizeof(KVEntry));
    if (!store) {
        fprintf(stderr, "Failed to allocate store\n");
        return 1;
    }
    
    srand(12345);
    
    uint64_t start = get_ns();
    long initial_rss = get_rss_kb();
    
    size_t total_data = 0;
    size_t set_ops = 0, get_ops = 0, del_ops = 0;
    uint64_t set_time = 0, get_time = 0, del_time = 0;
    
    for (int i = 0; i < ops; i++) {
        int idx = rand() % NUM_KEYS;
        int op = rand() % 100;
        
        if (op < 60) {
            // SET operation (60%)
            uint64_t t0 = get_ns();
            
            // Free old entry if exists
            if (store[idx].key) {
                total_data -= store[idx].key_len + store[idx].value_len;
                free(store[idx].key);
                free(store[idx].value);
            }
            
            // Allocate new key
            size_t key_len = random_key_size();
            store[idx].key = malloc(key_len + 1);
            if (store[idx].key) {
                memset(store[idx].key, 'K', key_len);
                store[idx].key[key_len] = '\0';
                store[idx].key_len = key_len;
            }
            
            // Allocate new value
            size_t value_len = random_value_size();
            store[idx].value = malloc(value_len + 1);
            if (store[idx].value) {
                memset(store[idx].value, 'V', value_len);
                store[idx].value[value_len] = '\0';
                store[idx].value_len = value_len;
            }
            
            if (store[idx].key && store[idx].value) {
                total_data += key_len + value_len;
            }
            
            set_time += get_ns() - t0;
            set_ops++;
            
        } else if (op < 90) {
            // GET operation (30%)
            uint64_t t0 = get_ns();
            
            if (store[idx].key && store[idx].value) {
                // Simulate reading the value
                volatile char c = store[idx].value[0];
                (void)c;
            }
            
            get_time += get_ns() - t0;
            get_ops++;
            
        } else {
            // DEL operation (10%)
            uint64_t t0 = get_ns();
            
            if (store[idx].key) {
                total_data -= store[idx].key_len + store[idx].value_len;
                free(store[idx].key);
                free(store[idx].value);
                store[idx].key = NULL;
                store[idx].value = NULL;
                store[idx].key_len = 0;
                store[idx].value_len = 0;
            }
            
            del_time += get_ns() - t0;
            del_ops++;
        }
    }
    
    uint64_t end = get_ns();
    long final_rss = get_rss_kb();
    
    double elapsed = (end - start) / 1000000000.0;
    double throughput = ops / elapsed;
    
    printf("{\"benchmark\": \"kv_store\", ");
    printf("\"total_ops\": %d, ", ops);
    printf("\"throughput_ops_per_sec\": %.0f, ", throughput);
    printf("\"operations\": {");
    printf("\"set\": %zu, \"get\": %zu, \"del\": %zu", set_ops, get_ops, del_ops);
    printf("}, ");
    printf("\"latency_ns\": {");
    printf("\"set_avg\": %.1f, ", (double)set_time / set_ops);
    printf("\"get_avg\": %.1f, ", (double)get_time / get_ops);
    printf("\"del_avg\": %.1f", (double)del_time / del_ops);
    printf("}, ");
    printf("\"memory\": {");
    printf("\"rss_kb\": %ld, ", final_rss);
    printf("\"data_bytes\": %zu, ", total_data);
    printf("\"overhead_pct\": %.1f", 
           total_data > 0 ? ((final_rss * 1024.0 - total_data) / total_data * 100) : 0.0);
    printf("}}\n");
    
    // Cleanup
    for (int i = 0; i < NUM_KEYS; i++) {
        if (store[i].key) free(store[i].key);
        if (store[i].value) free(store[i].value);
    }
    free(store);
    
    return 0;
}
