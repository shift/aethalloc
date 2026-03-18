#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(void) {
    printf("Step 1: Simple malloc/free\n");
    void *p1 = malloc(64);
    printf("  malloc(64) = %p\n", p1);
    memset(p1, 0xAA, 64);
    free(p1);
    printf("  free OK\n");
    
    printf("Step 2: Multiple sizes\n");
    for (int i = 16; i <= 65536; i *= 2) {
        void *p = malloc(i);
        printf("  malloc(%d) = %p\n", i, p);
        if (p) {
            memset(p, 0xBB, i);
            free(p);
            printf("  free OK\n");
        }
    }
    
    printf("Step 3: Large allocation\n");
    void *p3 = malloc(1024 * 1024);
    printf("  malloc(1MB) = %p\n", p3);
    if (p3) {
        memset(p3, 0xCC, 1024 * 1024);
        free(p3);
        printf("  free OK\n");
    }
    
    printf("All simple tests passed!\n");
    return 0;
}
