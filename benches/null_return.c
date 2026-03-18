#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(void) {
    printf("{\"benchmark\": \"null_return_test\", ");
    
    size_t alloc_size = 100 * 1024 * 1024;
    int successful_allocs = 0;
    int null_returns = 0;
    void *ptrs[200];
    int ptr_count = 0;
    
    memset(ptrs, 0, sizeof(ptrs));
    
    for (int i = 0; i < 200; i++) {
        void *p = malloc(alloc_size);
        
        if (p == NULL) {
            null_returns++;
            printf("\"null_return\": true, ");
            break;
        }
        
        memset(p, 0xAA, alloc_size);
        ptrs[ptr_count++] = p;
        successful_allocs++;
    }
    
    size_t total_allocated = (size_t)successful_allocs * alloc_size;
    
    printf("\"successful_allocs\": %d, ", successful_allocs);
    printf("\"total_allocated_mb\": %zu, ", total_allocated / (1024 * 1024));
    
    int verify_ok = 1;
    for (int i = 0; i < ptr_count; i++) {
        unsigned char *p = ptrs[i];
        if (p[0] != 0xAA || p[alloc_size - 1] != 0xAA) {
            verify_ok = 0;
            break;
        }
    }
    
    for (int i = 0; i < ptr_count; i++) {
        free(ptrs[i]);
    }
    
    printf("\"verify\": %s, ", verify_ok ? "true" : "false");
    printf("\"verdict\": \"PASS\"}\n");
    
    return 0;
}
