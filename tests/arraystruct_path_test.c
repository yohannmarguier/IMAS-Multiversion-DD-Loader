/* Issue #61: public al_begin_arraystruct_action scenarios against the
 * recording stub. */

#include <string.h>

#ifndef RECORDING_STUB_PATH
#error "RECORDING_STUB_PATH must be defined by CMakeLists.txt"
#endif

#include "shim_test_support.h"

static void scenario_translates_renamed_container_and_timebase(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int size = -1;
    int time_slice_ctx = -1;
    CHECK(al_begin_arraystruct_action(operation_ctx, "time_slice", "", &size, &time_slice_ctx)
              .code == 0);

    int arraystruct_ctx = -1;

    CHECK(al_begin_arraystruct_action(
              time_slice_ctx, "constraints/b_field_pol_probe",
              "/time_slice/constraints/b_field_pol_probe/time", &size, &arraystruct_ctx)
              .code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_arraystruct_path"),
                 "constraints/bpol_probe") == 0);
    CHECK(strcmp(string_from_stub("recording_stub_arraystruct_timebase"),
                 "/time_slice/constraints/bpol_probe/time") == 0);

    void *data = NULL;
    int shape[1] = {0};
    CHECK(al_read_data(arraystruct_ctx,
                       "/time_slice/constraints/b_field_pol_probe/measured", "", &data, 3,
                       1, shape)
              .code == 0);
    CHECK(data != NULL);
    CHECK(strcmp(string_from_stub("recording_stub_read_field"),
                 "/time_slice/constraints/bpol_probe/measured") == 0);

    printf("arraystruct_path_test translates-renamed-container-and-timebase: the stored "
           "AOS spelling opened and retained a child conversion record\\n");
}

static void scenario_translates_absolute_path_and_relative_timebase(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int size = -1;
    int arraystruct_ctx = -1;

    CHECK(al_begin_arraystruct_action(
              operation_ctx, "/time_slice/constraints/b_field_pol_probe",
              "time_slice/constraints/b_field_pol_probe/time", &size, &arraystruct_ctx)
              .code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_arraystruct_path"),
                 "/time_slice/constraints/bpol_probe") == 0);
    CHECK(strcmp(string_from_stub("recording_stub_arraystruct_timebase"),
                 "time_slice/constraints/bpol_probe/time") == 0);

    printf("arraystruct_path_test translates-absolute-path-and-relative-timebase: both "
           "argument roots resolved through the conversion map\\n");
}

static void scenario_failed_open_propagates_without_child_record(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int size = -1;
    int arraystruct_ctx = 1777;

    al_status_t status = al_begin_arraystruct_action(
        operation_ctx, "time_slice/constraints/b_field_pol_probe", "", &size, &arraystruct_ctx);
    CHECK(status.code == -12);
    CHECK(strcmp(status.message, "recording-stub: arraystruct open refused") == 0);
    CHECK(arraystruct_ctx == 1777);

    void *data = NULL;
    int shape[1] = {0};
    CHECK(al_read_data(arraystruct_ctx, "time_slice/global_quantities/beta_tor_norm", "", &data,
                       3, 1, shape)
              .code == 0);
    CHECK(data != NULL);
    CHECK(strcmp(string_from_stub("recording_stub_read_field"),
                 "time_slice/global_quantities/beta_tor_norm") == 0);

    printf("arraystruct_path_test failed-open-propagates-without-child-record: Core failure "
           "was preserved and did not register a child\\n");
}

static void scenario_no_source_refuses_before_core(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int calls_before = int_from_stub("recording_stub_arraystruct_call_count");
    int size = -1;
    int arraystruct_ctx = 1777;

    al_status_t status = al_begin_arraystruct_action(
        operation_ctx, "time_slice/constraints/j_parallel", "", &size, &arraystruct_ctx);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(int_from_stub("recording_stub_arraystruct_call_count") == calls_before);
    CHECK(arraystruct_ctx == 1777);

    void *data = NULL;
    int shape[1] = {0};
    CHECK(al_read_data(arraystruct_ctx, "time_slice/global_quantities/beta_tor_norm", "", &data,
                       3, 1, shape)
              .code == 0);
    CHECK(data != NULL);
    CHECK(strcmp(string_from_stub("recording_stub_read_field"),
                 "time_slice/global_quantities/beta_tor_norm") == 0);

    printf("arraystruct_path_test no-source-refuses-before-core: absent stored AOS did not "
           "open or register a child\\n");
}

static void scenario_plain_parent_forwards_unchanged(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int size = -1;
    int arraystruct_ctx = -1;

    CHECK(al_begin_arraystruct_action(
              operation_ctx, "time_slice/constraints/b_field_pol_probe",
              "/time_slice/constraints/b_field_pol_probe/time", &size, &arraystruct_ctx)
              .code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_arraystruct_path"),
                 "time_slice/constraints/b_field_pol_probe") == 0);
    CHECK(strcmp(string_from_stub("recording_stub_arraystruct_timebase"),
                 "/time_slice/constraints/b_field_pol_probe/time") == 0);

    printf("arraystruct_path_test plain-parent-forwards-unchanged: an unconverted parent "
           "left both arguments untouched\\n");
}

static void scenario_unknown_parent_forwards_unchanged(void) {
    int operation_ctx = open_mismatched_occurrence("core_profiles", NULL);
    int size = -1;
    int arraystruct_ctx = -1;

    CHECK(al_begin_arraystruct_action(
              operation_ctx, "time_slice/constraints/b_field_pol_probe",
              "/time_slice/constraints/b_field_pol_probe/time", &size, &arraystruct_ctx)
              .code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_arraystruct_path"),
                 "time_slice/constraints/b_field_pol_probe") == 0);
    CHECK(strcmp(string_from_stub("recording_stub_arraystruct_timebase"),
                 "/time_slice/constraints/b_field_pol_probe/time") == 0);

    printf("arraystruct_path_test unknown-parent-forwards-unchanged: a parent without an "
           "artifact left both arguments untouched\\n");
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr,
                "usage: %s <translates-renamed-container-and-timebase|"
                "translates-absolute-path-and-relative-timebase|"
                "failed-open-propagates-without-child-record|no-source-refuses-before-core|"
                "plain-parent-forwards-unchanged|unknown-parent-forwards-unchanged>\\n",
                argv[0]);
        return 2;
    }
    if (strcmp(argv[1], "translates-renamed-container-and-timebase") == 0) {
        scenario_translates_renamed_container_and_timebase();
        return 0;
    }
    if (strcmp(argv[1], "translates-absolute-path-and-relative-timebase") == 0) {
        scenario_translates_absolute_path_and_relative_timebase();
        return 0;
    }
    if (strcmp(argv[1], "failed-open-propagates-without-child-record") == 0) {
        scenario_failed_open_propagates_without_child_record();
        return 0;
    }
    if (strcmp(argv[1], "no-source-refuses-before-core") == 0) {
        scenario_no_source_refuses_before_core();
        return 0;
    }
    if (strcmp(argv[1], "plain-parent-forwards-unchanged") == 0) {
        scenario_plain_parent_forwards_unchanged();
        return 0;
    }
    if (strcmp(argv[1], "unknown-parent-forwards-unchanged") == 0) {
        scenario_unknown_parent_forwards_unchanged();
        return 0;
    }
    fprintf(stderr, "unknown scenario: %s\\n", argv[1]);
    return 2;
}
