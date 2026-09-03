/* JPEG only. Heap comes from the kernel (`coeleo_malloc`). No SSE. */
#define STB_IMAGE_IMPLEMENTATION
#define STBI_NO_STDIO
#define STBI_NO_SIMD
#define STBI_ONLY_JPEG
#define STBI_NO_LINEAR
#define STBI_NO_HDR
#define STBI_NO_THREAD_LOCALS
#define STBI_NO_FAILURE_STRINGS
#define STBI_ASSERT(x) ((void)0)
#define STBI_MAX_DIMENSIONS 2048
#define STBI_MALLOC(sz) coeleo_malloc(sz)
#define STBI_REALLOC(p, newsz) coeleo_realloc(p, newsz)
#define STBI_FREE(p) coeleo_free(p)

#include <stddef.h>

void *coeleo_malloc(size_t size);
void *coeleo_realloc(void *p, size_t newsz);
void coeleo_free(void *p);

#include "stb_image.h"
