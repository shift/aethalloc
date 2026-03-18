#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

int main(int argc, char **argv) {
    printf("{\"benchmark\": \"massive_allocation\", ");
    
    struct test_case {
        size_t size;
        size_t align;
        const char *desc;
    };
    
    struct test_case tests[] = {
        {256 * 1024 * 1024, 16, "256MB aligned 16B"},
        {512 * 1024 * 1024, 4096, "512MB aligned 4KB"},
        {1024 * 1024 * 1024ULL, 16, "1GB aligned 16B"},
        {1024 * 1024 * 1024ULL, 2 * 1024 * 1024, "1GB aligned 2MB"},
        {2ULL * 1024 * 1024 * 1024, 16, "2GB aligned 16B"},
    };
    
    int num_tests = sizeof(tests) / sizeof(tests[0]);
    int passed = 0;
    int failed = 0;
    
    printf("\"tests\": [");
    
    for (int i = 0; i < num_tests; i++) {
        size_t size = tests[i].size;
        size_t align = tests[i].align;
        
        if (i > 0) printf(", ");
        printf("{\"size_mb\": %zu, \"align\": %zu, ", size / (1024 * 1024), align);
        printf("\"desc\": \"%s\", ", tests[i].desc);
        
        void *ptr = NULL;
        
        if (align <= 16) {
            ptr = malloc(size);
        } else {
#if defined(_ISOC11_SOURCE)
            ptr = aligned_alloc(align, size);
#elif defined(_POSIX_C_SOURCE) && _POSIX_C_SOURCE >= 200112L
            posix_memalign(&ptr, align, size);
#else
            ptr = malloc(size);
#endif
        }
        
        if (ptr == NULL) {
            printf("\"result\": \"NULL\", \"status\": \"FAIL\"}");
            failed++;
            continue;
        }
        
        uintptr_t addr = (uintptr_t)ptr;
        int aligned_ok = (addr % align) == 0;
        
        memset(ptr, 0x55, size);
        
        volatile unsigned char *p = ptr;
        p[0] = 0xAA;
        p[size - 1] = 0xBB;
        
        int verify = (p[0] == 0xAA && p[size - 1] == 0xBB);
        
        printf("\"ptr\": \"%p\", ", ptr);
        printf("\"aligned\": %s, ", aligned_ok ? "true" : "false");
        printf("\"verify\": %s, ", verify ? "true" : "false");
        printf("\"result\": \"OK\", \"status\": \"%s\"}", 
               (aligned_ok && verify) ? "PASS" : "FAIL");
        
        if (aligned_ok && verify) {
            passed++;
        } else {
            failed++;
        }
        
        free(ptr);
    }
    
    printf("], \"passed\": %d, \"failed\": %d, ", passed, failed);
    printf("\"verdict\": \"%s\"}\n", (failed == 0) ? "PASS" : "FAIL");
    
    return (failed == 0) ? 0 : 1;
}
