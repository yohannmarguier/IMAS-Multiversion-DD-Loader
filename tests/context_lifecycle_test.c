/* Issue #63: public lifecycle C ABI scenarios (al_end_action,
 * al_iterate_over_arraystruct, al_close_pulse) against the recording stub.
 *
 * There is no C-level introspection into the context registry itself (see
 * version_discovery_test.c's file header for why), so every invariant below
 * is proven the only way it is externally observable: whether a later
 * al_read_data through a still-live context translates its field the way a
 * live conversion record would, or forwards it unchanged the way an absent
 * record would. */

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

static int open_time_slice(int operation_ctx) {
    int size = -1;
    int time_slice_ctx = -1;
    CHECK(al_begin_arraystruct_action(operation_ctx, "time_slice", "", &size, &time_slice_ctx)
              .code == 0);
    return time_slice_ctx;
}

static al_status_t read_data(int ctx_id, const char *field, const char *timebase, void **data) {
    int size[1] = {0};
    return al_read_data(ctx_id, field, timebase, data, 52 /* DOUBLE_DATA */, 1, size);
}

static void check_stub_field(const char *field) {
    CHECK(strcmp(string_from_stub("recording_stub_read_field"), field) == 0);
}

/* "rename-beta-normal" in docs/3.39.0--4.1.1.xml: 4.1.1's spelling on the
 * right, 3.39.0's on the left. Used throughout as the marker rename: a
 * translated read sends the stored spelling, a plain forward sends the HLI
 * spelling verbatim. */
static const char *const ROOT_HLI_FIELD = "time_slice/global_quantities/beta_tor_norm";
static const char *const ROOT_STORED_FIELD = "time_slice/global_quantities/beta_normal";
static const char *const CHILD_HLI_FIELD = "global_quantities/beta_tor_norm";
static const char *const CHILD_STORED_FIELD = "global_quantities/beta_normal";

/* --- A successful al_end_action removes only its own record -------------- */

static void scenario_ending_child_removes_only_its_own_record(void) {
    int pulse_ctx = -1;
    CHECK(open_dataentry(&pulse_ctx).code == 0);
    int operation_ctx = -1;
    CHECK(open_global(pulse_ctx, "equilibrium", "", &operation_ctx).code == 0);
    int time_slice_ctx = open_time_slice(operation_ctx);

    CHECK(al_end_action(time_slice_ctx).code == 0);

    /* The child's own record is gone: a relative read through it is now a
     * plain forward, sent verbatim rather than resolved against the child's
     * (now released) anchor. */
    void *child_data = NULL;
    CHECK(read_data(time_slice_ctx, CHILD_HLI_FIELD, "", &child_data).code == 0);
    check_stub_field(CHILD_HLI_FIELD);

    /* The root's own record is untouched: ending a child never mutates its
     * parent. */
    void *root_data = NULL;
    CHECK(read_data(operation_ctx, ROOT_HLI_FIELD, "", &root_data).code == 0);
    check_stub_field(ROOT_STORED_FIELD);

    printf("context_lifecycle_test ending-child-removes-only-its-own-record: ending the child "
           "released only the child's record, leaving the parent's translation intact\n");
}

static void scenario_ending_root_removes_only_its_own_record(void) {
    int pulse_ctx = -1;
    CHECK(open_dataentry(&pulse_ctx).code == 0);
    int operation_ctx = -1;
    CHECK(open_global(pulse_ctx, "equilibrium", "", &operation_ctx).code == 0);
    int time_slice_ctx = open_time_slice(operation_ctx);

    CHECK(al_end_action(operation_ctx).code == 0);

    /* The root's own record is gone: a read through it is now a plain
     * forward. */
    void *root_data = NULL;
    CHECK(read_data(operation_ctx, ROOT_HLI_FIELD, "", &root_data).code == 0);
    check_stub_field(ROOT_HLI_FIELD);

    /* The still-live child is untouched: closing a parent before its child
     * never removes the child's own record. */
    void *child_data = NULL;
    CHECK(read_data(time_slice_ctx, CHILD_HLI_FIELD, "", &child_data).code == 0);
    check_stub_field(CHILD_STORED_FIELD);

    printf("context_lifecycle_test ending-root-removes-only-its-own-record: ending the root "
           "released only the root's record, leaving the still-live child's translation intact\n");
}

/* --- A failed al_end_action leaves the record intact ---------------------- */

static void scenario_failed_end_action_leaves_the_record_intact(void) {
    int pulse_ctx = -1;
    CHECK(open_dataentry(&pulse_ctx).code == 0);
    int operation_ctx = -1;
    CHECK(open_global(pulse_ctx, "equilibrium", "", &operation_ctx).code == 0);

    CHECK(setenv("RECORDING_STUB_END_ACTION_FAIL", "1", 1) == 0);
    al_status_t status = al_end_action(operation_ctx);
    CHECK(status.code != 0);
    /* ctxID was still forwarded faithfully: the failure comes from the stub
     * after receiving it unchanged, not from the shim withholding it. */
    CHECK(int_from_stub("recording_stub_end_action_call_count") == 1);
    CHECK(int_from_stub("recording_stub_end_action_ctx_id") == operation_ctx);
    CHECK(unsetenv("RECORDING_STUB_END_ACTION_FAIL") == 0);

    void *data = NULL;
    CHECK(read_data(operation_ctx, ROOT_HLI_FIELD, "", &data).code == 0);
    check_stub_field(ROOT_STORED_FIELD);

    printf("context_lifecycle_test failed-end-action-leaves-the-record-intact: a refused close "
           "left the record live for a later read to still translate\n");
}

/* --- A recycled context ID cannot observe the released record ------------- */

static void scenario_recycled_id_cannot_observe_the_released_record(void) {
    int pulse_ctx = -1;
    CHECK(open_dataentry(&pulse_ctx).code == 0);

    CHECK(setenv("RECORDING_STUB_STAMP_VERSION", "3.39.0", 1) == 0);
    int operation_ctx = -1;
    CHECK(open_global(pulse_ctx, "equilibrium", "", &operation_ctx).code == 0);
    CHECK(al_end_action(operation_ctx).code == 0);

    /* IMAS-Core hands back the same raw context ID (the stub's fixed global-
     * action octxID) for this second, unrelated open, whose stamp now
     * matches the HLI DD version and therefore needs no conversion record at
     * all. */
    CHECK(setenv("RECORDING_STUB_STAMP_VERSION", "4.1.1", 1) == 0);
    int reopened_ctx = -1;
    CHECK(open_global(pulse_ctx, "equilibrium", "", &reopened_ctx).code == 0);
    CHECK(reopened_ctx == operation_ctx);

    /* A shim that failed to clear the old, mismatched record at this raw ID
     * would still translate this field. The new, matching-version open must
     * instead forward it unchanged. */
    void *data = NULL;
    CHECK(read_data(reopened_ctx, ROOT_HLI_FIELD, "", &data).code == 0);
    check_stub_field(ROOT_HLI_FIELD);

    printf("context_lifecycle_test recycled-id-cannot-observe-the-released-record: a later open "
           "reusing the same raw ID never exposed the ended occurrence's conversion record\n");
}

/* --- al_iterate_over_arraystruct forwards unchanged, no registry mutation - */

static void scenario_iterate_over_arraystruct_forwards_unchanged_and_mutates_nothing(void) {
    int pulse_ctx = -1;
    CHECK(open_dataentry(&pulse_ctx).code == 0);
    int operation_ctx = -1;
    CHECK(open_global(pulse_ctx, "equilibrium", "", &operation_ctx).code == 0);
    int time_slice_ctx = open_time_slice(operation_ctx);

    CHECK(al_iterate_over_arraystruct(time_slice_ctx, 1).code == 0);
    CHECK(int_from_stub("recording_stub_iterate_call_count") == 1);
    CHECK(int_from_stub("recording_stub_iterate_aosctx") == time_slice_ctx);
    CHECK(int_from_stub("recording_stub_iterate_step") == 1);

    /* The child's own record is untouched by iteration: a relative read
     * through it still translates exactly as before the call. */
    void *data = NULL;
    CHECK(read_data(time_slice_ctx, CHILD_HLI_FIELD, "", &data).code == 0);
    check_stub_field(CHILD_STORED_FIELD);

    printf("context_lifecycle_test iterate-over-arraystruct-forwards-unchanged-and-mutates-"
           "nothing: aosctx and step reached IMAS-Core unchanged and the registry was untouched\n");
}

/* --- al_close_pulse forwards unchanged, never mutates the registry ------- */

static void scenario_close_pulse_forwards_unchanged_and_never_mutates_the_registry(void) {
    int pulse_ctx = -1;
    CHECK(open_dataentry(&pulse_ctx).code == 0);

    CHECK(setenv("RECORDING_STUB_STAMP_VERSION", "3.39.0", 1) == 0);
    int operation_ctx = -1;
    CHECK(open_global(pulse_ctx, "equilibrium", "", &operation_ctx).code == 0);
    /* First use: the mismatch is discovered by this call's own stamp read,
     * so datapath is still forwarded unchanged here. */
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), "") == 0);

    CHECK(al_close_pulse(pulse_ctx, 42).code == 0);
    CHECK(int_from_stub("recording_stub_close_pulse_call_count") == 1);
    CHECK(int_from_stub("recording_stub_close_pulse_ctx") == pulse_ctx);
    CHECK(int_from_stub("recording_stub_close_pulse_mode") == 42);

    /* Reopening the same occurrence under the same pulse still translates
     * datapath from the mismatch cached by the very first open: al_close_pulse
     * did not clear it. */
    int reopened_ctx = -1;
    CHECK(open_global(pulse_ctx, "equilibrium", "time_slice/global_quantities/beta_tor_norm",
                       &reopened_ctx)
              .code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), ROOT_STORED_FIELD) == 0);

    printf("context_lifecycle_test close-pulse-forwards-unchanged-and-never-mutates-the-registry: "
           "pulseCtx and mode reached IMAS-Core unchanged and the occurrence cache survived\n");
}

/* --- Ending a data-entry context leaves live operation/child records ------ */

static void scenario_ending_dataentry_context_leaves_live_operation_and_child_records_intact(
    void) {
    int pulse_ctx = -1;
    CHECK(open_dataentry(&pulse_ctx).code == 0);
    int operation_ctx = -1;
    CHECK(open_global(pulse_ctx, "equilibrium", "", &operation_ctx).code == 0);
    int time_slice_ctx = open_time_slice(operation_ctx);

    CHECK(al_end_action(pulse_ctx).code == 0);

    void *root_data = NULL;
    CHECK(read_data(operation_ctx, ROOT_HLI_FIELD, "", &root_data).code == 0);
    check_stub_field(ROOT_STORED_FIELD);

    void *child_data = NULL;
    CHECK(read_data(time_slice_ctx, CHILD_HLI_FIELD, "", &child_data).code == 0);
    check_stub_field(CHILD_STORED_FIELD);

    /* The pulse's own record — the occurrence-version cache the very first
     * open above populated — is itself gone: a later open reusing the same
     * raw pulse ID no longer finds a cached mismatch to translate datapath
     * from, even though the stamp is still the same mismatched version. A
     * shim that failed to remove the data-entry record on this end_action
     * would still translate this datapath. */
    int reopened_ctx = -1;
    CHECK(open_global(pulse_ctx, "equilibrium", ROOT_HLI_FIELD, &reopened_ctx).code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), ROOT_HLI_FIELD) == 0);

    printf("context_lifecycle_test ending-dataentry-context-leaves-live-operation-and-child-"
           "records-intact: ending the pulse removed only its own record\n");
}

int main(int argc, char **argv) {
    static const shim_test_scenario scenarios[] = {
        {"ending-child-removes-only-its-own-record", scenario_ending_child_removes_only_its_own_record},
        {"ending-root-removes-only-its-own-record", scenario_ending_root_removes_only_its_own_record},
        {"failed-end-action-leaves-the-record-intact", scenario_failed_end_action_leaves_the_record_intact},
        {"recycled-id-cannot-observe-the-released-record", scenario_recycled_id_cannot_observe_the_released_record},
        {"iterate-over-arraystruct-forwards-unchanged-and-mutates-nothing", scenario_iterate_over_arraystruct_forwards_unchanged_and_mutates_nothing},
        {"close-pulse-forwards-unchanged-and-never-mutates-the-registry", scenario_close_pulse_forwards_unchanged_and_never_mutates_the_registry},
        {"ending-dataentry-context-leaves-live-operation-and-child-records-intact", scenario_ending_dataentry_context_leaves_live_operation_and_child_records_intact},
    };
    return RUN_NAMED_SCENARIO(argc, argv, scenarios);
}
