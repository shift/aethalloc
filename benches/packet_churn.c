/*
 * Benchmark 1: Multi-WAN Edge Routing Simulation
 * 
 * Simulates high-frequency packet processing where cache locality is critical.
 * Tests allocation churn while maintaining working set in cache.
 * 
 * Measures P99 latency under various throughput levels.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <stdint.h>

#define WORKING_SET_SIZE (64 * 1024)  // 64KB working set (firewall rules, routing tables)
#define PACKET_BUFFER_SIZE 1536        // MTU-sized buffers
#define MAX_PACKETS 1000000

// Simulated firewall rule (fits in cache line)
typedef struct {
    uint32_t src_ip;
    uint32_t dst_ip;
    uint16_t src_port;
    uint16_t dst_port;
    uint8_t proto;
    uint8_t action;  // 0=drop, 1=accept
    uint8_t pad[2];
} __attribute__((aligned(64))) FirewallRule;

// Simulated packet buffer
typedef struct {
    uint8_t data[PACKET_BUFFER_SIZE];
    uint32_t len;
} PacketBuffer;

static uint64_t get_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1000000000ULL + ts.tv_nsec;
}

static int compare_u64(const void *a, const void *b) {
    uint64_t va = *(const uint64_t *)a;
    uint64_t vb = *(const uint64_t *)b;
    if (va < vb) return -1;
    if (va > vb) return 1;
    return 0;
}

int main(int argc, char **argv) {
    int iterations = 100000;
    int warmup = 10000;
    
    if (argc > 1) iterations = atoi(argv[1]);
    if (argc > 2) warmup = atoi(argv[2]);
    
    // Allocate working set (firewall rules) - should stay in L1/L2
    size_t num_rules = WORKING_SET_SIZE / sizeof(FirewallRule);
    FirewallRule *rules = malloc(num_rules * sizeof(FirewallRule));
    if (!rules) {
        fprintf(stderr, "Failed to allocate rules\n");
        return 1;
    }
    
    // Initialize rules
    for (size_t i = 0; i < num_rules; i++) {
        rules[i].src_ip = 0x0A000000 + i;
        rules[i].dst_ip = 0xC0A80000 + i;
        rules[i].src_port = 1024 + (i % 64512);
        rules[i].dst_port = 80 + (i % 100);
        rules[i].proto = 6;  // TCP
        rules[i].action = (i % 10) ? 1 : 0;  // 90% accept
    }
    
    // Latency samples
    uint64_t *latencies = malloc((iterations + warmup) * sizeof(uint64_t));
    if (!latencies) {
        fprintf(stderr, "Failed to allocate latency array\n");
        free(rules);
        return 1;
    }
    
    // Warmup
    for (int i = 0; i < warmup; i++) {
        PacketBuffer *pkt = malloc(sizeof(PacketBuffer));
        if (!pkt) continue;
        pkt->len = 64 + (rand() % 1400);
        memset(pkt->data, 0, pkt->len);
        
        // Simulate rule lookup (cache-sensitive)
        volatile uint32_t sum = 0;
        for (size_t r = 0; r < num_rules; r++) {
            sum += rules[r].src_ip ^ rules[r].dst_ip;
        }
        (void)sum;
        
        free(pkt);
    }
    
    // Main benchmark
    uint64_t start_total = get_ns();
    
    for (int i = 0; i < iterations; i++) {
        uint64_t start = get_ns();
        
        // Allocate packet buffer (this should NOT evict rules from cache)
        PacketBuffer *pkt = malloc(sizeof(PacketBuffer));
        if (!pkt) continue;
        
        pkt->len = 64 + (rand() % 1400);
        memset(pkt->data, 0, pkt->len);
        
        // Simulate firewall rule evaluation (working set access)
        volatile uint32_t action = 0;
        for (size_t r = 0; r < num_rules; r++) {
            if (rules[r].proto == 6) {
                action = rules[r].action;
            }
        }
        (void)action;
        
        free(pkt);
        
        latencies[i] = get_ns() - start;
    }
    
    uint64_t end_total = get_ns();
    
    // Sort latencies for percentile calculation
    qsort(latencies, iterations, sizeof(uint64_t), compare_u64);
    
    uint64_t p50 = latencies[iterations / 2];
    uint64_t p95 = latencies[iterations * 95 / 100];
    uint64_t p99 = latencies[iterations * 99 / 100];
    uint64_t p999 = latencies[iterations * 999 / 1000];
    
    double throughput = (double)iterations * 1000000000.0 / (end_total - start_total);
    
    printf("{\"benchmark\": \"packet_churn\", \"iterations\": %d, ", iterations);
    printf("\"throughput_ops_per_sec\": %.0f, ", throughput);
    printf("\"latency_ns\": {\"p50\": %lu, \"p95\": %lu, \"p99\": %lu, \"p99.9\": %lu}}\n",
           p50, p95, p99, p999);
    
    free(latencies);
    free(rules);
    return 0;
}
