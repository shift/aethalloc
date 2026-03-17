#include <stdlib.h>
#include <stdio.h>
#include <string.h>

int main() {
    char *buf = malloc(128);
    if (!buf) {
        fprintf(stderr, "malloc failed\n");
        return 1;
    }
    
    memset(buf, 'A', 127);
    buf[127] = '\0';
    
    int *arr = calloc(10, sizeof(int));
    if (!arr) {
        fprintf(stderr, "calloc failed\n");
        free(buf);
        return 1;
    }
    
    for (int i = 0; i < 10; i++) {
        if (arr[i] != 0) {
            fprintf(stderr, "calloc did not zero memory\n");
            free(buf);
            free(arr);
            return 1;
        }
    }
    
    char *buf2 = realloc(buf, 256);
    if (!buf2) {
        fprintf(stderr, "realloc failed\n");
        free(buf);
        free(arr);
        return 1;
    }
    
    if (buf2[0] != 'A') {
        fprintf(stderr, "realloc did not preserve content\n");
        free(buf2);
        free(arr);
        return 1;
    }
    
    free(buf2);
    free(arr);
    
    printf("All tests passed!\n");
    return 0;
}
