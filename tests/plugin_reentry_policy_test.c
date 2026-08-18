/* Issue #67: give the al_plugin_* reentry twins the same context creation,
 * path translation, and lifecycle behavior as their non-plugin counterparts
 * (ADR 0002, ADR 0003, ADR 0007, ADR 0009, issue #55, issue #61, issue #63).
 *
 * These scenarios mirror version_discovery_test.c, arraystruct_path_test.c
 * and context_lifecycle_test.c, but drive the al_plugin_begin_global_action /
 * al_plugin_begin_slice_action / al_plugin_begin_arraystruct_action /
 * al_plugin_end_action seams instead of their ordinary twins. As before, the
 * HLI DD version latch and the context registry are both process-wide, so
 * each scenario is its own ctest process. */

#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifndef RECORDING_STUB_PATH
#error "RECORDING_STUB_PATH must be defined by CMakeLists.txt"
#endif

#include "shim_test_support.h"

static al_status_t open_dataentry(int *dectxID) {
    return al_begin_dataentry_action("imas:hdf5?path=/tmp/pulse", 7, dectxID);
}

/* READ_OP (30, al_const.h) — irrelevant beyond being a plausible rwmode. */
static al_status_t open_global(int pctxID, const char *dataobjectname, const char *datapath,
                                int *octxID) {
    return al_begin_global_action(pctxID, dataobjectname, datapath, 30, octxID);
}

static al_status_t open_plugin_global(int pctxID, const char *dataobjectname,
                                       const char *datapath, int *octxID) {
    return al_plugin_begin_global_action(pctxID, dataobjectname, datapath, 30, octxID);
}

static al_status_t open_plugin_slice(int pctxID, const char *dataobjectname, int *octxID) {
    return al_plugin_begin_slice_action(pctxID, dataobjectname, 30, 1.5, 0, octxID);
}

static al_status_t read_data(int ctx_id, const char *field, const char *timebase, void **data) {
    int size[1] = {0};
    return al_read_data(ctx_id, field, timebase, data, 52 /* DOUBLE_DATA */, 1, size);
}

static void check_stub_field(const char *field) {
    CHECK(strcmp(string_from_stub("recording_stub_read_field"), field) == 0);
}

static void check_plugin_stub_field(const char *field) {
    CHECK(strcmp(string_from_stub("recording_stub_plugin_first_string"), field) == 0);
}

static int loss_count(int ctx_id) {
    int count = -1;
    al_status_t status = imas_mvdd_context_loss_count(ctx_id, &count);
    CHECK(status.code == 0);
    return count;
}

static void check_loss_at(int ctx_id, int index, const char *expected_path, int expected_verdict) {
    char path_buf[256] = {0};
    int verdict = -1;
    al_status_t status =
        imas_mvdd_context_loss_at(ctx_id, index, path_buf, sizeof(path_buf), &verdict);
    CHECK(status.code == 0);
    CHECK(strcmp(path_buf, expected_path) == 0);
    CHECK(verdict == expected_verdict);
}

/* "rename-beta-normal" in docs/3.39.0--4.1.1.xml: 4.1.1's spelling on the
 * right, 3.39.0's on the left. */
static const char *const ROOT_HLI_FIELD = "time_slice/global_quantities/beta_tor_norm";
static const char *const ROOT_STORED_FIELD = "time_slice/global_quantities/beta_normal";

/* --- al_plugin_begin_global_action follows begin_global_action's rule ----- */

static void scenario_plugin_global_hli_unset_is_plain_forward(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    int octxID = -1;
    al_status_t status = open_plugin_global(1001, "equilibrium", "some/datapath", &octxID);

    CHECK(status.code == 0);
    CHECK(octxID == 5001);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_first_string"), "equilibrium") == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_second_string"), "some/datapath") == 0);
    /* No HLI DD version means no discovery at all: not even the version-
     * stamp read happens. */
    CHECK(int_from_stub("recording_stub_read_call_count") == 0);

    printf("plugin_reentry_policy_test plugin-global-hli-unset-is-plain-forward: no discovery "
           "was attempted\n");
}

static void scenario_plugin_global_unstamped_forwards_datapath_unchanged(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    int octxID = -1;
    al_status_t status = open_plugin_global(1001, "equilibrium", ROOT_HLI_FIELD, &octxID);

    CHECK(status.code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_second_string"), ROOT_HLI_FIELD) == 0);
    CHECK(int_from_stub("recording_stub_read_call_count") == 1);
    CHECK(strcmp(string_from_stub("recording_stub_read_field"),
                 "ids_properties/version_put/data_dictionary") == 0);

    printf("plugin_reentry_policy_test plugin-global-unstamped-forwards-datapath-unchanged: "
           "discovery was attempted through the plugin reentry seam\n");
}

static void scenario_plugin_global_matching_version_forwards_datapath_unchanged(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    int octxID = -1;
    CHECK(open_plugin_global(1001, "equilibrium", ROOT_HLI_FIELD, &octxID).code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_second_string"), ROOT_HLI_FIELD) == 0);

    int octxID2 = -1;
    CHECK(open_plugin_global(1001, "equilibrium", ROOT_HLI_FIELD, &octxID2).code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_second_string"), ROOT_HLI_FIELD) == 0);

    printf("plugin_reentry_policy_test plugin-global-matching-version-forwards-datapath-"
           "unchanged: a matching stamp is a passthrough\n");
}

static void scenario_plugin_global_mismatch_translates_datapath_on_second_open(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    int octxID = -1;
    CHECK(open_plugin_global(1001, "equilibrium", ROOT_HLI_FIELD, &octxID).code == 0);
    /* First use: the stored version isn't known until this very call's
     * stamp read completes, so datapath is forwarded unchanged. */
    CHECK(strcmp(string_from_stub("recording_stub_plugin_second_string"), ROOT_HLI_FIELD) == 0);

    int octxID2 = -1;
    CHECK(open_plugin_global(1001, "equilibrium", ROOT_HLI_FIELD, &octxID2).code == 0);
    /* Second open of the same occurrence under the same pulse: the mismatch
     * discovered by the first plugin open's stamp read is now known, so
     * datapath is translated before IMAS-Core is ever called. */
    CHECK(strcmp(string_from_stub("recording_stub_plugin_second_string"), ROOT_STORED_FIELD) == 0);

    printf("plugin_reentry_policy_test plugin-global-mismatch-translates-datapath-on-second-open: "
           "a discovered mismatch translated a later plugin open's datapath\n");
}

static void scenario_plugin_global_malformed_stamp_refuses_and_ends_context(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    int octxID = -1;
    al_status_t status = open_plugin_global(1001, "equilibrium", "", &octxID);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(strstr(status.message, "malformed") != NULL);

    /* The just-opened IMAS-Core context (id 5001, the stub's fixed plugin
     * global-action octxID) must be ended through al_plugin_end_action, not
     * al_end_action, so a refusal here never leaks it. */
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"), "al_plugin_end_action") ==
          0);
    CHECK(int_from_stub("recording_stub_plugin_last_ctx") == 5001);
    CHECK(int_from_stub("recording_stub_end_action_call_count") == 0);

    printf("plugin_reentry_policy_test plugin-global-malformed-stamp-refuses-and-ends-context: "
           "refused and cleaned up the leaked-open context through the plugin reentry twin\n");
}

static void scenario_plugin_global_failure_forwards_status_unchanged(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    int octxID = -1;
    al_status_t status = open_plugin_global(1001, "equilibrium", ROOT_HLI_FIELD, &octxID);

    CHECK(status.code != 0);
    CHECK(strstr(status.message, "recording-stub: plugin global open refused") != NULL);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_second_string"), ROOT_HLI_FIELD) == 0);
    /* A failed open must attempt no stamp discovery and leak no context. */
    CHECK(int_from_stub("recording_stub_read_call_count") == 0);

    printf("plugin_reentry_policy_test plugin-global-failure-forwards-status-unchanged: "
           "propagated IMAS-Core's refusal without attempting discovery\n");
}

/* --- al_plugin_begin_slice_action follows begin_slice_action's rule ------- */

static void scenario_plugin_slice_hli_unset_is_plain_forward(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    int octxID = -1;
    al_status_t status = open_plugin_slice(1001, "equilibrium", &octxID);

    CHECK(status.code == 0);
    CHECK(octxID == 5002);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_first_string"), "equilibrium") == 0);
    CHECK(int_from_stub("recording_stub_read_call_count") == 0);

    printf("plugin_reentry_policy_test plugin-slice-hli-unset-is-plain-forward: no discovery was "
           "attempted\n");
}

static void scenario_plugin_slice_mismatch_registers_occurrence_for_plugin_global_action(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    int octxID = -1;
    CHECK(open_plugin_slice(1001, "equilibrium", &octxID).code == 0);

    /* There is no C-level introspection into the context registry itself, so
     * the slice reentry's discovered mismatch is only externally observable
     * through another seam sharing the same occurrence cache: a subsequent
     * plugin global action on the same occurrence now translates its
     * datapath before IMAS-Core is ever called. */
    int octxID2 = -1;
    CHECK(open_plugin_global(1001, "equilibrium", ROOT_HLI_FIELD, &octxID2).code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_second_string"), ROOT_STORED_FIELD) == 0);

    printf("plugin_reentry_policy_test plugin-slice-mismatch-registers-occurrence-for-plugin-"
           "global-action: a plugin slice action's discovered mismatch translated a later plugin "
           "global action's datapath\n");
}

static void scenario_plugin_slice_malformed_stamp_refuses_and_ends_context(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    int octxID = -1;
    al_status_t status = open_plugin_slice(1001, "equilibrium", &octxID);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(strstr(status.message, "malformed") != NULL);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"), "al_plugin_end_action") ==
          0);
    CHECK(int_from_stub("recording_stub_plugin_last_ctx") == 5002);
    CHECK(int_from_stub("recording_stub_end_action_call_count") == 0);

    printf("plugin_reentry_policy_test plugin-slice-malformed-stamp-refuses-and-ends-context: "
           "refused and cleaned up the leaked-open context through the plugin reentry twin\n");
}

static void scenario_plugin_slice_failure_forwards_status_unchanged(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    int octxID = -1;
    al_status_t status = open_plugin_slice(1001, "equilibrium", &octxID);

    CHECK(status.code != 0);
    CHECK(strstr(status.message, "recording-stub: plugin slice open refused") != NULL);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_first_string"), "equilibrium") == 0);
    CHECK(int_from_stub("recording_stub_read_call_count") == 0);

    printf("plugin_reentry_policy_test plugin-slice-failure-forwards-status-unchanged: "
           "propagated IMAS-Core's refusal without attempting discovery\n");
}

/* --- al_plugin_begin_arraystruct_action follows begin_arraystruct_action's rule --- */

static void scenario_plugin_arraystruct_translates_under_mismatch(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);
    int operation_ctx = -1;
    CHECK(open_plugin_global(1001, "equilibrium", "", &operation_ctx).code == 0);

    int size = -1;
    int time_slice_ctx = -1;
    CHECK(al_plugin_begin_arraystruct_action(operation_ctx, "time_slice", "", &size,
                                              &time_slice_ctx)
              .code == 0);

    int arraystruct_ctx = -1;
    CHECK(al_plugin_begin_arraystruct_action(
              time_slice_ctx, "constraints/b_field_pol_probe",
              "/time_slice/constraints/b_field_pol_probe/time", &size, &arraystruct_ctx)
              .code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_first_string"),
                 "constraints/bpol_probe") == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_second_string"),
                 "/time_slice/constraints/bpol_probe/time") == 0);

    /* The child conversion record this plugin open registered is generic
     * over which seam opened it: an ordinary al_read_data through the same
     * ctxID still resolves the relative field against it. */
    void *data = NULL;
    CHECK(read_data(arraystruct_ctx, "/time_slice/constraints/b_field_pol_probe/measured", "",
                     &data)
              .code == 0);
    check_stub_field("/time_slice/constraints/bpol_probe/measured");

    printf("plugin_reentry_policy_test plugin-arraystruct-translates-under-mismatch: the stored "
           "AOS spelling opened through the plugin reentry seam and retained a child conversion "
           "record\n");
}

static void scenario_plugin_arraystruct_failed_open_propagates_without_child_record(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);
    int operation_ctx = -1;
    CHECK(open_plugin_global(1001, "equilibrium", "", &operation_ctx).code == 0);

    int size = -1;
    int arraystruct_ctx = 1777;
    al_status_t status = al_plugin_begin_arraystruct_action(
        operation_ctx, "time_slice/constraints/b_field_pol_probe", "", &size, &arraystruct_ctx);
    CHECK(status.code != 0);
    CHECK(strstr(status.message, "recording-stub: plugin arraystruct open refused") != NULL);
    CHECK(arraystruct_ctx == 1777);

    /* No child record was registered for the failed open: an ordinary read
     * through the untouched ctxID is a plain forward. */
    void *data = NULL;
    CHECK(read_data(arraystruct_ctx, "time_slice/global_quantities/beta_tor_norm", "", &data)
              .code == 0);
    check_stub_field("time_slice/global_quantities/beta_tor_norm");

    printf("plugin_reentry_policy_test plugin-arraystruct-failed-open-propagates-without-child-"
           "record: IMAS-Core's refusal was preserved and did not register a child\n");
}

static void scenario_plugin_arraystruct_no_source_refuses_before_core(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);
    int operation_ctx = -1;
    CHECK(open_plugin_global(1001, "equilibrium", "", &operation_ctx).code == 0);

    int calls_before = int_from_stub("recording_stub_plugin_call_count");
    int size = -1;
    int arraystruct_ctx = 1777;

    al_status_t status = al_plugin_begin_arraystruct_action(
        operation_ctx, "time_slice/constraints/j_parallel", "", &size, &arraystruct_ctx);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(int_from_stub("recording_stub_plugin_call_count") == calls_before);
    CHECK(arraystruct_ctx == 1777);

    printf("plugin_reentry_policy_test plugin-arraystruct-no-source-refuses-before-core: absent "
           "stored AOS did not open or register a child\n");
}

static void scenario_plugin_arraystruct_unknown_parent_forwards_unchanged(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);
    int operation_ctx = -1;
    CHECK(open_plugin_global(1001, "core_profiles", "", &operation_ctx).code == 0);

    int size = -1;
    int arraystruct_ctx = -1;
    CHECK(al_plugin_begin_arraystruct_action(
              operation_ctx, "time_slice/constraints/b_field_pol_probe",
              "/time_slice/constraints/b_field_pol_probe/time", &size, &arraystruct_ctx)
              .code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_first_string"),
                 "time_slice/constraints/b_field_pol_probe") == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_second_string"),
                 "/time_slice/constraints/b_field_pol_probe/time") == 0);

    printf("plugin_reentry_policy_test plugin-arraystruct-unknown-parent-forwards-unchanged: a "
           "parent without a conversion record left both arguments untouched\n");
}

/* --- al_plugin_end_action removes only its own record --------------------- */

static void scenario_plugin_end_action_removes_only_its_own_record(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);
    int operation_ctx = -1;
    CHECK(open_plugin_global(1001, "equilibrium", "", &operation_ctx).code == 0);
    int size = -1;
    int time_slice_ctx = -1;
    CHECK(al_plugin_begin_arraystruct_action(operation_ctx, "time_slice", "", &size,
                                              &time_slice_ctx)
              .code == 0);

    CHECK(al_plugin_end_action(time_slice_ctx).code == 0);

    /* The child's own record is gone: a relative read through it is now a
     * plain forward. */
    void *child_data = NULL;
    CHECK(read_data(time_slice_ctx, "global_quantities/beta_tor_norm", "", &child_data).code ==
          0);
    check_stub_field("global_quantities/beta_tor_norm");

    /* The root's own record is untouched: ending a child never mutates its
     * parent. */
    void *root_data = NULL;
    CHECK(read_data(operation_ctx, ROOT_HLI_FIELD, "", &root_data).code == 0);
    check_stub_field(ROOT_STORED_FIELD);

    printf("plugin_reentry_policy_test plugin-end-action-removes-only-its-own-record: ending the "
           "child through the plugin reentry twin released only the child's record\n");
}

static void scenario_plugin_end_action_failed_leaves_the_record_intact(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);
    int operation_ctx = -1;
    CHECK(open_plugin_global(1001, "equilibrium", "", &operation_ctx).code == 0);

    CHECK(setenv("RECORDING_STUB_PLUGIN_END_ACTION_FAIL", "1", 1) == 0);
    al_status_t status = al_plugin_end_action(operation_ctx);
    CHECK(status.code != 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"), "al_plugin_end_action") ==
          0);
    CHECK(int_from_stub("recording_stub_plugin_last_ctx") == operation_ctx);
    CHECK(unsetenv("RECORDING_STUB_PLUGIN_END_ACTION_FAIL") == 0);

    void *data = NULL;
    CHECK(read_data(operation_ctx, ROOT_HLI_FIELD, "", &data).code == 0);
    check_stub_field(ROOT_STORED_FIELD);

    printf("plugin_reentry_policy_test plugin-end-action-failed-leaves-the-record-intact: a "
           "refused close left the record live for a later read to still translate\n");
}

/* --- al_plugin_read_data follows read_data's rule exactly (issue #68) ----- */

static void scenario_plugin_read_translates_field_under_mismatch(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);
    int operation_ctx = -1;
    CHECK(open_plugin_global(1001, "equilibrium", "", &operation_ctx).code == 0);

    void *data = NULL;
    int size[1] = {0};
    CHECK(al_plugin_read_data(operation_ctx, ROOT_HLI_FIELD, "", &data, 52 /* DOUBLE_DATA */, 1,
                              size)
              .code == 0);
    CHECK(data != NULL);
    check_plugin_stub_field(ROOT_STORED_FIELD);
    CHECK(loss_count(operation_ctx) == 0);

    printf("plugin_reentry_policy_test plugin-read-translates-field-under-mismatch: a rename "
           "rule's stored spelling reached IMAS-Core through the plugin reentry seam\n");
}

static void scenario_plugin_read_refusal_before_core(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);
    int operation_ctx = -1;
    CHECK(open_plugin_global(1001, "equilibrium", "", &operation_ctx).code == 0);

    int plugin_calls_before = int_from_stub("recording_stub_plugin_call_count");
    const char *field = "time_slice/constraints/strike_point/chi_squared_r";
    void *data = (void *)1;
    int size[1] = {73};

    al_status_t status =
        al_plugin_read_data(operation_ctx, field, "", &data, 52 /* DOUBLE_DATA */, 1, size);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(strstr(status.message, "this path's unit was redefined and cannot be converted") !=
          NULL);
    CHECK(int_from_stub("recording_stub_plugin_call_count") == plugin_calls_before);
    CHECK(data == (void *)1);
    CHECK(size[0] == 73);
    CHECK(loss_count(operation_ctx) == 1);
    check_loss_at(operation_ctx, 0, field, IMAS_MVDD_FIDELITY_UNMAPPABLE);

    printf("plugin_reentry_policy_test plugin-read-refusal-before-core: a unit redefinition "
           "refused through the plugin reentry seam without calling IMAS-Core\n");
}

static void scenario_plugin_read_no_source_returns_null_without_core_call(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);
    int operation_ctx = -1;
    CHECK(open_plugin_global(1001, "equilibrium", "", &operation_ctx).code == 0);

    int plugin_calls_before = int_from_stub("recording_stub_plugin_call_count");
    void *data = (void *)1;
    int size[1] = {0};
    CHECK(al_plugin_read_data(operation_ctx, "time_slice/contour_tree/critical_point", "", &data,
                              52 /* DOUBLE_DATA */, 1, size)
              .code == 0);

    CHECK(data == NULL);
    CHECK(int_from_stub("recording_stub_plugin_call_count") == plugin_calls_before);
    CHECK(loss_count(operation_ctx) == 1);
    check_loss_at(operation_ctx, 0, "time_slice/contour_tree/critical_point",
                  IMAS_MVDD_FIDELITY_LOSSY);

    printf("plugin_reentry_policy_test plugin-read-no-source-returns-null-without-core-call: no "
           "stored path was read through the plugin reentry seam\n");
}

static void scenario_plugin_read_merged_candidate_falls_through(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);
    int operation_ctx = -1;
    CHECK(open_plugin_global(1001, "equilibrium", "", &operation_ctx).code == 0);

    int plugin_calls_before = int_from_stub("recording_stub_plugin_call_count");
    void *data = NULL;
    int size[1] = {0};
    CHECK(al_plugin_read_data(operation_ctx, "time_slice/ggd/b_field_phi", "", &data,
                              52 /* DOUBLE_DATA */, 1, size)
              .code == 0);

    CHECK(data != NULL);
    CHECK(int_from_stub("recording_stub_plugin_call_count") == plugin_calls_before + 2);
    check_plugin_stub_field("time_slice/ggd/b_field_tor");

    printf("plugin_reentry_policy_test plugin-read-merged-candidate-falls-through: the merged "
           "plan's candidate loop ran through the plugin reentry seam\n");
}

static void scenario_plugin_read_sign_flip_negates_values(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);
    int operation_ctx = -1;
    CHECK(open_plugin_global(1001, "equilibrium", "", &operation_ctx).code == 0);

    void *data = NULL;
    int size[1] = {0};
    CHECK(al_plugin_read_data(operation_ctx, "time_slice/profiles_1d/psi", "", &data,
                              52 /* DOUBLE_DATA */, 1, size)
              .code == 0);

    CHECK(data != NULL);
    CHECK(size[0] == 1);
    CHECK(*(double *)data == -1.5);

    printf("plugin_reentry_policy_test plugin-read-sign-flip-negates-values: a COCOS sign flip "
           "applied through the plugin reentry seam exactly as it does for al_read_data\n");
}

static void scenario_plugin_read_through_child_context_retains_loss_on_root(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);
    int operation_ctx = -1;
    CHECK(open_plugin_global(1001, "equilibrium", "", &operation_ctx).code == 0);

    int size = -1;
    int time_slice_ctx = -1;
    CHECK(al_plugin_begin_arraystruct_action(operation_ctx, "time_slice", "", &size,
                                              &time_slice_ctx)
              .code == 0);

    /* move-gap is rel="moved", fidelity forward="lossy" (read_path_test.c
     * proves the same rule at the root; nested_context_read_test.c proves it
     * through an ordinary arraystruct child). Reading it relative to a
     * plugin-opened time_slice child must append the same complete DD path
     * to the root's loss log. */
    void *data = NULL;
    int shape[1] = {0};
    CHECK(al_plugin_read_data(time_slice_ctx, "boundary_separatrix/gap/r", "", &data,
                              52 /* DOUBLE_DATA */, 1, shape)
              .code == 0);
    CHECK(data != NULL);
    check_plugin_stub_field("boundary/gap/r");

    CHECK(loss_count(time_slice_ctx) == 1);
    check_loss_at(time_slice_ctx, 0, "time_slice/boundary_separatrix/gap/r",
                  IMAS_MVDD_FIDELITY_LOSSY);
    CHECK(loss_count(operation_ctx) == 1);
    check_loss_at(operation_ctx, 0, "time_slice/boundary_separatrix/gap/r",
                  IMAS_MVDD_FIDELITY_LOSSY);

    printf("plugin_reentry_policy_test plugin-read-through-child-context-retains-loss-on-root: a "
           "non-exact plugin read through a plugin-opened child context appended its complete DD "
           "path to the root's loss log\n");
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr,
                "usage: %s "
                "<plugin-global-hli-unset-is-plain-forward|"
                "plugin-global-unstamped-forwards-datapath-unchanged|"
                "plugin-global-matching-version-forwards-datapath-unchanged|"
                "plugin-global-mismatch-translates-datapath-on-second-open|"
                "plugin-global-malformed-stamp-refuses-and-ends-context|"
                "plugin-global-failure-forwards-status-unchanged|"
                "plugin-slice-hli-unset-is-plain-forward|"
                "plugin-slice-mismatch-registers-occurrence-for-plugin-global-action|"
                "plugin-slice-malformed-stamp-refuses-and-ends-context|"
                "plugin-slice-failure-forwards-status-unchanged|"
                "plugin-arraystruct-translates-under-mismatch|"
                "plugin-arraystruct-failed-open-propagates-without-child-record|"
                "plugin-arraystruct-no-source-refuses-before-core|"
                "plugin-arraystruct-unknown-parent-forwards-unchanged|"
                "plugin-end-action-removes-only-its-own-record|"
                "plugin-end-action-failed-leaves-the-record-intact|"
                "plugin-read-translates-field-under-mismatch|"
                "plugin-read-refusal-before-core|"
                "plugin-read-no-source-returns-null-without-core-call|"
                "plugin-read-merged-candidate-falls-through|"
                "plugin-read-sign-flip-negates-values|"
                "plugin-read-through-child-context-retains-loss-on-root>\n",
                argv[0]);
        return 2;
    }

    const char *scenario = argv[1];
    if (strcmp(scenario, "plugin-global-hli-unset-is-plain-forward") == 0) {
        scenario_plugin_global_hli_unset_is_plain_forward();
    } else if (strcmp(scenario, "plugin-global-unstamped-forwards-datapath-unchanged") == 0) {
        scenario_plugin_global_unstamped_forwards_datapath_unchanged();
    } else if (strcmp(scenario, "plugin-global-matching-version-forwards-datapath-unchanged") ==
               0) {
        scenario_plugin_global_matching_version_forwards_datapath_unchanged();
    } else if (strcmp(scenario, "plugin-global-mismatch-translates-datapath-on-second-open") ==
               0) {
        scenario_plugin_global_mismatch_translates_datapath_on_second_open();
    } else if (strcmp(scenario, "plugin-global-malformed-stamp-refuses-and-ends-context") == 0) {
        scenario_plugin_global_malformed_stamp_refuses_and_ends_context();
    } else if (strcmp(scenario, "plugin-global-failure-forwards-status-unchanged") == 0) {
        scenario_plugin_global_failure_forwards_status_unchanged();
    } else if (strcmp(scenario, "plugin-slice-hli-unset-is-plain-forward") == 0) {
        scenario_plugin_slice_hli_unset_is_plain_forward();
    } else if (strcmp(scenario,
                       "plugin-slice-mismatch-registers-occurrence-for-plugin-global-action") ==
               0) {
        scenario_plugin_slice_mismatch_registers_occurrence_for_plugin_global_action();
    } else if (strcmp(scenario, "plugin-slice-malformed-stamp-refuses-and-ends-context") == 0) {
        scenario_plugin_slice_malformed_stamp_refuses_and_ends_context();
    } else if (strcmp(scenario, "plugin-slice-failure-forwards-status-unchanged") == 0) {
        scenario_plugin_slice_failure_forwards_status_unchanged();
    } else if (strcmp(scenario, "plugin-arraystruct-translates-under-mismatch") == 0) {
        scenario_plugin_arraystruct_translates_under_mismatch();
    } else if (strcmp(scenario,
                       "plugin-arraystruct-failed-open-propagates-without-child-record") == 0) {
        scenario_plugin_arraystruct_failed_open_propagates_without_child_record();
    } else if (strcmp(scenario, "plugin-arraystruct-no-source-refuses-before-core") == 0) {
        scenario_plugin_arraystruct_no_source_refuses_before_core();
    } else if (strcmp(scenario, "plugin-arraystruct-unknown-parent-forwards-unchanged") == 0) {
        scenario_plugin_arraystruct_unknown_parent_forwards_unchanged();
    } else if (strcmp(scenario, "plugin-end-action-removes-only-its-own-record") == 0) {
        scenario_plugin_end_action_removes_only_its_own_record();
    } else if (strcmp(scenario, "plugin-end-action-failed-leaves-the-record-intact") == 0) {
        scenario_plugin_end_action_failed_leaves_the_record_intact();
    } else if (strcmp(scenario, "plugin-read-translates-field-under-mismatch") == 0) {
        scenario_plugin_read_translates_field_under_mismatch();
    } else if (strcmp(scenario, "plugin-read-refusal-before-core") == 0) {
        scenario_plugin_read_refusal_before_core();
    } else if (strcmp(scenario, "plugin-read-no-source-returns-null-without-core-call") == 0) {
        scenario_plugin_read_no_source_returns_null_without_core_call();
    } else if (strcmp(scenario, "plugin-read-merged-candidate-falls-through") == 0) {
        scenario_plugin_read_merged_candidate_falls_through();
    } else if (strcmp(scenario, "plugin-read-sign-flip-negates-values") == 0) {
        scenario_plugin_read_sign_flip_negates_values();
    } else if (strcmp(scenario, "plugin-read-through-child-context-retains-loss-on-root") == 0) {
        scenario_plugin_read_through_child_context_retains_loss_on_root();
    } else {
        fprintf(stderr, "unknown scenario: %s\n", scenario);
        return 2;
    }

    return 0;
}
