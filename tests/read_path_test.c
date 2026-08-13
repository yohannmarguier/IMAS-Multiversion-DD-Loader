/* Issue #56: public al_read_data ABI scenarios against the recording stub.
 *
 * Every scenario opens an equilibrium occurrence whose supplied stamp makes
 * its stored DD version differ from the HLI DD version. The recording stub
 * is only the external IMAS-Core substitute: calls enter the shim through
 * its public C ABI and observe its behavior through the arguments the stub
 * receives. */

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

static void *dlsym_or_die(void *handle, const char *name) {
    void *symbol = dlsym(handle, name);
    if (symbol == NULL) {
        fprintf(stderr, "recording stub has no symbol '%s': %s\\n", name, dlerror());
        abort();
    }
    return symbol;
}

static const char *string_from_stub(const char *symbol_name) {
    void *stub = dlopen(RECORDING_STUB_PATH, RTLD_NOW | RTLD_LOCAL);
    if (stub == NULL) {
        fprintf(stderr, "failed to open recording stub: %s\\n", dlerror());
        abort();
    }
    string_accessor_fn accessor = (string_accessor_fn)dlsym_or_die(stub, symbol_name);
    return accessor();
}

static int int_from_stub(const char *symbol_name) {
    void *stub = dlopen(RECORDING_STUB_PATH, RTLD_NOW | RTLD_LOCAL);
    if (stub == NULL) {
        fprintf(stderr, "failed to open recording stub: %s\\n", dlerror());
        abort();
    }
    int_accessor_fn accessor = (int_accessor_fn)dlsym_or_die(stub, symbol_name);
    return accessor();
}

static int open_mismatched_equilibrium(void) {
    int pulse_ctx = -1;
    CHECK(al_begin_dataentry_action("imas:hdf5?path=/tmp/pulse", 7, &pulse_ctx).code == 0);

    int operation_ctx = -1;
    CHECK(al_begin_global_action(pulse_ctx, "equilibrium", "", 30, &operation_ctx).code == 0);
    return operation_ctx;
}

static al_status_t read_data(int ctx_id, const char *field, const char *timebase, void **data) {
    int size[1] = {0};
    return al_read_data(ctx_id, field, timebase, data, 3, 1, size);
}

static void check_stub_paths(const char *field, const char *timebase) {
    CHECK(strcmp(string_from_stub("recording_stub_read_field"), field) == 0);
    CHECK(strcmp(string_from_stub("recording_stub_read_timebase"), timebase) == 0);
}

static void check_read_refusal(int operation_ctx, const char *field, int datatype,
                               const char *expected_message) {
    int reads_before = int_from_stub("recording_stub_read_call_count");
    void *data = (void *)1;
    int size[1] = {73};

    al_status_t status = al_read_data(operation_ctx, field, "", &data, datatype, 1, size);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(strcmp(status.message, expected_message) == 0);
    CHECK(data == (void *)1);
    CHECK(size[0] == 73);
    CHECK(int_from_stub("recording_stub_read_call_count") == reads_before);
}

static void scenario_translates_field_and_timebase_independently(void) {
    int operation_ctx = open_mismatched_equilibrium();
    void *data = NULL;

    CHECK(read_data(operation_ctx, "time_slice/global_quantities/beta_tor_norm", "time", &data)
              .code == 0);
    CHECK(data != NULL);
    check_stub_paths("time_slice/global_quantities/beta_normal", "time");

    printf("read_path_test translates-field-and-timebase-independently: both paths reached "
           "the stored DD spelling\\n");
}

static void scenario_forward_direction_translates_and_reports_no_source(void) {
    int operation_ctx = open_mismatched_equilibrium();
    void *data = NULL;

    CHECK(read_data(operation_ctx, "time_slice/global_quantities/beta_normal", "time", &data)
              .code == 0);
    CHECK(data != NULL);
    check_stub_paths("time_slice/global_quantities/beta_tor_norm", "time");

    int reads_before = int_from_stub("recording_stub_read_call_count");
    data = (void *)1;
    CHECK(read_data(operation_ctx, "time_slice/boundary/lcfs", "", &data).code == 0);
    CHECK(data == NULL);
    CHECK(int_from_stub("recording_stub_read_call_count") == reads_before);

    printf("read_path_test forward-direction-translates-and-reports-no-source: 3.39.0 HLI "
           "paths used 4.1.1 spellings or returned not found\\n");
}

static void scenario_identity_rule_returns_data(void) {
    int operation_ctx = open_mismatched_equilibrium();
    void *data = NULL;

    CHECK(read_data(operation_ctx, "time", "", &data).code == 0);
    CHECK(data != NULL);
    check_stub_paths("time", "");

    printf("read_path_test identity-rule-returns-data: identity rule read the stored path\\n");
}

static void scenario_no_source_returns_null_without_core_call(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int reads_before = int_from_stub("recording_stub_read_call_count");
    void *data = (void *)1;

    CHECK(read_data(operation_ctx, "time_slice/contour_tree/critical_point", "", &data).code ==
          0);
    CHECK(data == NULL);
    CHECK(int_from_stub("recording_stub_read_call_count") == reads_before);

    printf("read_path_test no-source-returns-null-without-core-call: no stored path was read\\n");
}

static void scenario_rank_changing_retype_refuses_without_core_call(void) {
    int operation_ctx = open_mismatched_equilibrium();
    check_read_refusal(
        operation_ctx, "grids_ggd/grid/space/coordinates_type", 2 /* INTEGER_DATA */,
        "IMAS-MVDD: this path's container changed shape and cannot be served; "
        "DD path: grids_ggd/grid/space/coordinates_type; HLI DD version: 4.1.1; "
        "stored DD version: 3.39.0");

    printf("read_path_test rank-changing-retype-refuses-without-core-call: refusal preserved "
           "caller storage and never reached IMAS-Core\\n");
}

static void scenario_unit_redefinition_refuses_without_core_call(void) {
    int operation_ctx = open_mismatched_equilibrium();
    check_read_refusal(
        operation_ctx, "time_slice/constraints/strike_point/chi_squared_r", 3 /* DOUBLE_DATA */,
        "IMAS-MVDD: this path's unit was redefined and cannot be converted; "
        "DD path: time_slice/constraints/strike_point/chi_squared_r; "
        "HLI DD version: 4.1.1; stored DD version: 3.39.0");

    printf("read_path_test unit-redefinition-refuses-without-core-call: refusal preserved "
           "caller storage and never reached IMAS-Core\\n");
}

static void scenario_unsupported_sign_flip_types_refuse_without_core_call(void) {
    int operation_ctx = open_mismatched_equilibrium();
    const int unsupported_types[] = {2 /* INTEGER_DATA */, 4 /* COMPLEX_DATA */};

    for (size_t i = 0; i < sizeof unsupported_types / sizeof unsupported_types[0]; ++i) {
        check_read_refusal(
            operation_ctx, "time_slice/boundary/psi", unsupported_types[i],
            "IMAS-MVDD: value-transform execution is not yet implemented (issue #59); "
            "DD path: time_slice/boundary/psi; HLI DD version: 4.1.1; stored DD version: "
            "3.39.0");
    }

    printf("read_path_test unsupported-sign-flip-types-refuse-without-core-call: integer "
           "and complex reads were refused before IMAS-Core\\n");
}

static void scenario_resolves_relative_field_and_absolute_timebase(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int size = -1;
    int arraystruct_ctx = -1;
    CHECK(al_begin_arraystruct_action(operation_ctx, "time_slice", "", &size, &arraystruct_ctx)
              .code == 0);

    void *data = NULL;
    CHECK(read_data(arraystruct_ctx, "global_quantities/beta_tor_norm",
                    "/time_slice/global_quantities/beta_tor_norm", &data)
              .code == 0);
    CHECK(data != NULL);
    check_stub_paths("global_quantities/beta_normal",
                     "/time_slice/global_quantities/beta_normal");

    printf("read_path_test resolves-relative-field-and-absolute-timebase: each path resolved "
           "through the correct context root\\n");
}

static void scenario_matching_context_bypasses_conversion(void) {
    int operation_ctx = open_mismatched_equilibrium();
    void *data = NULL;

    CHECK(read_data(operation_ctx, "time_slice/global_quantities/beta_tor_norm",
                    "time_slice/global_quantities/beta_tor_norm", &data)
              .code == 0);
    CHECK(data != NULL);
    check_stub_paths("time_slice/global_quantities/beta_tor_norm",
                     "time_slice/global_quantities/beta_tor_norm");

    printf("read_path_test matching-context-bypasses-conversion: matching stamp was forwarded\\n");
}

static void scenario_unknown_context_bypasses_conversion(void) {
    int pulse_ctx = -1;
    CHECK(al_begin_dataentry_action("imas:hdf5?path=/tmp/pulse", 7, &pulse_ctx).code == 0);
    int operation_ctx = -1;
    CHECK(al_begin_global_action(pulse_ctx, "core_profiles", "", 30, &operation_ctx).code == 0);
    void *data = NULL;

    CHECK(read_data(operation_ctx, "profiles_1d/electrons/density", "time", &data).code == 0);
    CHECK(data != NULL);
    check_stub_paths("profiles_1d/electrons/density", "time");

    printf("read_path_test unknown-context-bypasses-conversion: unavailable artifact was forwarded\\n");
}

static void scenario_unstamped_context_bypasses_conversion(void) {
    int operation_ctx = open_mismatched_equilibrium();
    void *data = NULL;

    CHECK(read_data(operation_ctx, "time_slice/global_quantities/beta_tor_norm",
                    "time_slice/global_quantities/beta_tor_norm", &data)
              .code == 0);
    CHECK(data != NULL);
    check_stub_paths("time_slice/global_quantities/beta_tor_norm",
                     "time_slice/global_quantities/beta_tor_norm");

    printf("read_path_test unstamped-context-bypasses-conversion: unstamped occurrence was "
           "forwarded\\n");
}

static void scenario_conversion_disabled_bypasses_conversion(void) {
    int operation_ctx = open_mismatched_equilibrium();
    void *data = NULL;

    CHECK(read_data(operation_ctx, "time_slice/global_quantities/beta_tor_norm",
                    "time_slice/global_quantities/beta_tor_norm", &data)
              .code == 0);
    CHECK(data != NULL);
    check_stub_paths("time_slice/global_quantities/beta_tor_norm",
                     "time_slice/global_quantities/beta_tor_norm");

    printf("read_path_test conversion-disabled-bypasses-conversion: unset HLI version was "
           "forwarded\\n");
}

static void scenario_core_failure_propagates_unchanged(void) {
    int operation_ctx = open_mismatched_equilibrium();
    void *data = NULL;

    al_status_t status = read_data(operation_ctx, "time_slice/global_quantities/beta_tor_norm",
                                   "time_slice/global_quantities/beta_tor_norm", &data);
    CHECK(status.code == -23);
    CHECK(strcmp(status.message, "recording-stub: read refused") == 0);
    check_stub_paths("time_slice/global_quantities/beta_normal",
                     "time_slice/global_quantities/beta_normal");

    printf("read_path_test core-failure-propagates-unchanged: IMAS-Core status was preserved\\n");
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr,
                "usage: %s <translates-field-and-timebase-independently|"
                "forward-direction-translates-and-reports-no-source|identity-rule-returns-data|"
                "no-source-returns-null-without-core-call|"
                "rank-changing-retype-refuses-without-core-call|"
                "unit-redefinition-refuses-without-core-call|"
                "unsupported-sign-flip-types-refuse-without-core-call|"
                "resolves-relative-field-and-absolute-timebase|"
                "matching-context-bypasses-conversion|unknown-context-bypasses-conversion|"
                "unstamped-context-bypasses-conversion|conversion-disabled-bypasses-conversion|"
                "core-failure-propagates-unchanged>\\n",
                argv[0]);
        return 2;
    }
    if (strcmp(argv[1], "translates-field-and-timebase-independently") == 0) {
        scenario_translates_field_and_timebase_independently();
        return 0;
    }
    if (strcmp(argv[1], "forward-direction-translates-and-reports-no-source") == 0) {
        scenario_forward_direction_translates_and_reports_no_source();
        return 0;
    }
    if (strcmp(argv[1], "identity-rule-returns-data") == 0) {
        scenario_identity_rule_returns_data();
        return 0;
    }
    if (strcmp(argv[1], "no-source-returns-null-without-core-call") == 0) {
        scenario_no_source_returns_null_without_core_call();
        return 0;
    }
    if (strcmp(argv[1], "rank-changing-retype-refuses-without-core-call") == 0) {
        scenario_rank_changing_retype_refuses_without_core_call();
        return 0;
    }
    if (strcmp(argv[1], "unit-redefinition-refuses-without-core-call") == 0) {
        scenario_unit_redefinition_refuses_without_core_call();
        return 0;
    }
    if (strcmp(argv[1], "unsupported-sign-flip-types-refuse-without-core-call") == 0) {
        scenario_unsupported_sign_flip_types_refuse_without_core_call();
        return 0;
    }
    if (strcmp(argv[1], "resolves-relative-field-and-absolute-timebase") == 0) {
        scenario_resolves_relative_field_and_absolute_timebase();
        return 0;
    }
    if (strcmp(argv[1], "matching-context-bypasses-conversion") == 0) {
        scenario_matching_context_bypasses_conversion();
        return 0;
    }
    if (strcmp(argv[1], "unknown-context-bypasses-conversion") == 0) {
        scenario_unknown_context_bypasses_conversion();
        return 0;
    }
    if (strcmp(argv[1], "unstamped-context-bypasses-conversion") == 0) {
        scenario_unstamped_context_bypasses_conversion();
        return 0;
    }
    if (strcmp(argv[1], "conversion-disabled-bypasses-conversion") == 0) {
        scenario_conversion_disabled_bypasses_conversion();
        return 0;
    }
    if (strcmp(argv[1], "core-failure-propagates-unchanged") == 0) {
        scenario_core_failure_propagates_unchanged();
        return 0;
    }
    fprintf(stderr, "unknown scenario: %s\\n", argv[1]);
    return 2;
}
