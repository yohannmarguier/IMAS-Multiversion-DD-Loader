/* Issue #64: public al_write_data / al_delete_data ABI scenarios against the
 * recording stub.
 *
 * Per docs/adr/0002-read-path-seam-policy.md: "al_write_data, al_delete_data:
 * If known versions differ, return failure without calling IMAS-Core.
 * Otherwise forward unchanged." Unlike al_read_data, no path translation is
 * introduced here — the rule is a blanket refusal keyed only on whether
 * `ctx_id` carries a live conversion record (a known mismatched root, or a
 * child context that inherited one), never on the path argument's content. */

#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <imas_mvdd_loader.h>

#ifndef RECORDING_STUB_PATH
#error "RECORDING_STUB_PATH must be defined by CMakeLists.txt"
#endif

#define CHECK(condition)                                                       \
    do {                                                                       \
        if (!(condition)) {                                                    \
            fprintf(stderr, "check failed at %s:%d: %s\\n", __FILE__, __LINE__, \
                    #condition);                                               \
            exit(EXIT_FAILURE);                                                \
        }                                                                      \
    } while (0)

typedef const char *(*string_accessor_fn)(void);
typedef int (*int_accessor_fn)(void);
typedef const void *(*pointer_accessor_fn)(void);

static void *dlsym_or_die(void *handle, const char *name) {
    void *symbol = dlsym(handle, name);
    if (symbol == NULL) {
        fprintf(stderr, "recording stub has no symbol '%s': %s\\n", name, dlerror());
        abort();
    }
    return symbol;
}

static void *open_stub(void) {
    void *stub = dlopen(RECORDING_STUB_PATH, RTLD_NOW | RTLD_LOCAL);
    if (stub == NULL) {
        fprintf(stderr, "failed to open recording stub: %s\\n", dlerror());
        abort();
    }
    return stub;
}

static const char *string_from_stub(const char *symbol_name) {
    string_accessor_fn accessor = (string_accessor_fn)dlsym_or_die(open_stub(), symbol_name);
    return accessor();
}

static int int_from_stub(const char *symbol_name) {
    int_accessor_fn accessor = (int_accessor_fn)dlsym_or_die(open_stub(), symbol_name);
    return accessor();
}

static const void *pointer_from_stub(const char *symbol_name) {
    pointer_accessor_fn accessor = (pointer_accessor_fn)dlsym_or_die(open_stub(), symbol_name);
    return accessor();
}

static int open_mismatched_equilibrium(void) {
    int pulse_ctx = -1;
    CHECK(al_begin_dataentry_action("imas:hdf5?path=/tmp/pulse", 7, &pulse_ctx).code == 0);

    int operation_ctx = -1;
    CHECK(al_begin_global_action(pulse_ctx, "equilibrium", "", 30, &operation_ctx).code == 0);
    return operation_ctx;
}

static al_status_t write_field(int ctx_id, const char *field, const char *timebase, void *data,
                                int *size) {
    return al_write_data(ctx_id, field, timebase, data, 52 /* DOUBLE_DATA */, 1, size);
}

static void scenario_write_refuses_under_known_mismatched_root_before_core_call(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int writes_before = int_from_stub("recording_stub_write_call_count");

    double sentinel = 42.0;
    void *data = &sentinel;
    int size[1] = {73};

    al_status_t status =
        write_field(operation_ctx, "time_slice/global_quantities/beta_tor_norm", "time", data, size);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(int_from_stub("recording_stub_write_call_count") == writes_before);
    CHECK(data == &sentinel);
    CHECK(sentinel == 42.0);
    CHECK(size[0] == 73);

    printf("write_delete_conversion_test write-refuses-under-known-mismatched-root-before-core-call: "
           "IMAS-Core was never called and the caller's buffers were untouched\\n");
}

static void scenario_delete_refuses_under_known_mismatched_root_before_core_call(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int deletes_before = int_from_stub("recording_stub_delete_call_count");

    al_status_t status =
        al_delete_data(operation_ctx, "time_slice/global_quantities/beta_tor_norm");

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(int_from_stub("recording_stub_delete_call_count") == deletes_before);

    printf("write_delete_conversion_test delete-refuses-under-known-mismatched-root-before-core-call: "
           "IMAS-Core was never called\\n");
}

static void scenario_write_nested_child_context_refuses_through_mismatched_root(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int size = -1;
    int arraystruct_ctx = -1;
    CHECK(al_begin_arraystruct_action(operation_ctx, "time_slice", "", &size, &arraystruct_ctx)
              .code == 0);

    int writes_before = int_from_stub("recording_stub_write_call_count");
    int write_size[1] = {0};
    double sentinel = 1.0;
    al_status_t status =
        write_field(arraystruct_ctx, "global_quantities/beta_tor_norm", "", &sentinel, write_size);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(int_from_stub("recording_stub_write_call_count") == writes_before);

    printf("write_delete_conversion_test write-nested-child-context-refuses-through-mismatched-root: "
           "a child arraystruct context inherited its root's refusal\\n");
}

static void scenario_delete_nested_child_context_refuses_through_mismatched_root(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int size = -1;
    int arraystruct_ctx = -1;
    CHECK(al_begin_arraystruct_action(operation_ctx, "time_slice", "", &size, &arraystruct_ctx)
              .code == 0);

    int deletes_before = int_from_stub("recording_stub_delete_call_count");
    al_status_t status = al_delete_data(arraystruct_ctx, "global_quantities/beta_tor_norm");

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(int_from_stub("recording_stub_delete_call_count") == deletes_before);

    printf("write_delete_conversion_test delete-nested-child-context-refuses-through-mismatched-root: "
           "a child arraystruct context inherited its root's refusal\\n");
}

static void scenario_write_matching_context_forwards_unchanged(void) {
    int operation_ctx = open_mismatched_equilibrium();
    double sentinel = 7.5;
    int size[1] = {1};

    CHECK(write_field(operation_ctx, "time_slice/global_quantities/beta_tor_norm", "time",
                       &sentinel, size)
              .code == 0);

    CHECK(int_from_stub("recording_stub_write_ctx_id") == operation_ctx);
    CHECK(strcmp(string_from_stub("recording_stub_write_field"),
                 "time_slice/global_quantities/beta_tor_norm") == 0);
    CHECK(strcmp(string_from_stub("recording_stub_write_timebase"), "time") == 0);
    CHECK(pointer_from_stub("recording_stub_write_data") == &sentinel);

    printf("write_delete_conversion_test write-matching-context-forwards-unchanged: a matching "
           "stamp was forwarded verbatim\\n");
}

static void scenario_write_unknown_context_forwards_unchanged(void) {
    int pulse_ctx = -1;
    CHECK(al_begin_dataentry_action("imas:hdf5?path=/tmp/pulse", 7, &pulse_ctx).code == 0);
    int operation_ctx = -1;
    CHECK(al_begin_global_action(pulse_ctx, "core_profiles", "", 30, &operation_ctx).code == 0);

    double sentinel = 1.0;
    int size[1] = {1};
    CHECK(write_field(operation_ctx, "profiles_1d/electrons/density", "time", &sentinel, size)
              .code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_write_field"), "profiles_1d/electrons/density") ==
          0);

    printf("write_delete_conversion_test write-unknown-context-forwards-unchanged: an unavailable "
           "artifact was forwarded\\n");
}

static void scenario_write_unstamped_context_forwards_unchanged(void) {
    int operation_ctx = open_mismatched_equilibrium();
    double sentinel = 1.0;
    int size[1] = {1};
    CHECK(write_field(operation_ctx, "time_slice/global_quantities/beta_tor_norm", "time",
                       &sentinel, size)
              .code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_write_field"),
                 "time_slice/global_quantities/beta_tor_norm") == 0);

    printf("write_delete_conversion_test write-unstamped-context-forwards-unchanged: an unstamped "
           "occurrence was forwarded\\n");
}

static void scenario_write_conversion_disabled_forwards_unchanged(void) {
    int operation_ctx = open_mismatched_equilibrium();
    double sentinel = 1.0;
    int size[1] = {1};
    CHECK(write_field(operation_ctx, "time_slice/global_quantities/beta_tor_norm", "time",
                       &sentinel, size)
              .code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_write_field"),
                 "time_slice/global_quantities/beta_tor_norm") == 0);

    printf("write_delete_conversion_test write-conversion-disabled-forwards-unchanged: an unset "
           "HLI version was forwarded\\n");
}

static void scenario_delete_matching_context_forwards_unchanged(void) {
    int operation_ctx = open_mismatched_equilibrium();

    CHECK(al_delete_data(operation_ctx, "time_slice/global_quantities/beta_tor_norm").code == 0);

    CHECK(int_from_stub("recording_stub_delete_ctx") == operation_ctx);
    CHECK(strcmp(string_from_stub("recording_stub_delete_path"),
                 "time_slice/global_quantities/beta_tor_norm") == 0);

    printf("write_delete_conversion_test delete-matching-context-forwards-unchanged: a matching "
           "stamp was forwarded verbatim\\n");
}

static void scenario_delete_unknown_context_forwards_unchanged(void) {
    int pulse_ctx = -1;
    CHECK(al_begin_dataentry_action("imas:hdf5?path=/tmp/pulse", 7, &pulse_ctx).code == 0);
    int operation_ctx = -1;
    CHECK(al_begin_global_action(pulse_ctx, "core_profiles", "", 30, &operation_ctx).code == 0);

    CHECK(al_delete_data(operation_ctx, "profiles_1d/electrons/density").code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_delete_path"), "profiles_1d/electrons/density") ==
          0);

    printf("write_delete_conversion_test delete-unknown-context-forwards-unchanged: an unavailable "
           "artifact was forwarded\\n");
}

static void scenario_delete_unstamped_context_forwards_unchanged(void) {
    int operation_ctx = open_mismatched_equilibrium();

    CHECK(al_delete_data(operation_ctx, "time_slice/global_quantities/beta_tor_norm").code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_delete_path"),
                 "time_slice/global_quantities/beta_tor_norm") == 0);

    printf("write_delete_conversion_test delete-unstamped-context-forwards-unchanged: an unstamped "
           "occurrence was forwarded\\n");
}

static void scenario_delete_conversion_disabled_forwards_unchanged(void) {
    int operation_ctx = open_mismatched_equilibrium();

    CHECK(al_delete_data(operation_ctx, "time_slice/global_quantities/beta_tor_norm").code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_delete_path"),
                 "time_slice/global_quantities/beta_tor_norm") == 0);

    printf("write_delete_conversion_test delete-conversion-disabled-forwards-unchanged: an unset "
           "HLI version was forwarded\\n");
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr,
                "usage: %s <write-refuses-under-known-mismatched-root-before-core-call|"
                "delete-refuses-under-known-mismatched-root-before-core-call|"
                "write-nested-child-context-refuses-through-mismatched-root|"
                "delete-nested-child-context-refuses-through-mismatched-root|"
                "write-matching-context-forwards-unchanged|"
                "write-unknown-context-forwards-unchanged|"
                "write-unstamped-context-forwards-unchanged|"
                "write-conversion-disabled-forwards-unchanged|"
                "delete-matching-context-forwards-unchanged|"
                "delete-unknown-context-forwards-unchanged|"
                "delete-unstamped-context-forwards-unchanged|"
                "delete-conversion-disabled-forwards-unchanged>\\n",
                argv[0]);
        return 2;
    }
    if (strcmp(argv[1], "write-refuses-under-known-mismatched-root-before-core-call") == 0) {
        scenario_write_refuses_under_known_mismatched_root_before_core_call();
        return 0;
    }
    if (strcmp(argv[1], "delete-refuses-under-known-mismatched-root-before-core-call") == 0) {
        scenario_delete_refuses_under_known_mismatched_root_before_core_call();
        return 0;
    }
    if (strcmp(argv[1], "write-nested-child-context-refuses-through-mismatched-root") == 0) {
        scenario_write_nested_child_context_refuses_through_mismatched_root();
        return 0;
    }
    if (strcmp(argv[1], "delete-nested-child-context-refuses-through-mismatched-root") == 0) {
        scenario_delete_nested_child_context_refuses_through_mismatched_root();
        return 0;
    }
    if (strcmp(argv[1], "write-matching-context-forwards-unchanged") == 0) {
        scenario_write_matching_context_forwards_unchanged();
        return 0;
    }
    if (strcmp(argv[1], "write-unknown-context-forwards-unchanged") == 0) {
        scenario_write_unknown_context_forwards_unchanged();
        return 0;
    }
    if (strcmp(argv[1], "write-unstamped-context-forwards-unchanged") == 0) {
        scenario_write_unstamped_context_forwards_unchanged();
        return 0;
    }
    if (strcmp(argv[1], "write-conversion-disabled-forwards-unchanged") == 0) {
        scenario_write_conversion_disabled_forwards_unchanged();
        return 0;
    }
    if (strcmp(argv[1], "delete-matching-context-forwards-unchanged") == 0) {
        scenario_delete_matching_context_forwards_unchanged();
        return 0;
    }
    if (strcmp(argv[1], "delete-unknown-context-forwards-unchanged") == 0) {
        scenario_delete_unknown_context_forwards_unchanged();
        return 0;
    }
    if (strcmp(argv[1], "delete-unstamped-context-forwards-unchanged") == 0) {
        scenario_delete_unstamped_context_forwards_unchanged();
        return 0;
    }
    if (strcmp(argv[1], "delete-conversion-disabled-forwards-unchanged") == 0) {
        scenario_delete_conversion_disabled_forwards_unchanged();
        return 0;
    }
    fprintf(stderr, "unknown scenario: %s\\n", argv[1]);
    return 2;
}
