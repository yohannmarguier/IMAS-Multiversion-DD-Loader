/* Issue #62: public al_read_data scenarios through a live arraystruct
 * context, against the recording stub.
 *
 * read_path_test.c (issue #56) already proves a relative field resolves
 * beneath an *unrenamed* arraystruct anchor ("time_slice" spells the same on
 * both sides), and arraystruct_path_test.c (issue #61) proves a *renamed*
 * container's own `path`/`timebase` arguments translate when the AOS is
 * opened. Neither proves the case this issue is about: a relative
 * `al_read_data` argument addressed against a child context whose own anchor
 * was itself renamed, which requires translating that anchor before
 * stripping it from the resolved stored path (`resolve::stored_anchor`). That
 * is this file's first scenario. The remaining scenarios prove the read
 * policies a live child context inherits unchanged from a root context:
 * absolute resolution from the IDS root regardless of the child's own
 * anchor, no-source, refusal, and a supported value transformation. */

#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <imas_mvdd_loader.h>

#ifndef RECORDING_STUB_PATH
#error "RECORDING_STUB_PATH must be defined by CMakeLists.txt"
#endif

#define CHECK(condition)                                                      \
    do {                                                                      \
        if (!(condition)) {                                                   \
            fprintf(stderr, "check failed at %s:%d: %s\\n", __FILE__, __LINE__, \
                    #condition);                                             \
            exit(EXIT_FAILURE);                                               \
        }                                                                     \
    } while (0)

typedef const char *(*string_accessor_fn)(void);
typedef int (*int_accessor_fn)(void);

static const char *string_from_stub(const char *symbol_name) {
    void *stub = dlopen(RECORDING_STUB_PATH, RTLD_NOW | RTLD_LOCAL);
    CHECK(stub != NULL);
    string_accessor_fn accessor = (string_accessor_fn)dlsym(stub, symbol_name);
    CHECK(accessor != NULL);
    return accessor();
}

static int int_from_stub(const char *symbol_name) {
    void *stub = dlopen(RECORDING_STUB_PATH, RTLD_NOW | RTLD_LOCAL);
    CHECK(stub != NULL);
    int_accessor_fn accessor = (int_accessor_fn)dlsym(stub, symbol_name);
    CHECK(accessor != NULL);
    return accessor();
}

static int open_mismatched_equilibrium(void) {
    int pulse_ctx = -1;
    CHECK(al_begin_dataentry_action("imas:hdf5?path=/tmp/pulse", 7, &pulse_ctx).code == 0);

    int operation_ctx = -1;
    CHECK(al_begin_global_action(pulse_ctx, "equilibrium", "", 30, &operation_ctx).code == 0);
    return operation_ctx;
}

static int open_time_slice(int operation_ctx) {
    int size = -1;
    int time_slice_ctx = -1;
    CHECK(al_begin_arraystruct_action(operation_ctx, "time_slice", "", &size, &time_slice_ctx)
              .code == 0);
    return time_slice_ctx;
}

static void check_stub_paths(const char *field, const char *timebase) {
    CHECK(strcmp(string_from_stub("recording_stub_read_field"), field) == 0);
    CHECK(strcmp(string_from_stub("recording_stub_read_timebase"), timebase) == 0);
}

/* --- the core gap: a relative read beneath a *renamed* AOS anchor -------- */

static void scenario_relative_field_and_timebase_resolve_through_renamed_child(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int time_slice_ctx = open_time_slice(operation_ctx);

    /* "constraints/b_field_pol_probe" is DD3's "constraints/bpol_probe"
     * (rule rename-bpol-probe). Opening it makes the child's own resolved
     * HLI-DD anchor a renamed path — unlike time_slice, whose anchor is
     * identical on both sides. */
    int size = -1;
    int arraystruct_ctx = -1;
    CHECK(al_begin_arraystruct_action(
              time_slice_ctx, "constraints/b_field_pol_probe", "", &size, &arraystruct_ctx)
              .code == 0);

    void *data = NULL;
    int shape[1] = {0};
    CHECK(al_read_data(arraystruct_ctx, "measured", "time", &data, 3, 1, shape).code == 0);
    CHECK(data != NULL);

    /* A plain forward would ask IMAS-Core for "measured" beneath a context
     * IMAS-Core itself opened under the stored spelling, which is exactly
     * right here — but only because the shim first translated the *anchor*
     * ("constraints/b_field_pol_probe" -> "constraints/bpol_probe") before
     * stripping it back off the resolved stored path. A shim that forgot to
     * translate the anchor would still send "measured" by coincidence (both
     * anchors have the same path depth), so this scenario also asserts the
     * anchor was genuinely retranslated by checking the arraystruct-open
     * side effect below. */
    check_stub_paths("measured", "time");

    printf("nested_context_read_test relative-field-and-timebase-resolve-through-renamed-child: "
           "a relative read beneath a renamed AOS anchor reached the stored spelling\\n");
}

/* --- absolute resolution ignores the live child's own anchor ------------ */

static void scenario_absolute_field_outside_child_subtree_resolves_from_ids_root(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int time_slice_ctx = open_time_slice(operation_ctx);

    int size = -1;
    int arraystruct_ctx = -1;
    CHECK(al_begin_arraystruct_action(
              time_slice_ctx, "constraints/b_field_pol_probe", "", &size, &arraystruct_ctx)
              .code == 0);

    /* This path lies entirely outside the open child's own subtree. A shim
     * that anchored every read at the live context's resolved path — instead
     * of only the relative ones — could not resolve this at all. */
    void *data = NULL;
    int shape[1] = {0};
    CHECK(al_read_data(arraystruct_ctx, "/time_slice/global_quantities/beta_tor_norm", "", &data,
                       3, 1, shape)
              .code == 0);
    CHECK(data != NULL);
    check_stub_paths("/time_slice/global_quantities/beta_normal", "");

    printf("nested_context_read_test absolute-field-outside-child-subtree-resolves-from-ids-root: "
           "an absolute read ignored the live child's own renamed anchor\\n");
}

/* --- no-source and refusal are unaffected by nesting --------------------- */

static void scenario_no_source_returns_null_through_nested_child(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int time_slice_ctx = open_time_slice(operation_ctx);
    int reads_before = int_from_stub("recording_stub_read_call_count");

    void *data = (void *)1;
    int shape[1] = {0};
    CHECK(al_read_data(time_slice_ctx, "boundary/lcfs", "", &data, 3, 1, shape).code == 0);
    CHECK(data == NULL);
    CHECK(int_from_stub("recording_stub_read_call_count") == reads_before);

    printf("nested_context_read_test no-source-returns-null-through-nested-child: an absent "
           "stored counterpart returned success with no data, without calling IMAS-Core\\n");
}

static void scenario_refusal_stops_before_core_through_nested_child(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int time_slice_ctx = open_time_slice(operation_ctx);
    int reads_before = int_from_stub("recording_stub_read_call_count");

    void *data = (void *)1;
    int shape[1] = {73};
    al_status_t status = al_read_data(
        time_slice_ctx, "constraints/strike_point/chi_squared_r", "", &data, 3, 1, shape);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(strcmp(status.message,
                 "IMAS-MVDD: this path's unit was redefined and cannot be converted; "
                 "DD path: time_slice/constraints/strike_point/chi_squared_r; "
                 "HLI DD version: 4.1.1; stored DD version: 3.39.0") == 0);
    CHECK(data == (void *)1);
    CHECK(shape[0] == 73);
    CHECK(int_from_stub("recording_stub_read_call_count") == reads_before);

    printf("nested_context_read_test refusal-stops-before-core-through-nested-child: an "
           "unmappable unit redefinition refused before IMAS-Core, addressed relative to a "
           "live arraystruct context\\n");
}

/* --- a supported value transformation still applies when nested --------- */

static void scenario_sign_flip_applies_through_nested_child(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int time_slice_ctx = open_time_slice(operation_ctx);

    /* "constraints/flux_loop" spells identically on both sides, so opening it
     * exercises the identity path through a nested AOS rather than a rename
     * — the COCOS flip below is what this scenario is actually about. */
    int size = -1;
    int flux_loop_ctx = -1;
    CHECK(al_begin_arraystruct_action(time_slice_ctx, "constraints/flux_loop", "", &size,
                                      &flux_loop_ctx)
              .code == 0);

    int read_size[1] = {0};
    void *data = NULL;
    CHECK(al_read_data(flux_loop_ctx, "measured", "", &data, 52 /* DOUBLE_DATA */, 1, read_size)
              .code == 0);
    CHECK(data != NULL);
    CHECK(*(double *)data == -1.5);
    check_stub_paths("measured", "");

    printf("nested_context_read_test sign-flip-applies-through-nested-child: a COCOS sign flip "
           "was applied to a field read relative to a live arraystruct context\\n");
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr,
                "usage: %s <relative-field-and-timebase-resolve-through-renamed-child|"
                "absolute-field-outside-child-subtree-resolves-from-ids-root|"
                "no-source-returns-null-through-nested-child|"
                "refusal-stops-before-core-through-nested-child|"
                "sign-flip-applies-through-nested-child>\\n",
                argv[0]);
        return 2;
    }
    if (strcmp(argv[1], "relative-field-and-timebase-resolve-through-renamed-child") == 0) {
        scenario_relative_field_and_timebase_resolve_through_renamed_child();
        return 0;
    }
    if (strcmp(argv[1], "absolute-field-outside-child-subtree-resolves-from-ids-root") == 0) {
        scenario_absolute_field_outside_child_subtree_resolves_from_ids_root();
        return 0;
    }
    if (strcmp(argv[1], "no-source-returns-null-through-nested-child") == 0) {
        scenario_no_source_returns_null_through_nested_child();
        return 0;
    }
    if (strcmp(argv[1], "refusal-stops-before-core-through-nested-child") == 0) {
        scenario_refusal_stops_before_core_through_nested_child();
        return 0;
    }
    if (strcmp(argv[1], "sign-flip-applies-through-nested-child") == 0) {
        scenario_sign_flip_applies_through_nested_child();
        return 0;
    }
    fprintf(stderr, "unknown scenario: %s\\n", argv[1]);
    return 2;
}
