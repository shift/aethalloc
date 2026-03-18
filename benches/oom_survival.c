#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <signal.h>
#include <setjmp.h>
#include <errno.h>

static sigjmp_buf jmp_env;
static volatile int got_signal = 0;

static void sigsegv_handler(int sig) {
    got_signal = 1;
    siglongjmp(jmp_env, 1);
}

int main(void) {
    printf("{\"benchmark\": \"oom_survival\", ");
    
    signal(SIGSEGV, sigsegv_handler);
    
    size_t alloc_size = 100 * 1024 * 1024;
    int successful_allocs = 0;
    int null_returns = 0;
    int crashes = 0;
    void **ptrs = NULL;
    int ptr_count = 0;
    int ptr_capacity = 1000;
    
    ptrs = malloc(ptr_capacity * sizeof(void*));
    if (!ptrs) {
        printf("\"error\": \"failed to allocate pointer array\"}\n");
        return 1;
    }
    
    while (1) {
        if (sigsetjmp(jmp_env, 1) != 0) {
            crashes++;
            break;
        }
        
        void *p = malloc(alloc_size);
        
        if (p == NULL) {
            null_returns++;
            break;
        }
        
        memset(p, 0xAA, alloc_size);
        
        if (ptr_count >= ptr_capacity) {
            ptr_capacity *= 2;
            void **new_ptrs = realloc(ptrs, ptr_capacity * sizeof(void*));
            if (!new_ptrs) {
                null_returns++;
                break;
            }
            ptrs = new_ptrs;
        }
        
        ptrs[ptr_count++] = p;
        successful_allocs++;
        
        if (successful_allocs % 10 == 0) {
            fprintf(stderr, "Allocated %d * 100MB = %d MB\n", 
                    successful_allocs, successful_allocs * 100);
        }
        
        if (successful_allocs >= 100) {
            break;
        }
    }
    
    signal(SIGSEGV, SIG_DFL);
    
    size_t total_allocated = (size_t)successful_allocs * alloc_size;
    
    printf("\"successful_allocs\": %d, ", successful_allocs);
    printf("\"null_returns\": %d, ", null_returns);
    printf("\"crashes\": %d, ", crashes);
    printf("\"total_allocated_mb\": %zu, ", total_allocated / (1024 * 1024));
    printf("\"verdict\": \"");
    
    if (crashes > 0) {
        printf("CRASHED");
    } else if (null_returns > 0 && successful_allocs > 0) {
        printf("PASS\");}\n");
        for (int i = 0; i < ptr_count; i++) {
            free(ptrs[i]);
        }
        free(ptrs);
        return 0;
    } else if (successful_allocs >= 500) {
        printf("PASS (hit limit)\");}\n");
        for (int i = 0; i < ptr_count; i++) {
            free(ptrs[i]);
        }
        free(ptrs);
        return 0;
    } else {
        printf("UNKNOWN\");}\n");
    }
    
    for (int i = 0; i < ptr_count; i++) {
        free(ptrs[i]);
    }
    free(ptrs);
    
    return crashes > 0 ? 1 : 0;
}
