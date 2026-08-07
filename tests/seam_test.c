/* The tracer bullet from issue #3: drives the shim's exported
 * al_context_info and asserts on what a recording stub -- standing in for
 * IMAS-Core -- actually received. Proves the shim exports al_context_info
 * *and* calls al_context_info, with its own definition never capturing the
 * outbound call.
 *
 * The stub is deliberately never linked into this executable: linking it
 * would give the linker two candidate definitions of al_context_info (the
 * shim's and the stub's) to choose between, exactly the ambiguity runtime
 * binding exists to avoid (see docs/adr/0001-runtime-binding-not-linking.md).
 * Instead this test opens the stub with its own dlopen call, purely to read
 * back its recorded state through introspection accessors that are not
 * part of the mirrored ABI. The dynamic loader maps a given shared object
 * once per process no matter how many times it is dlopen'd, so the shim's
 * handle and this test's handle observe the same recorded state.
 *
 * Each scenario is registered as its own ctest process (see CMakeLists.txt)
 * because resolution is cached for the process's lifetime: a scenario that
 * needs a fresh resolution needs a fresh process, not a fresh setenv().
 *
 * The version literals below ("1.0.0", "2.0.0") must agree with
 * BUILT_AGAINST_VERSION in src/resolve.rs and the recording stub's default
 * in tests/stub/recording_stub.c -- kept as plain literals here rather than
 * a shared symbol, so the shim doesn't grow a permanent public export for a
 * test-only need. */

#include <assert.h>
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <imas_mvdd_loader.h>

#ifndef RECORDING_STUB_PATH
#error "RECORDING_STUB_PATH must be defined by the build (see CMakeLists.txt)"
#endif

typedef int (*int_accessor_fn)(void);

static void *dlsym_or_die(void *handle, const char *name) {
    void *symbol = dlsym(handle, name);
    if (symbol == NULL) {
        fprintf(stderr, "recording stub has no symbol '%s': %s\n", name, dlerror());
        abort();
    }
    return symbol;
}

static void *open_stub_for_introspection(void) {
    void *handle = dlopen(RECORDING_STUB_PATH, RTLD_NOW | RTLD_LOCAL);
    if (handle == NULL) {
        fprintf(stderr, "failed to dlopen the recording stub for introspection: %s\n", dlerror());
        abort();
    }
    return handle;
}

static void scenario_success(void) {
    void *stub = open_stub_for_introspection();
    int_accessor_fn call_count = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_call_count");
    int_accessor_fn last_ctx = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_last_ctx");
    assert(call_count() == 0);

    char *info = NULL;
    al_status_t status = al_context_info(42, &info);

    assert(status.code == 0);
    assert(info != NULL);
    free(info);
    assert(call_count() == 1);
    assert(last_ctx() == 42);

    /* A second call reaches the stub again without re-resolving. */
    char *info2 = NULL;
    al_status_t status2 = al_context_info(7, &info2);
    assert(status2.code == 0);
    free(info2);
    assert(call_count() == 2);
    assert(last_ctx() == 7);

    printf("seam_test success: the shim reached the stub, not itself\n");
}

static void scenario_version_mismatch(void) {
    void *stub = open_stub_for_introspection();
    int_accessor_fn call_count = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_call_count");

    char *info = NULL;
    al_status_t status = al_context_info(1, &info);

    assert(status.code != 0);
    assert(strstr(status.message, "1.0.0") != NULL); /* built-against version */
    assert(strstr(status.message, "2.0.0") != NULL); /* stub's reported version */
    assert(strstr(status.message, "IMAS_CORE_LIBRARY") != NULL);
    /* The mismatch must fail resolution before ever forwarding the call. */
    assert(call_count() == 0);

    printf("seam_test version-mismatch: resolution failed before forwarding\n");
}

static void scenario_missing_library(void) {
    char *info = NULL;
    al_status_t status = al_context_info(1, &info);

    assert(status.code != 0);
    assert(status.message[0] != '\0');
    assert(strstr(status.message, "IMAS_CORE_LIBRARY") != NULL);

    printf("seam_test missing-library: status=%d message=%s\n", status.code, status.message);
}

static void scenario_bare_soname(void) {
    /* No IMAS_CORE_LIBRARY override here (see CMakeLists.txt): the shim
     * must locate IMAS-Core by its bare soname through the loader's normal
     * search path, which ctest points at the stub via LD_LIBRARY_PATH /
     * DYLD_LIBRARY_PATH. */
    char *info = NULL;
    al_status_t status = al_context_info(11, &info);
    assert(status.code == 0);
    free(info);

    void *stub = open_stub_for_introspection();
    int_accessor_fn last_ctx = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_last_ctx");
    assert(last_ctx() == 11);

    printf("seam_test bare-soname: resolved IMAS-Core through the loader's search path\n");
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: %s <success|version-mismatch|missing-library|bare-soname>\n", argv[0]);
        return 2;
    }

    const char *scenario = argv[1];
    if (strcmp(scenario, "success") == 0) {
        scenario_success();
    } else if (strcmp(scenario, "version-mismatch") == 0) {
        scenario_version_mismatch();
    } else if (strcmp(scenario, "missing-library") == 0) {
        scenario_missing_library();
    } else if (strcmp(scenario, "bare-soname") == 0) {
        scenario_bare_soname();
    } else {
        fprintf(stderr, "unknown scenario: %s\n", scenario);
        return 2;
    }

    return 0;
}
