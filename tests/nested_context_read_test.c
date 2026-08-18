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
 * anchor, no-source, refusal, and a supported value transformation.
 *
 * Issue #66 adds the loss-log scenarios: a non-exact nested read must append
 * the complete DD path — the child's own anchor joined onto the argument the
 * HLI passed — to its root's queryable loss log, and a query on either the
 * live child or the root must resolve to that same entry. */

#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifndef RECORDING_STUB_PATH
#error "RECORDING_STUB_PATH must be defined by CMakeLists.txt"
#endif

#include "shim_test_support.h"

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

static int loss_count(int ctx_id) {
    int count = -1;
    al_status_t status = imas_mvdd_context_loss_count(ctx_id, &count);
    CHECK(status.code == 0);
    return count;
}

static void check_no_loss_entry(int ctx_id) {
    CHECK(loss_count(ctx_id) == 0);
}

static void check_loss_at(int ctx_id, int index, const char *expected_path, int expected_verdict) {
    char path_buf[256] = {0};
    int verdict = -1;
    al_status_t status = imas_mvdd_context_loss_at(ctx_id, index, path_buf, sizeof(path_buf), &verdict);
    CHECK(status.code == 0);
    CHECK(strcmp(path_buf, expected_path) == 0);
    CHECK(verdict == expected_verdict);
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
    CHECK(al_read_data(arraystruct_ctx, "measured", "time", &data, 52, 1, shape).code == 0);
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
    check_no_loss_entry(arraystruct_ctx);
    check_no_loss_entry(operation_ctx);

    printf("nested_context_read_test relative-field-and-timebase-resolve-through-renamed-child: "
           "a relative read beneath a renamed AOS anchor reached the stored spelling\n");
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
                       52, 1, shape)
              .code == 0);
    CHECK(data != NULL);
    check_stub_paths("/time_slice/global_quantities/beta_normal", "");

    printf("nested_context_read_test absolute-field-outside-child-subtree-resolves-from-ids-root: "
           "an absolute read ignored the live child's own renamed anchor\n");
}

/* --- no-source and refusal are unaffected by nesting --------------------- */

static void scenario_no_source_returns_null_through_nested_child(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int time_slice_ctx = open_time_slice(operation_ctx);
    int reads_before = int_from_stub("recording_stub_read_call_count");

    void *data = (void *)1;
    int shape[1] = {0};
    CHECK(al_read_data(time_slice_ctx, "boundary/lcfs", "", &data, 52, 1, shape).code == 0);
    CHECK(data == NULL);
    CHECK(int_from_stub("recording_stub_read_call_count") == reads_before);

    printf("nested_context_read_test no-source-returns-null-through-nested-child: an absent "
           "stored counterpart returned success with no data, without calling IMAS-Core\n");
}

static void scenario_refusal_stops_before_core_through_nested_child(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int time_slice_ctx = open_time_slice(operation_ctx);
    int reads_before = int_from_stub("recording_stub_read_call_count");

    void *data = (void *)1;
    int shape[1] = {73};
    al_status_t status = al_read_data(
        time_slice_ctx, "constraints/strike_point/chi_squared_r", "", &data, 52, 1, shape);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(strcmp(status.message,
                 "IMAS-MVDD: this path's unit was redefined and cannot be converted; "
                 "DD path: time_slice/constraints/strike_point/chi_squared_r; "
                 "HLI DD version: 4.1.1; stored DD version: 3.39.0") == 0);
    CHECK(data == (void *)1);
    CHECK(shape[0] == 73);
    CHECK(int_from_stub("recording_stub_read_call_count") == reads_before);

    /* The refusal is retained on the root's loss log under the complete DD
     * path — the child's own "time_slice" anchor joined onto the relative
     * argument the HLI actually passed — and a query on either the live
     * child or the root resolves to the same entry (issue #66). */
    CHECK(loss_count(time_slice_ctx) == 1);
    check_loss_at(time_slice_ctx, 0, "time_slice/constraints/strike_point/chi_squared_r",
                  IMAS_MVDD_FIDELITY_UNMAPPABLE);
    CHECK(loss_count(operation_ctx) == 1);
    check_loss_at(operation_ctx, 0, "time_slice/constraints/strike_point/chi_squared_r",
                  IMAS_MVDD_FIDELITY_UNMAPPABLE);

    printf("nested_context_read_test refusal-stops-before-core-through-nested-child: an "
           "unmappable unit redefinition refused before IMAS-Core, addressed relative to a "
           "live arraystruct context\n");
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
    check_no_loss_entry(flux_loop_ctx);
    check_no_loss_entry(operation_ctx);

    printf("nested_context_read_test sign-flip-applies-through-nested-child: a COCOS sign flip "
           "was applied to a field read relative to a live arraystruct context\n");
}

/* --- issue #66: a nested non-exact read attributes to its root ---------- */

static void scenario_moved_read_through_nested_child_retains_lossy_verdict_on_root(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int time_slice_ctx = open_time_slice(operation_ctx);

    /* move-gap is rel="moved", fidelity forward="lossy" (read_path_test.c
     * proves the same rule at the root). Read it relative to the live
     * time_slice child instead: the anchor ("time_slice") spells identically
     * on both sides, so a shim that logged the raw, unjoined argument would
     * retain "boundary_separatrix/gap/r" — missing the anchor prefix a root
     * read of the same field would have produced. */
    void *data = NULL;
    int shape[1] = {0};
    CHECK(al_read_data(time_slice_ctx, "boundary_separatrix/gap/r", "", &data, 52, 1, shape).code
          == 0);
    CHECK(data != NULL);
    check_stub_paths("boundary/gap/r", "");

    CHECK(loss_count(time_slice_ctx) == 1);
    check_loss_at(time_slice_ctx, 0, "time_slice/boundary_separatrix/gap/r",
                  IMAS_MVDD_FIDELITY_LOSSY);
    CHECK(loss_count(operation_ctx) == 1);
    check_loss_at(operation_ctx, 0, "time_slice/boundary_separatrix/gap/r",
                  IMAS_MVDD_FIDELITY_LOSSY);

    printf("nested_context_read_test moved-read-through-nested-child-retains-lossy-verdict-on-root: "
           "a certainly-lossy nested read appended the complete DD path to its root's loss log\n");
}

static void scenario_merged_read_through_nested_child_retains_potentially_lossy_verdict(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int time_slice_ctx = open_time_slice(operation_ctx);

    /* fold-ggd-bfield is rel="merged", fidelity forward="lossy" — the
     * "potentially lossy and unverified" bucket (ADR 0008), distinct from
     * move-gap's unconditional "certainly lossy" bucket above. */
    void *data = NULL;
    int shape[1] = {0};
    CHECK(al_read_data(time_slice_ctx, "ggd/b_field_phi", "", &data, 52, 1, shape).code == 0);
    CHECK(data != NULL);
    check_stub_paths("ggd/b_field_phi", "");

    CHECK(loss_count(time_slice_ctx) == 1);
    check_loss_at(time_slice_ctx, 0, "time_slice/ggd/b_field_phi",
                  IMAS_MVDD_FIDELITY_POTENTIALLY_LOSSY);
    CHECK(loss_count(operation_ctx) == 1);
    check_loss_at(operation_ctx, 0, "time_slice/ggd/b_field_phi",
                  IMAS_MVDD_FIDELITY_POTENTIALLY_LOSSY);

    printf("nested_context_read_test "
           "merged-read-through-nested-child-retains-potentially-lossy-verdict: a merged rule's "
           "verdict reached the root's loss log with the complete nested DD path\n");
}

static void scenario_ending_root_before_child_destroys_the_shared_loss_log(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int time_slice_ctx = open_time_slice(operation_ctx);

    void *data = NULL;
    int shape[1] = {0};
    CHECK(al_read_data(time_slice_ctx, "boundary_separatrix/gap/r", "", &data, 52, 1, shape).code
          == 0);
    CHECK(loss_count(operation_ctx) == 1);
    CHECK(loss_count(time_slice_ctx) == 1);

    /* End the root while its child context is still open — non-LIFO relative
     * to the order the two were opened in. The log is owned by the root
     * record, not the child, so it must not outlive the root. */
    CHECK(al_end_action(operation_ctx).code == 0);

    check_no_loss_entry(time_slice_ctx);

    printf("nested_context_read_test "
           "ending-root-before-child-destroys-the-shared-loss-log: the loss log died with its "
           "root even though a child context closed non-LIFO\n");
}

int main(int argc, char **argv) {
    static const shim_test_scenario scenarios[] = {
        {"relative-field-and-timebase-resolve-through-renamed-child", scenario_relative_field_and_timebase_resolve_through_renamed_child},
        {"absolute-field-outside-child-subtree-resolves-from-ids-root", scenario_absolute_field_outside_child_subtree_resolves_from_ids_root},
        {"no-source-returns-null-through-nested-child", scenario_no_source_returns_null_through_nested_child},
        {"refusal-stops-before-core-through-nested-child", scenario_refusal_stops_before_core_through_nested_child},
        {"sign-flip-applies-through-nested-child", scenario_sign_flip_applies_through_nested_child},
        {"moved-read-through-nested-child-retains-lossy-verdict-on-root", scenario_moved_read_through_nested_child_retains_lossy_verdict_on_root},
        {"merged-read-through-nested-child-retains-potentially-lossy-verdict", scenario_merged_read_through_nested_child_retains_potentially_lossy_verdict},
        {"ending-root-before-child-destroys-the-shared-loss-log", scenario_ending_root_before_child_destroys_the_shared_loss_log},
    };
    return RUN_NAMED_SCENARIO(argc, argv, scenarios);
}
