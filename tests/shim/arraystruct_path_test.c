/* Issue #61: public al_begin_arraystruct_action scenarios against the
 * recording stub. Issue #178 adds the merged/subtree candidate-plan
 * scenarios below `scenario_unknown_parent_forwards_unchanged`. */

#include <stdlib.h>
#include <string.h>

#ifndef RECORDING_STUB_PATH
#error "RECORDING_STUB_PATH must be defined by CMakeLists.txt"
#endif

#include "../support/shim_test_support.h"

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
                       "/time_slice/constraints/b_field_pol_probe/measured", "", &data, 52,
                       1, shape)
              .code == 0);
    CHECK(data != NULL);
    CHECK(strcmp(string_from_stub("recording_stub_read_field"),
                 "/time_slice/constraints/bpol_probe/measured") == 0);

    printf("arraystruct_path_test translates-renamed-container-and-timebase: the stored "
           "AOS spelling opened and retained a child conversion record\n");
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
           "argument roots resolved through the conversion map\n");
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
                       52, 1, shape)
              .code == 0);
    CHECK(data != NULL);
    CHECK(strcmp(string_from_stub("recording_stub_read_field"),
                 "time_slice/global_quantities/beta_tor_norm") == 0);

    printf("arraystruct_path_test failed-open-propagates-without-child-record: Core failure "
           "was preserved and did not register a child\n");
}

static void scenario_no_source_refuses_before_core(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int calls_before = int_from_stub("recording_stub_arraystruct_call_count");
    int size = -1;
    int arraystruct_ctx = 1777;

    al_status_t status = al_begin_arraystruct_action(
        operation_ctx, "time_slice/constraints/j_parallel", "", &size, &arraystruct_ctx);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status, "arraystruct path has no stored source",
                          "time_slice/constraints/j_parallel", "4.1.1", "3.39.0");
    CHECK(int_from_stub("recording_stub_arraystruct_call_count") == calls_before);
    CHECK(arraystruct_ctx == 1777);

    void *data = NULL;
    int shape[1] = {0};
    CHECK(al_read_data(arraystruct_ctx, "time_slice/global_quantities/beta_tor_norm", "", &data,
                       52, 1, shape)
              .code == 0);
    CHECK(data != NULL);
    CHECK(strcmp(string_from_stub("recording_stub_read_field"),
                 "time_slice/global_quantities/beta_tor_norm") == 0);

    printf("arraystruct_path_test no-source-refuses-before-core: absent stored AOS did not "
           "open or register a child\n");
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
           "left both arguments untouched\n");
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
           "artifact left both arguments untouched\n");
}

/* --- Issue #178: merged/subtree candidate plans -------------------------- */

/* fold-constraints-j (docs/3.39.0--4.1.1.xml) is `rel="merged" subtree="yes"`:
 * right="time_slice/constraints/j_phi", from left="time_slice/constraints/j_phi"
 * precedence 1, from left="time_slice/constraints/j_tor" precedence 2
 * (deprecated). The HLI's own spelling and the precedence-1 stored spelling
 * happen to be textually identical here, which is exactly the ordinary case:
 * only the deprecated alias differs. */

static void scenario_merged_subtree_falls_through_to_populated_candidate(void) {
    int operation_ctx = open_mismatched_equilibrium();
    CHECK(setenv("RECORDING_STUB_ARRAYSTRUCT_EMPTY_PATHS", "time_slice/constraints/j_phi", 1) ==
          0);

    int size = -1;
    int arraystruct_ctx = -1;
    CHECK(al_begin_arraystruct_action(operation_ctx, "time_slice/constraints/j_phi", "", &size,
                                       &arraystruct_ctx)
              .code == 0);

    /* Precedence 1 ("j_phi") opened empty and was closed; precedence 2
     * ("j_tor"), which actually held data, was tried next and kept. */
    CHECK(strcmp(string_from_stub("recording_stub_arraystruct_path"),
                 "time_slice/constraints/j_tor") == 0);
    CHECK(int_from_stub("recording_stub_arraystruct_call_count") == 2);
    CHECK(int_from_stub("recording_stub_end_action_call_count") == 1);
    CHECK(size == 3003);

    void *data = NULL;
    int shape[1] = {0};
    CHECK(al_read_data(arraystruct_ctx, "measured", "", &data, 52, 1, shape).code == 0);
    CHECK(data != NULL);

    CHECK(unsetenv("RECORDING_STUB_ARRAYSTRUCT_EMPTY_PATHS") == 0);

    printf("arraystruct_path_test merged-subtree-falls-through-to-populated-candidate: an empty "
           "precedence-1 candidate was closed and the deprecated alias, which actually held "
           "data, was opened and registered as the child context instead\n");
}

static void scenario_merged_subtree_opens_empty_when_every_candidate_is_absent(void) {
    int operation_ctx = open_mismatched_equilibrium();
    CHECK(setenv("RECORDING_STUB_ARRAYSTRUCT_EMPTY_PATHS",
                 "time_slice/constraints/j_phi,time_slice/constraints/j_tor", 1) == 0);

    int size = -1;
    int arraystruct_ctx = -1;
    CHECK(al_begin_arraystruct_action(operation_ctx, "time_slice/constraints/j_phi", "", &size,
                                       &arraystruct_ctx)
              .code == 0);

    /* Every candidate came back empty, so the last one tried is kept rather
     * than refusing: a subtree with no data anywhere is a legitimate empty
     * array-of-structures, not an error (issue #178). */
    CHECK(strcmp(string_from_stub("recording_stub_arraystruct_path"),
                 "time_slice/constraints/j_tor") == 0);
    CHECK(int_from_stub("recording_stub_arraystruct_call_count") == 2);
    CHECK(int_from_stub("recording_stub_end_action_call_count") == 1);
    CHECK(size == 0);

    CHECK(unsetenv("RECORDING_STUB_ARRAYSTRUCT_EMPTY_PATHS") == 0);

    printf("arraystruct_path_test merged-subtree-opens-empty-when-every-candidate-is-absent: a "
           "wholly unpopulated merged subtree opened successfully with zero elements instead of "
           "refusing\n");
}

/* WRITE_OP (31, al_const.h) has no reader IMAS-Core's HDF5 backend can
 * guarantee (ADR 0020), so a candidate reporting "empty" through it cannot be
 * trusted to mean "absent" the way it can under READ_OP. This opens the same
 * occurrence and rule as the two scenarios above, but under WRITE_OP. */
static int open_mismatched_equilibrium_write(void) {
    int pulse_ctx = -1;
    CHECK(al_begin_dataentry_action("imas:hdf5?path=/tmp/pulse", 7, &pulse_ctx).code == 0);
    int operation_ctx = -1;
    CHECK(al_begin_global_action(pulse_ctx, "equilibrium", "", 31, &operation_ctx).code == 0);
    return operation_ctx;
}

static void scenario_merged_subtree_write_mode_takes_the_primary_candidate_without_probing(void) {
    int operation_ctx = open_mismatched_equilibrium_write();
    CHECK(setenv("RECORDING_STUB_ARRAYSTRUCT_EMPTY_PATHS", "time_slice/constraints/j_phi", 1) ==
          0);

    int calls_before = int_from_stub("recording_stub_arraystruct_call_count");
    int ends_before = int_from_stub("recording_stub_end_action_call_count");

    int size = -1;
    int arraystruct_ctx = -1;
    CHECK(al_begin_arraystruct_action(operation_ctx, "time_slice/constraints/j_phi", "", &size,
                                       &arraystruct_ctx)
              .code == 0);

    /* The declared precedence-1 candidate was opened once and kept, even
     * though the stub reported it empty: a WRITE_OP open never tries the
     * deprecated alias. */
    CHECK(strcmp(string_from_stub("recording_stub_arraystruct_path"),
                 "time_slice/constraints/j_phi") == 0);
    CHECK(int_from_stub("recording_stub_arraystruct_call_count") == calls_before + 1);
    CHECK(int_from_stub("recording_stub_end_action_call_count") == ends_before);
    CHECK(size == 0);

    CHECK(unsetenv("RECORDING_STUB_ARRAYSTRUCT_EMPTY_PATHS") == 0);

    printf("arraystruct_path_test merged-subtree-write-mode-takes-the-primary-candidate-without-"
           "probing: a WRITE_OP open kept the declared precedence-1 candidate without trying the "
           "deprecated alias\n");
}

static void scenario_refusal_retains_an_unmappable_read_loss(void) {
    int operation_ctx = open_mismatched_equilibrium();
    check_no_loss_entry(operation_ctx);

    int size = -1;
    int arraystruct_ctx = 1777;
    al_status_t status = al_begin_arraystruct_action(
        operation_ctx, "time_slice/constraints/j_parallel", "", &size, &arraystruct_ctx);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);

    CHECK(loss_count(operation_ctx) == 1);
    check_loss_at(operation_ctx, 0, "time_slice/constraints/j_parallel",
                  IMAS_MVDD_FIDELITY_UNMAPPABLE, IMAS_MVDD_LOSS_OPERATION_READ);

    printf("arraystruct_path_test refusal-retains-an-unmappable-read-loss: an arraystruct-open "
           "refusal now reaches the loss log exactly as a refused write or delete already does\n");
}

int main(int argc, char **argv) {
    static const shim_test_scenario scenarios[] = {
        {"translates-renamed-container-and-timebase", scenario_translates_renamed_container_and_timebase},
        {"translates-absolute-path-and-relative-timebase", scenario_translates_absolute_path_and_relative_timebase},
        {"failed-open-propagates-without-child-record", scenario_failed_open_propagates_without_child_record},
        {"no-source-refuses-before-core", scenario_no_source_refuses_before_core},
        {"plain-parent-forwards-unchanged", scenario_plain_parent_forwards_unchanged},
        {"unknown-parent-forwards-unchanged", scenario_unknown_parent_forwards_unchanged},
        {"merged-subtree-falls-through-to-populated-candidate",
         scenario_merged_subtree_falls_through_to_populated_candidate},
        {"merged-subtree-opens-empty-when-every-candidate-is-absent",
         scenario_merged_subtree_opens_empty_when_every_candidate_is_absent},
        {"merged-subtree-write-mode-takes-the-primary-candidate-without-probing",
         scenario_merged_subtree_write_mode_takes_the_primary_candidate_without_probing},
        {"refusal-retains-an-unmappable-read-loss", scenario_refusal_retains_an_unmappable_read_loss},
    };
    return RUN_NAMED_SCENARIO(argc, argv, scenarios);
}
