/* Issue #45, ADR 0005: the process-wide HLI DD version latch.
 *
 * `imas_mvdd_set_hli_dd_version` and the `IMAS_MVDD_HLI_DD_VERSION`
 * environment-variable fallback share one `OnceLock`-backed latch that
 * settles for the life of the process. Every scenario below is registered
 * as its own ctest process (see CMakeLists.txt) for exactly the reason
 * runtime_binding_test.c's scenarios are: the latch settles once for the
 * process's lifetime, so a scenario that needs a fresh latch needs a fresh
 * process, not a fresh setenv().
 *
 * The "first open" scenarios call al_begin_dataentry_action against the
 * recording stub — the earliest action any HLI performs — since that is
 * where an unresolved latch settles from the environment variable or to
 * unset (see src/resolve.rs's begin_dataentry_action). */

#include <dlfcn.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <imas_mvdd_loader.h>

#ifndef RECORDING_STUB_PATH
#error "RECORDING_STUB_PATH must be defined by the build (see CMakeLists.txt)"
#endif

#define CHECK(condition)                                                       \
    do {                                                                       \
        if (!(condition)) {                                                    \
            fprintf(stderr, "check failed at %s:%d: %s\n", __FILE__, __LINE__, \
                    #condition);                                               \
            exit(EXIT_FAILURE);                                                \
        }                                                                      \
    } while (0)

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

static int dataentry_call_count(void) {
    void *stub = open_stub_for_introspection();
    int_accessor_fn call_count =
        (int_accessor_fn)dlsym_or_die(stub, "recording_stub_dataentry_call_count");
    return call_count();
}

static al_status_t open_dataentry(void) {
    int dectxID = -1;
    return al_begin_dataentry_action("imas:hdf5?path=/tmp/pulse", 7, &dectxID);
}

/* --- setter scenarios, no "open" involved -------------------------------- */

static void scenario_setter_accepts_valid_version(void) {
    al_status_t status = imas_mvdd_set_hli_dd_version("4.1.1");
    CHECK(status.code == 0);

    printf("hli_dd_version_test setter-accepts-valid-version: latched successfully\n");
}

static void scenario_setter_accepts_identical_repeat(void) {
    CHECK(imas_mvdd_set_hli_dd_version("3.39.0").code == 0);
    CHECK(imas_mvdd_set_hli_dd_version("3.39.0").code == 0);
    CHECK(imas_mvdd_set_hli_dd_version("3.39.0").code == 0);

    printf("hli_dd_version_test setter-accepts-identical-repeat: repeats were silently "
           "accepted\n");
}

static void scenario_setter_rejects_conflicting_repeat(void) {
    CHECK(imas_mvdd_set_hli_dd_version("3.39.0").code == 0);

    al_status_t status = imas_mvdd_set_hli_dd_version("4.1.1");
    CHECK(status.code != 0);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(strstr(status.message, "3.39.0") != NULL);
    CHECK(strstr(status.message, "4.1.1") != NULL);
    CHECK(strstr(status.message, "one process") != NULL);
    CHECK(strstr(status.message, "two") != NULL);

    /* The rejected report must not have overwritten the latch: the original
     * version is still the one in force, so a repeat of it is accepted. */
    CHECK(imas_mvdd_set_hli_dd_version("3.39.0").code == 0);

    printf("hli_dd_version_test setter-rejects-conflicting-repeat: named both versions and "
           "the one-process/two-HLI conflict\n");
}

static void scenario_setter_rejects_invalid_version(void) {
    al_status_t status = imas_mvdd_set_hli_dd_version("not-a-version");
    CHECK(status.code != 0);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);

    /* An invalid report must fail before ever touching the latch: a
     * subsequent valid report still succeeds as a genuine first use. */
    CHECK(imas_mvdd_set_hli_dd_version("4.1.1").code == 0);

    printf("hli_dd_version_test setter-rejects-invalid-version: failed immediately without "
           "poisoning the latch\n");
}

static void scenario_setter_rejects_null_version(void) {
    al_status_t status = imas_mvdd_set_hli_dd_version(NULL);
    CHECK(status.code != 0);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);

    CHECK(imas_mvdd_set_hli_dd_version("4.1.1").code == 0);

    printf("hli_dd_version_test setter-rejects-null-version: failed immediately without "
           "poisoning the latch\n");
}

/* --- setter vs. environment ----------------------------------------------- */

static void scenario_setter_precedes_environment(void) {
    /* IMAS_MVDD_HLI_DD_VERSION is set to a malformed value by CMake for this
     * scenario. If the setter truly takes precedence, that value is never
     * even parsed, so opening a pulse afterwards succeeds. */
    CHECK(imas_mvdd_set_hli_dd_version("4.1.1").code == 0);

    al_status_t status = open_dataentry();
    CHECK(status.code == 0);
    CHECK(dataentry_call_count() == 1);

    printf("hli_dd_version_test setter-precedes-environment: an invalid environment value "
           "was never consulted\n");
}

/* --- first-open resolution (setter never called) --------------------------- */

static void scenario_valid_environment_latches_on_first_open(void) {
    CHECK(dataentry_call_count() == 0);

    al_status_t status = open_dataentry();
    CHECK(status.code == 0);
    CHECK(dataentry_call_count() == 1);

    /* A setter report identical to the environment value is a harmless
     * repeat: the environment's resolution latched it first. */
    CHECK(imas_mvdd_set_hli_dd_version("4.1.1").code == 0);

    printf("hli_dd_version_test valid-environment-latches-on-first-open: the environment "
           "value settled the latch\n");
}

static void scenario_invalid_environment_fails_first_open(void) {
    al_status_t status = open_dataentry();
    CHECK(status.code != 0);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    /* The refusal must happen before IMAS-Core is ever reached. */
    CHECK(dataentry_call_count() == 0);

    printf("hli_dd_version_test invalid-environment-fails-first-open: refused before "
           "reaching IMAS-Core\n");
}

static void scenario_unset_first_open_then_setter_refused(void) {
    al_status_t open_status = open_dataentry();
    CHECK(open_status.code == 0);
    CHECK(dataentry_call_count() == 1);

    al_status_t set_status = imas_mvdd_set_hli_dd_version("4.1.1");
    CHECK(set_status.code != 0);
    CHECK(set_status.code == IMAS_MVDD_CONVERSION_ERROR);

    printf("hli_dd_version_test unset-first-open-then-setter-refused: the setter was "
           "refused after an unset open\n");
}

/* --- thread safety ---------------------------------------------------------- */

#define THREAD_COUNT 8

static void *setter_thread(void *unused) {
    (void)unused;
    al_status_t status = imas_mvdd_set_hli_dd_version("4.1.1");
    return status.code == 0 ? NULL : (void *)(intptr_t)1;
}

static void scenario_concurrent_identical_setters_all_succeed(void) {
    pthread_t threads[THREAD_COUNT];
    for (int i = 0; i < THREAD_COUNT; ++i) {
        CHECK(pthread_create(&threads[i], NULL, setter_thread, NULL) == 0);
    }
    int failures = 0;
    for (int i = 0; i < THREAD_COUNT; ++i) {
        void *result = NULL;
        CHECK(pthread_join(threads[i], &result) == 0);
        if (result != NULL) {
            failures++;
        }
    }
    CHECK(failures == 0);

    printf("hli_dd_version_test concurrent-identical-setters-all-succeed: %d threads agreed "
           "on one version\n",
           THREAD_COUNT);
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr,
                "usage: %s "
                "<setter-accepts-valid-version|setter-accepts-identical-repeat|"
                "setter-rejects-conflicting-repeat|setter-rejects-invalid-version|"
                "setter-rejects-null-version|setter-precedes-environment|"
                "valid-environment-latches-on-first-open|"
                "invalid-environment-fails-first-open|"
                "unset-first-open-then-setter-refused|"
                "concurrent-identical-setters-all-succeed>\n",
                argv[0]);
        return 2;
    }

    const char *scenario = argv[1];
    if (strcmp(scenario, "setter-accepts-valid-version") == 0) {
        scenario_setter_accepts_valid_version();
    } else if (strcmp(scenario, "setter-accepts-identical-repeat") == 0) {
        scenario_setter_accepts_identical_repeat();
    } else if (strcmp(scenario, "setter-rejects-conflicting-repeat") == 0) {
        scenario_setter_rejects_conflicting_repeat();
    } else if (strcmp(scenario, "setter-rejects-invalid-version") == 0) {
        scenario_setter_rejects_invalid_version();
    } else if (strcmp(scenario, "setter-rejects-null-version") == 0) {
        scenario_setter_rejects_null_version();
    } else if (strcmp(scenario, "setter-precedes-environment") == 0) {
        scenario_setter_precedes_environment();
    } else if (strcmp(scenario, "valid-environment-latches-on-first-open") == 0) {
        scenario_valid_environment_latches_on_first_open();
    } else if (strcmp(scenario, "invalid-environment-fails-first-open") == 0) {
        scenario_invalid_environment_fails_first_open();
    } else if (strcmp(scenario, "unset-first-open-then-setter-refused") == 0) {
        scenario_unset_first_open_then_setter_refused();
    } else if (strcmp(scenario, "concurrent-identical-setters-all-succeed") == 0) {
        scenario_concurrent_identical_setters_all_succeed();
    } else {
        fprintf(stderr, "unknown scenario: %s\n", scenario);
        return 2;
    }

    return 0;
}
