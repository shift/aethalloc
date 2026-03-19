#define _POSIX_C_SOURCE 200809L
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <time.h>
#include <unistd.h>

#define ALLOC_SIZE (2ULL * 1024 * 1024 * 1024)
#define NUM_CHUNKS 256
#define CHECK_INTERVAL_MS 50
#define MAX_WAIT_MS 5000

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

static uint64_t get_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1000000000ULL + ts.tv_nsec;
}

static void sleep_ms(int ms) {
    struct timespec ts = { .tv_sec = ms / 1000, .tv_nsec = (ms % 1000) * 1000000 };
    nanosleep(&ts, NULL);
}

int main(int argc, char **argv) {
    size_t alloc_size = ALLOC_SIZE;
    int num_chunks = NUM_CHUNKS;
    
    if (argc > 1) alloc_size = strtoull(argv[1], NULL, 10) * 1024 * 1024;
    if (argc > 2) num_chunks = atoi(argv[2]);
    
    size_t chunk_size = alloc_size / num_chunks;
    
    printf("{\"benchmark\": \"rss_reclaim\", ");
    printf("\"config\": {\"total_mb\": %zu, \"chunks\": %d}, ", 
           alloc_size / (1024 * 1024), num_chunks);
    
    long baseline_rss = get_rss_kb();
    printf("\"baseline_rss_kb\": %ld, ", baseline_rss);
    
    void **chunks = malloc(num_chunks * sizeof(void*));
    if (!chunks) {
        printf("\"error\": \"failed to allocate chunk array\"}\n");
        return 1;
    }
    
    uint64_t alloc_start = get_ns();
    for (int i = 0; i < num_chunks; i++) {
        chunks[i] = malloc(chunk_size);
        if (!chunks[i]) {
            printf("\"error\": \"allocation failed at chunk %d\"}\n", i);
            for (int j = 0; j < i; j++) free(chunks[j]);
            free(chunks);
            return 1;
        }
        memset(chunks[i], 0x55, chunk_size);
    }
    uint64_t alloc_end = get_ns();
    
    long peak_rss = get_rss_kb();
    printf("\"peak_rss_kb\": %ld, ", peak_rss);
    printf("\"alloc_time_ms\": %.1f, ", (alloc_end - alloc_start) / 1e6);
    
    uint64_t free_start = get_ns();
    for (int i = 0; i < num_chunks; i++) {
        free(chunks[i]);
    }
    uint64_t free_end = get_ns();
    free(chunks);
    
    printf("\"free_time_ms\": %.1f, ", (free_end - free_start) / 1e6);
    
    long min_rss = peak_rss;
    int wait_ms = 0;
    int reclaim_ms = 0;
    
    while (wait_ms < MAX_WAIT_MS) {
        sleep_ms(CHECK_INTERVAL_MS);
        wait_ms += CHECK_INTERVAL_MS;
        
        long current_rss = get_rss_kb();
        if (current_rss < min_rss) {
            min_rss = current_rss;
            reclaim_ms = wait_ms;
        }
        
        if (current_rss <= baseline_rss * 1.5) {
            break;
        }
    }
    
    long final_rss = get_rss_kb();
    long reclaimed = peak_rss - final_rss;
    long total_allocated = peak_rss - baseline_rss;
    double reclaim_pct = total_allocated > 0 ? 
        (reclaimed * 100.0 / total_allocated) : 0.0;
    
    printf("\"results\": {");
    printf("\"final_rss_kb\": %ld, ", final_rss);
    printf("\"reclaimed_kb\": %ld, ", reclaimed);
    printf("\"reclaim_pct\": %.1f, ", reclaim_pct);
    printf("\"reclaim_time_ms\": %d, ", reclaim_ms);
    printf("\"held_kb\": %ld", final_rss - baseline_rss);
    printf("}}\n");
    
    return 0;
}
