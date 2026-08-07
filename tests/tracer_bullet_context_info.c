/* The tracer bullet from issue #3: drives the shim's exported
 * al_context_info and asserts on what the recording stub — standing in for
 * IMAS-Core — actually received. Proves the shim exports al_context_info
 * *and* calls al_context_info, with its own definition never capturing its
 * outbound call.
 *
 * The recording stub is never linked into this executable: linking it
 * would give the linker two candidate definitions of al_context_info (the
 * shim's and the stub's) to choose between, which is exactly the ambiguity
 * the runtime-binding design exists to avoid (see
 * docs/adr/0001-runtime-binding-not-linking.md). Instead this test dlopen's
 * the stub itself, purely to read back its recorded state through its
 * introspection accessors — the same file the shim resolves at runtime via
 * IMAS_MVDD_LOADER_CORE_LIBRARY, dlopen'd a second time. The dynamic loader
 * maps a given shared object once per process regardless of how many times
 * it's dlopen'd, so both handles see the same recorded state.
 *
 * Each scenario below is registered as its own ctest process (see
 * CMakeLists.txt): the runtime binding is resolved once per process and
 * cached for its lifetime, so scenarios that need a fresh resolution need a
 * fresh process, not a fresh setenv() call.
 *
 * The version compared against below (1.0.0) must match
 * EXPECTED_AL_VERSION in src/binding.rs — kept as a literal here rather
 * than as a shared accessor, so the shim doesn't grow a permanent public
 * symbol for a test-only need. */

#include <assert.h>
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <imas_mvdd_loader.h>

#ifndef RECORDING_STUB_PATH
#error "RECORDING_STUB_PATH must be defined by the build (see CMakeLists.txt)"
#endif

static const char *const EXPECTED_AL_VERSION = "1.0.0";

typedef int (*int_accessor_fn)(void);
typedef void (*set_version_fn)(const char *);
typedef void (*reset_fn)(void);

static int_accessor_fn stub_call_count;
static int_accessor_fn stub_last_ctx;
static int_accessor_fn stub_version_query_count;
static set_version_fn stub_set_al_version;
static reset_fn stub_reset;

static void *dlsym_or_die(void *handle, const char *name) {
    void *symbol = dlsym(handle, name);
    if (symbol == NULL) {
        fprintf(stderr, "missing symbol '%s' in recording stub: %s\n", name, dlerror());
        abort();
    }
    return symbol;
}

static void open_stub_introspection(void) {
    void *handle = dlopen(RECORDING_STUB_PATH, RTLD_NOW | RTLD_LOCAL);
    if (handle == NULL) {
        fprintf(stderr, "failed to dlopen recording stub for introspection: %s\n", dlerror());
        abort();
    }
    stub_call_count = (int_accessor_fn)dlsym_or_die(handle, "recording_stub_call_count");
    stub_last_ctx = (int_accessor_fn)dlsym_or_die(handle, "recording_stub_last_ctx");
    stub_version_query_count =
        (int_accessor_fn)dlsym_or_die(handle, "recording_stub_version_query_count");
    stub_set_al_version = (set_version_fn)dlsym_or_die(handle, "recording_stub_set_al_version");
    stub_reset = (reset_fn)dlsym_or_die(handle, "recording_stub_reset");
}

static void set_core_library_override(const char *value) {
    setenv("IMAS_MVDD_LOADER_CORE_LIBRARY", value, 1);
}

static void scenario_success(void) {
    open_stub_introspection();
    stub_reset();
    set_core_library_override(RECORDING_STUB_PATH);

    char *info_a = NULL;
    al_status_t status_a = al_context_info(7, &info_a);
    assert(status_a.code == 0);
    assert(stub_last_ctx() == 7);
    assert(stub_call_count() == 1);
    free(info_a);

    /* A second call must reach the stub again without re-resolving. */
    char *info_b = NULL;
    al_status_t status_b = al_context_info(9, &info_b);
    assert(status_b.code == 0);
    assert(stub_last_ctx() == 9);
    assert(stub_call_count() == 2);
    assert(stub_version_query_count() == 1);
    free(info_b);

    printf("tracer-bullet-context-info: success scenario passed\n");
}

static void scenario_minor_drift(void) {
    open_stub_introspection();
    stub_reset();
    stub_set_al_version("1.9.9"); /* same major as EXPECTED_AL_VERSION, drifted minor/patch */
    set_core_library_override(RECORDING_STUB_PATH);

    char *info = NULL;
    al_status_t status = al_context_info(3, &info);
    assert(status.code == 0);
    assert(stub_call_count() == 1);
    free(info);

    printf("tracer-bullet-context-info: minor-drift scenario passed\n");
}

static void scenario_major_mismatch(void) {
    open_stub_introspection();
    stub_reset();
    static const char *const mismatched_version = "2.0.0"; /* different major */
    stub_set_al_version(mismatched_version);
    set_core_library_override(RECORDING_STUB_PATH);

    char *info = NULL;
    al_status_t status = al_context_info(3, &info);
    assert(status.code != 0);
    assert(strstr(status.message, EXPECTED_AL_VERSION) != NULL);
    assert(strstr(status.message, mismatched_version) != NULL);
    /* The mismatch must fail resolution before ever forwarding the call. */
    assert(stub_call_count() == 0);

    printf("tracer-bullet-context-info: major-mismatch scenario passed\n");
}

static void scenario_unresolvable(void) {
    static const char *const nonexistent_path = "/nonexistent/path/to/libal-does-not-exist.so";
    set_core_library_override(nonexistent_path);

    char *info = NULL;
    al_status_t status = al_context_info(3, &info);
    assert(status.code != 0);
    /* Names the override variable... */
    assert(strstr(status.message, "IMAS_MVDD_LOADER_CORE_LIBRARY") != NULL);
    /* ...and the underlying failure. */
    assert(strstr(status.message, nonexistent_path) != NULL);

    printf("tracer-bullet-context-info: unresolvable scenario passed\n");
}

static void scenario_bare_soname(void) {
    /* CTest sets the platform loader path before process start. Do not open
     * the stub for introspection until after the shim has resolved it: this
     * ensures the shim itself must locate IMAS-Core by its bare soname. */
    unsetenv("IMAS_MVDD_LOADER_CORE_LIBRARY");

    char *info = NULL;
    al_status_t status = al_context_info(11, &info);
    assert(status.code == 0);
    free(info);

    open_stub_introspection();
    assert(stub_last_ctx() == 11);
    assert(stub_call_count() == 1);
    assert(stub_version_query_count() == 1);

    printf("tracer-bullet-context-info: bare-soname scenario passed\n");
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: %s <success|minor-drift|major-mismatch|unresolvable>\n", argv[0]);
        return 2;
    }

    const char *scenario = argv[1];
    if (strcmp(scenario, "success") == 0) {
        scenario_success();
    } else if (strcmp(scenario, "minor-drift") == 0) {
        scenario_minor_drift();
    } else if (strcmp(scenario, "major-mismatch") == 0) {
        scenario_major_mismatch();
    } else if (strcmp(scenario, "unresolvable") == 0) {
        scenario_unresolvable();
    } else if (strcmp(scenario, "bare-soname") == 0) {
        scenario_bare_soname();
    } else {
        fprintf(stderr, "unknown scenario: %s\n", scenario);
        return 2;
    }

    return 0;
}
