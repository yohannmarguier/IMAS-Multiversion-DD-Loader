/* Stands in for IMAS-Core in tests/runtime_binding_test.c: exports al_context_info and
 * getALVersion under their real names and signatures, and records what it
 * received instead of doing anything real.
 *
 * al_status_t is duplicated here rather than pulled from the shim's
 * generated header: a real IMAS-Core defines its own copy independently of
 * this project's header, and this stub should behave the same way. */

#include <stdlib.h>
#include <string.h>

typedef struct {
    int code;
    char message[256];
} al_status_t;

static int g_call_count = 0;
static int g_last_ctx = 0;

al_status_t al_context_info(int ctx, char **info) {
    g_call_count++;
    g_last_ctx = ctx;

    al_status_t status;
    status.code = 0;
    memset(status.message, 0, sizeof status.message);

    if (info != NULL) {
        static const char reply[] = "recording-stub: context info";
        char *copy = malloc(sizeof reply);
        if (copy != NULL) {
            memcpy(copy, reply, sizeof reply);
        }
        *info = copy;
    }

    return status;
}

/* Defaults to the version runtime_binding_test.c and src/resolve.rs both expect the
 * shim to be built against ("1.0.0"); RECORDING_STUB_VERSION lets a test
 * scenario simulate a different IMAS-Core. */
const char *getALVersion(void) {
    if (getenv("RECORDING_STUB_NULL_VERSION") != NULL) {
        return NULL;
    }
    const char *version_override = getenv("RECORDING_STUB_VERSION");
    return version_override != NULL ? version_override : "1.0.0";
}

/* Introspection accessors below: not part of the mirrored IMAS-Core ABI.
 * tests/runtime_binding_test.c dlsym's these directly rather than linking this stub —
 * see CMakeLists.txt for why. */

int recording_stub_call_count(void) {
    return g_call_count;
}

int recording_stub_last_ctx(void) {
    return g_last_ctx;
}
