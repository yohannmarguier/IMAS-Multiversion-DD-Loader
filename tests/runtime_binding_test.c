/* The tracer bullet from issue #3: drives the shim's exported
 * al_context_info against both a recording stub and a real IMAS-Core.
 * Proves the shim exports al_context_info *and* calls al_context_info, with
 * its own definition never capturing the outbound call.
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
 * The supported and deliberately incompatible versions are supplied from
 * CMake's reading of IMAS_CORE_VERSION, the same pin consumed by build.rs. */

#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <imas_mvdd_loader.h>

#ifndef RECORDING_STUB_PATH
#error "RECORDING_STUB_PATH must be defined by the build (see CMakeLists.txt)"
#endif
#ifndef SUPPORTED_CORE_VERSION
#error "SUPPORTED_CORE_VERSION must come from IMAS_CORE_VERSION"
#endif
#ifndef INCOMPATIBLE_CORE_VERSION
#error "INCOMPATIBLE_CORE_VERSION must be defined by the build"
#endif

typedef int (*int_accessor_fn)(void);

#define CHECK(condition)                                                        \
    do {                                                                        \
        if (!(condition)) {                                                     \
            fprintf(stderr, "check failed at %s:%d: %s\n", __FILE__, __LINE__, \
                    #condition);                                                \
            exit(EXIT_FAILURE);                                                 \
        }                                                                       \
    } while (0)

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

static void check_context_info(int ctx, const char *expected_info) {
    char *info = NULL;
    al_status_t status = al_context_info(ctx, &info);

    CHECK(status.code == 0);
    CHECK(info != NULL);
    CHECK(strcmp(info, expected_info) == 0);
    free(info);
}

static void scenario_success(void) {
    void *stub = open_stub_for_introspection();
    int_accessor_fn call_count = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_call_count");
    int_accessor_fn last_ctx = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_last_ctx");
    int_accessor_fn version_call_count =
        (int_accessor_fn)dlsym_or_die(stub, "recording_stub_version_call_count");
    CHECK(call_count() == 0);

    check_context_info(42, "recording-stub: context info");
    CHECK(call_count() == 1);
    CHECK(last_ctx() == 42);

    /* A second call reaches the stub again without re-resolving. */
    check_context_info(7, "recording-stub: context info");
    CHECK(call_count() == 2);
    CHECK(last_ctx() == 7);
    CHECK(version_call_count() == 1);

    printf("runtime_binding_test success: the shim reached the stub, not itself\n");
}

static void scenario_version_drift(void) {
    void *stub = open_stub_for_introspection();
    int_accessor_fn version_call_count =
        (int_accessor_fn)dlsym_or_die(stub, "recording_stub_version_call_count");

    char *info = NULL;
    al_status_t status = al_context_info(42, &info);
    CHECK(status.code == 0);
    free(info);
    CHECK(version_call_count() == 1);

    char *info2 = NULL;
    al_status_t status2 = al_context_info(7, &info2);
    CHECK(status2.code == 0);
    free(info2);
    CHECK(version_call_count() == 1);

    printf("runtime_binding_test version-drift: recorded and tolerated once\n");
}

static void scenario_version_mismatch(void) {
    void *stub = open_stub_for_introspection();
    int_accessor_fn call_count = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_call_count");

    char *info = NULL;
    al_status_t status = al_context_info(1, &info);

    CHECK(status.code != 0);
    CHECK(strstr(status.message, SUPPORTED_CORE_VERSION) != NULL);
    CHECK(strstr(status.message, INCOMPATIBLE_CORE_VERSION) != NULL);
    CHECK(strstr(status.message, "IMAS_CORE_LIBRARY") != NULL);
    /* The mismatch must fail resolution before ever forwarding the call. */
    CHECK(call_count() == 0);

    printf("runtime_binding_test version-mismatch: resolution failed before forwarding\n");
}

static void scenario_null_version(void) {
    void *stub = open_stub_for_introspection();
    int_accessor_fn call_count = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_call_count");

    char *info = NULL;
    al_status_t status = al_context_info(1, &info);

    CHECK(status.code != 0);
    CHECK(strstr(status.message, "getALVersion") != NULL);
    CHECK(strstr(status.message, "null") != NULL);
    CHECK(strstr(status.message, "IMAS_CORE_LIBRARY") != NULL);
    CHECK(call_count() == 0);

    printf("runtime_binding_test null-version: resolution failed safely\n");
}

static void scenario_missing_library(void) {
    char *info = NULL;
    al_status_t status = al_context_info(1, &info);

    CHECK(status.code != 0);
    CHECK(status.message[0] != '\0');
    CHECK(strstr(status.message, "IMAS_CORE_LIBRARY") != NULL);

    printf("runtime_binding_test missing-library: status=%d message=%s\n", status.code,
           status.message);
}

static void scenario_real_core(void) {
    /* Unlike the other scenarios, this one runs against a real, acquired
     * IMAS-Core (see CMakeLists.txt's IMAS-Core acquisition section), not
     * the recording stub, so there is no introspection handle to open:
     * real IMAS-Core exports no such thing. ctxID 0 is real IMAS-Core's
     * "NULL context" case (al_lowlevel.cpp), the one value it answers
     * deterministically with no context ever having been opened. */
    check_context_info(0, "NULL context");

    /* The same public forwarding path remains stable across repeated calls.
     * Stub-only introspection in scenario_success verifies memoization. */
    check_context_info(0, "NULL context");

    printf("runtime_binding_test real-core: the shim reached real IMAS-Core, not a stub\n");
}

static void scenario_bare_soname(void) {
    /* No IMAS_CORE_LIBRARY override here (see CMakeLists.txt): the shim
     * must locate IMAS-Core by its bare soname through the loader's normal
     * search path, which ctest points at the stub via LD_LIBRARY_PATH /
     * DYLD_LIBRARY_PATH. */
    char *info = NULL;
    al_status_t status = al_context_info(11, &info);
    CHECK(status.code == 0);
    free(info);

    void *stub = open_stub_for_introspection();
    int_accessor_fn last_ctx = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_last_ctx");
    CHECK(last_ctx() == 11);

    printf("runtime_binding_test bare-soname: resolved IMAS-Core through the loader's search path\n");
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr,
                "usage: %s <success|version-drift|version-mismatch|null-version|missing-library|bare-soname|real-core>\n",
                argv[0]);
        return 2;
    }

    const char *scenario = argv[1];
    if (strcmp(scenario, "success") == 0) {
        scenario_success();
    } else if (strcmp(scenario, "real-core") == 0) {
        scenario_real_core();
    } else if (strcmp(scenario, "version-drift") == 0) {
        scenario_version_drift();
    } else if (strcmp(scenario, "version-mismatch") == 0) {
        scenario_version_mismatch();
    } else if (strcmp(scenario, "null-version") == 0) {
        scenario_null_version();
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
