#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(void) {
    printf("Testing allocation patterns...\n");
    
    for (int round = 0; round < 100; round++) {
        void *ptrs[1000];
        int count = 0;
        
        for (int i = 0; i < 1000; i++) {
            size_t size = 16 + (i % 64);
            ptrs[i] = malloc(size);
            if (ptrs[i]) {
                memset(ptrs[i], 0xAA, size);
                count++;
            }
        }
        
        for (int i = 0; i < 1000; i++) {
            if (ptrs[i]) free(ptrs[i]);
        }
        
        if (round % 10 == 0) printf("Round %d complete (%d allocs)\n", round, count);
    }
    
    printf("Basic test passed\n");
    
    printf("Testing large allocations...\n");
    for (int i = 0; i < 50; i++) {
        void *p = malloc(1024 * 1024);
        if (p) {
            memset(p, 0xBB, 1024 * 1024);
            free(p);
        }
    }
    printf("Large allocation test passed\n");
    
    printf("Testing aligned allocations...\n");
    for (int i = 0; i < 100; i++) {
        size_t align = 1 << (5 + (i % 10));
        size_t size = 64 + (i * 1024);
        void *p = NULL;
        if (posix_memalign(&p, align, size) == 0) {
            memset(p, 0xCC, size);
            free(p);
        }
    }
    printf("Aligned allocation test passed\n");
    
    printf("All tests passed!\n");
    return 0;
}
