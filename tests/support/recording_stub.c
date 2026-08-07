/* Stands in for IMAS-Core in tests/tracer_bullet_context_info.c: exports
 * `al_context_info` and `getALVersion` under their real names and
 * signatures, and records what it received instead of doing anything real.
 * `al_status_t` is duplicated here rather than included from the shim's
 * generated header — a real IMAS-Core defines its own copy independently,
 * and this stub should behave the same way. */

#include "recording_stub.h"

#include <stdlib.h>
#include <string.h>

typedef struct {
    int code;
    char message[256];
} al_status_t;

static int g_call_count = 0;
static int g_last_ctx = 0;
static int g_version_query_count = 0;
static char g_al_version[64] = "1.0.0";

al_status_t al_context_info(int ctx, char **info) {
    g_call_count++;
    g_last_ctx = ctx;

    if (info != NULL) {
        static const char text[] = "recording-stub-context-info";
        char *copy = malloc(sizeof text);
        if (copy != NULL) {
            memcpy(copy, text, sizeof text);
        }
        *info = copy;
    }

    al_status_t status;
    status.code = 0;
    memset(status.message, 0, sizeof status.message);
    return status;
}

const char *getALVersion(void) {
    g_version_query_count++;
    return g_al_version;
}

int recording_stub_call_count(void) {
    return g_call_count;
}

int recording_stub_last_ctx(void) {
    return g_last_ctx;
}

int recording_stub_version_query_count(void) {
    return g_version_query_count;
}

void recording_stub_set_al_version(const char *version) {
    strncpy(g_al_version, version, sizeof(g_al_version) - 1);
    g_al_version[sizeof(g_al_version) - 1] = '\0';
}

void recording_stub_reset(void) {
    g_call_count = 0;
    g_last_ctx = 0;
    g_version_query_count = 0;
    strcpy(g_al_version, "1.0.0");
}
