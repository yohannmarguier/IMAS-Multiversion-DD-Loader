/* Issue #216: graph-selected runtime-map tracer through the public C ABI.
 *
 * The executable intentionally knows nothing about map construction.  It
 * opens and operates through the same ABI and recording stub as the retained
 * XML mechanism suites; only CMake selects the graph-backed shim instance.
 */

#include <dlfcn.h>
#include <string.h>

#ifndef RECORDING_STUB_PATH
#error "RECORDING_STUB_PATH must be defined by CMakeLists.txt"
#endif

#include "../support/shim_test_support.h"

typedef al_status_t (*read_data_fn)(int, const char *, const char *, void **, int, int, int *);
typedef void (*set_reentrant_read_fn)(read_data_fn, const char *);

static void arm_reentrant_read(const char *field) {
    set_reentrant_read_fn arm =
        (set_reentrant_read_fn)stub_symbol_or_die("recording_stub_set_reentrant_read");
    arm(al_read_data, field);
}

static void scenario_identity_operations(void) {
    int operation_ctx = open_mismatched_equilibrium();
    void *read_data = NULL;
    int read_size[1] = {0};
    double write_data[] = {12.5};
    int write_size[] = {1};

    CHECK_OK(al_read_data(operation_ctx, "time", "time", &read_data, IMAS_DOUBLE_DATA, 1,
                          read_size));
    CHECK(read_data != NULL);
    CHECK(strcmp(read_data, "recording-stub: read data payload") == 0);
    CHECK(read_size[0] == 4004);
    CHECK(strcmp(string_from_stub("recording_stub_read_field"), "time") == 0);
    CHECK(strcmp(string_from_stub("recording_stub_read_timebase"), "time") == 0);

    CHECK_OK(al_write_data(operation_ctx, "time", "time", write_data, IMAS_DOUBLE_DATA, 1,
                           write_size));
    CHECK(strcmp(string_from_stub("recording_stub_write_field"), "time") == 0);
    CHECK(strcmp(string_from_stub("recording_stub_write_timebase"), "time") == 0);
    CHECK(int_from_stub("recording_stub_write_double_count") == 1);
    CHECK(double_at_from_stub("recording_stub_write_double_at", 0) == 12.5);

    CHECK_OK(al_delete_data(operation_ctx, "time"));
    CHECK(strcmp(string_from_stub("recording_stub_delete_path"), "time") == 0);
    check_no_loss_entry(operation_ctx);

    printf("graph_runtime_map_test identity-operations: graph-acquired map kept identity "
           "read, write and leaf delete on the existing C ABI\n");
}

static void scenario_renamed_read(const char *caller_path, const char *stored_path) {
    int operation_ctx = open_mismatched_equilibrium();
    void *read_data = NULL;
    int read_size[1] = {0};

    CHECK_OK(al_read_data(operation_ctx, caller_path, "time", &read_data, IMAS_DOUBLE_DATA, 1,
                          read_size));
    CHECK(read_data != NULL);
    CHECK(strcmp(string_from_stub("recording_stub_read_field"), stored_path) == 0);
    CHECK(strcmp(string_from_stub("recording_stub_read_timebase"), "time") == 0);
    check_no_loss_entry(operation_ctx);
}

static void scenario_renamed_write(const char *caller_path, const char *stored_path) {
    int operation_ctx = open_mismatched_equilibrium();
    double write_data[] = {12.5};
    int write_size[] = {1};

    CHECK_OK(al_write_data(operation_ctx, caller_path, "time", write_data, IMAS_DOUBLE_DATA, 1,
                           write_size));
    CHECK(strcmp(string_from_stub("recording_stub_write_field"), stored_path) == 0);
    CHECK(strcmp(string_from_stub("recording_stub_write_timebase"), "time") == 0);
    check_no_loss_entry(operation_ctx);
}

static void scenario_renamed_delete(const char *caller_path, const char *stored_path) {
    int operation_ctx = open_mismatched_equilibrium();

    CHECK_OK(al_delete_data(operation_ctx, caller_path));
    CHECK(strcmp(string_from_stub("recording_stub_delete_path"), stored_path) == 0);
    check_no_loss_entry(operation_ctx);
}

static void scenario_renamed_read_hli_new(void) {
    scenario_renamed_read("time_slice/global_quantities/beta_tor_norm",
                          "time_slice/global_quantities/beta_normal");
}

static void scenario_renamed_read_hli_old(void) {
    scenario_renamed_read("time_slice/global_quantities/beta_normal",
                          "time_slice/global_quantities/beta_tor_norm");
}

static void scenario_renamed_write_hli_new(void) {
    scenario_renamed_write("time_slice/global_quantities/beta_tor_norm",
                           "time_slice/global_quantities/beta_normal");
}

static void scenario_renamed_write_hli_old(void) {
    scenario_renamed_write("time_slice/global_quantities/beta_normal",
                           "time_slice/global_quantities/beta_tor_norm");
}

static void scenario_renamed_delete_hli_new(void) {
    scenario_renamed_delete("time_slice/global_quantities/beta_tor_norm",
                            "time_slice/global_quantities/beta_normal");
}

static void scenario_renamed_delete_hli_old(void) {
    scenario_renamed_delete("time_slice/global_quantities/beta_normal",
                            "time_slice/global_quantities/beta_tor_norm");
}

static void scenario_scientific_gate_refuses_caller_path(void) {
    int operation_ctx = open_mismatched_equilibrium();
    void *read_data = (void *)1;
    int read_size[1] = {73};
    int reads_before = int_from_stub("recording_stub_read_call_count");
    const char *caller_path = "time_slice/constraints/j_phi";

    al_status_t status = al_read_data(operation_ctx, caller_path, "time", &read_data,
                                      IMAS_DOUBLE_DATA, 1, read_size);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status, "this path has no safe conversion between DD versions", caller_path,
                          "4.1.1", "3.39.0");
    CHECK(int_from_stub("recording_stub_read_call_count") == reads_before);
    CHECK(read_data == (void *)1);
    CHECK(read_size[0] == 73);
    check_loss_at(operation_ctx, 0, caller_path, IMAS_MVDD_FIDELITY_UNMAPPABLE,
                  IMAS_MVDD_LOSS_OPERATION_READ);

    printf("graph_runtime_map_test scientific-gate-refuses-caller-path: a direct name "
           "correspondence without value evidence refused before IMAS-Core\n");
}

static void scenario_acquisition_failure_cleans_up_open_context(void) {
    int pulse_ctx = -1;
    int opened_ctx = -1;
    int losses = -1;

    CHECK_OK(al_begin_dataentry_action("imas:hdf5?path=/tmp/pulse", 7, &pulse_ctx));
    al_status_t status = al_begin_global_action(pulse_ctx, "unsupported_ids", "", 30, &opened_ctx);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status, "conversion map acquisition failed", "unsupported_ids", "4.1.1",
                          "3.39.0");
    CHECK(strcmp(string_from_stub("recording_stub_global_dataobjectname"), "unsupported_ids") == 0);
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), "") == 0);
    CHECK(int_from_stub("recording_stub_global_rwmode") == 30);
    CHECK(int_from_stub("recording_stub_end_action_call_count") == 1);
    CHECK(int_from_stub("recording_stub_end_action_ctx_id") == opened_ctx);
    CHECK_OK(imas_mvdd_context_loss_count(opened_ctx, &losses));
    CHECK(losses == 0);

    /* A failed first acquisition must not leave a mismatch cache entry. If
     * it did, this datapath-bearing retry would fail before Core sees it. */
    int retried_ctx = -1;
    status = al_begin_global_action(pulse_ctx, "unsupported_ids", "time", 30, &retried_ctx);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(int_from_stub("recording_stub_global_call_count") == 2);
    CHECK(int_from_stub("recording_stub_end_action_call_count") == 2);
    CHECK(int_from_stub("recording_stub_end_action_ctx_id") == retried_ctx);

    printf("graph_runtime_map_test acquisition-failure-cleans-up-open-context: refused "
           "unsupported graph map without retaining a context\n");
}

static void scenario_opening_families_reuse_a_retained_map(void) {
    int pulse_ctx = -1;
    int context = -1;
    double dtime = 0.0;
    int dtime_shape = 0;

    CHECK_OK(al_begin_dataentry_action("imas:hdf5?path=/tmp/pulse", 7, &pulse_ctx));

    /* The first opening acquires the complete scope. The source then becomes
     * unavailable, so all later openings prove process-life map retention. */
    CHECK_OK(al_begin_global_action(pulse_ctx, "equilibrium_once/1", "time", 30, &context));
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), "time") == 0);
    CHECK_OK(al_end_action(context));

    /* Reopening the same occurrence also takes the cached-mismatch datapath
     * route; this controlled map resolves its proven identity spelling. */
    CHECK_OK(al_begin_global_action(pulse_ctx, "equilibrium_once/1", "time", 30, &context));
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), "time") == 0);
    CHECK_OK(al_end_action(context));

    CHECK_OK(al_begin_slice_action(pulse_ctx, "equilibrium_once/2", 30, 1.5, 0, &context));
    CHECK(strcmp(string_from_stub("recording_stub_slice_dataobjectname"),
                 "equilibrium_once/2") == 0);
    CHECK_OK(al_end_action(context));

    CHECK_OK(al_begin_timerange_action(pulse_ctx, "equilibrium_once/3", 30, 1.0, 2.0, &dtime,
                                       &dtime_shape, 0, &context));
    CHECK(strcmp(string_from_stub("recording_stub_timerange_dataobjectname"),
                 "equilibrium_once/3") == 0);
    CHECK_OK(al_end_action(context));

    CHECK_OK(al_plugin_begin_global_action(pulse_ctx, "equilibrium_once/4", "time", 30,
                                           &context));
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"),
                 "al_plugin_begin_global_action") == 0);
    CHECK_OK(al_plugin_end_action(context));

    CHECK_OK(al_plugin_begin_slice_action(pulse_ctx, "equilibrium_once/5", 30, 1.5, 0, &context));
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"),
                 "al_plugin_begin_slice_action") == 0);
    CHECK_OK(al_plugin_end_action(context));

    printf("graph_runtime_map_test opening-families-reuse-a-retained-map: global, slice, "
           "timerange and plugin openings reused one retained graph map\n");
}

static void scenario_cached_mismatch_translates_global_datapath(void) {
    const char *hli_path = "time_slice/global_quantities/beta_tor_norm";
    const char *stored_path = "time_slice/global_quantities/beta_normal";
    int pulse_ctx = -1;
    int context = -1;

    CHECK_OK(al_begin_dataentry_action("imas:hdf5?path=/tmp/pulse", 7, &pulse_ctx));
    CHECK_OK(al_begin_global_action(pulse_ctx, "equilibrium/1", hli_path, 30, &context));
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), hli_path) == 0);
    CHECK_OK(al_end_action(context));

    CHECK_OK(al_begin_global_action(pulse_ctx, "equilibrium/1", hli_path, 30, &context));
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), stored_path) == 0);
    CHECK_OK(al_end_action(context));

    printf("graph_runtime_map_test cached-mismatch-translates-global-datapath: a retained "
           "graph map translated the second open before Core\n");
}

static void scenario_write_mode_uses_its_read_op_stamp_probe(void) {
    int pulse_ctx = -1;
    int context = -1;

    CHECK_OK(al_begin_dataentry_action("imas:hdf5?path=/tmp/pulse", 7, &pulse_ctx));
    CHECK_OK(al_begin_global_action(pulse_ctx, "equilibrium", "time", 31, &context));
    CHECK(int_from_stub("recording_stub_global_rwmode") == 31);
    CHECK(int_from_stub("recording_stub_plugin_call_count") == 2);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"), "al_plugin_end_action")
          == 0);
    CHECK(int_from_stub("recording_stub_read_call_count") == 1);
    CHECK_OK(al_end_action(context));

    printf("graph_runtime_map_test write-mode-uses-its-read-op-stamp-probe: graph-backed "
           "write opening discovered the stamp through a separate read context\n");
}

static void scenario_nonmismatch_opening_families_are_passthrough(void) {
    int pulse_ctx = -1;
    int context = -1;
    double dtime = 0.0;
    int dtime_shape = 0;

    CHECK_OK(al_begin_dataentry_action("imas:hdf5?path=/tmp/pulse", 7, &pulse_ctx));
    CHECK_OK(al_begin_global_action(pulse_ctx, "equilibrium", "time", 30, &context));
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), "time") == 0);
    check_no_loss_entry(context);
    CHECK_OK(al_end_action(context));
    CHECK_OK(al_begin_slice_action(pulse_ctx, "equilibrium", 30, 1.5, 0, &context));
    CHECK(strcmp(string_from_stub("recording_stub_slice_dataobjectname"), "equilibrium") == 0);
    CHECK_OK(al_end_action(context));
    CHECK_OK(al_begin_timerange_action(pulse_ctx, "equilibrium", 30, 1.0, 2.0, &dtime,
                                       &dtime_shape, 0, &context));
    CHECK(strcmp(string_from_stub("recording_stub_timerange_dataobjectname"), "equilibrium")
          == 0);
    CHECK_OK(al_end_action(context));
    CHECK_OK(al_plugin_begin_global_action(pulse_ctx, "equilibrium", "time", 30, &context));
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"),
                 "al_plugin_begin_global_action") == 0);
    CHECK_OK(al_plugin_end_action(context));
    CHECK_OK(al_plugin_begin_slice_action(pulse_ctx, "equilibrium", 30, 1.5, 0, &context));
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"),
                 "al_plugin_begin_slice_action") == 0);
    CHECK_OK(al_plugin_end_action(context));

    printf("graph_runtime_map_test nonmismatch-opening-families-are-passthrough: absent or "
           "matching stamps did not acquire or register conversion\n");
}

static void scenario_malformed_stamp_refuses_and_ends_every_family(void) {
    int pulse_ctx = -1;
    int context = -1;
    double dtime = 0.0;
    int dtime_shape = 0;

    CHECK_OK(al_begin_dataentry_action("imas:hdf5?path=/tmp/pulse", 7, &pulse_ctx));
    al_status_t status = al_begin_global_action(pulse_ctx, "equilibrium", "time", 30, &context);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(strstr(status.message, "malformed") != NULL);
    CHECK(int_from_stub("recording_stub_end_action_call_count") == 1);
    CHECK(int_from_stub("recording_stub_end_action_ctx_id") == context);
    status = al_begin_slice_action(pulse_ctx, "equilibrium", 30, 1.5, 0, &context);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(int_from_stub("recording_stub_end_action_call_count") == 2);
    CHECK(int_from_stub("recording_stub_end_action_ctx_id") == context);
    status = al_begin_timerange_action(pulse_ctx, "equilibrium", 30, 1.0, 2.0, &dtime,
                                       &dtime_shape, 0, &context);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(int_from_stub("recording_stub_end_action_call_count") == 3);
    CHECK(int_from_stub("recording_stub_end_action_ctx_id") == context);
    status = al_plugin_begin_global_action(pulse_ctx, "equilibrium", "time", 30, &context);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"), "al_plugin_end_action")
          == 0);
    CHECK(int_from_stub("recording_stub_plugin_last_ctx") == context);
    status = al_plugin_begin_slice_action(pulse_ctx, "equilibrium", 30, 1.5, 0, &context);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"), "al_plugin_end_action")
          == 0);
    CHECK(int_from_stub("recording_stub_plugin_last_ctx") == context);

    printf("graph_runtime_map_test malformed-stamp-refuses-and-ends-every-family: preserved "
           "stamp refusal and cleanup\n");
}

static void scenario_conversion_disabled_is_a_plain_forward(void) {
    int pulse_ctx = -1;
    int context = -1;

    CHECK_OK(al_begin_dataentry_action("imas:hdf5?path=/tmp/pulse", 7, &pulse_ctx));
    CHECK_OK(al_begin_global_action(pulse_ctx, "equilibrium", "time", 30, &context));
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), "time") == 0);
    CHECK(int_from_stub("recording_stub_read_call_count") == 0);
    CHECK_OK(al_end_action(context));
    CHECK_OK(al_begin_slice_action(pulse_ctx, "equilibrium", 30, 1.5, 0, &context));
    CHECK_OK(al_end_action(context));
    CHECK_OK(al_begin_timerange_action(pulse_ctx, "equilibrium", 30, 1.0, 2.0, NULL, NULL, 0,
                                       &context));
    CHECK_OK(al_end_action(context));
    CHECK_OK(al_plugin_begin_global_action(pulse_ctx, "equilibrium", "time", 30, &context));
    CHECK_OK(al_plugin_end_action(context));
    CHECK_OK(al_plugin_begin_slice_action(pulse_ctx, "equilibrium", 30, 1.5, 0, &context));
    CHECK_OK(al_plugin_end_action(context));
    CHECK(int_from_stub("recording_stub_read_call_count") == 0);

    printf("graph_runtime_map_test conversion-disabled-is-a-plain-forward: no graph work "
           "occurred without a latched HLI DD version\n");
}

static void scenario_core_failure_is_a_plain_forward(void) {
    int pulse_ctx = -1;
    int context = -1;

    CHECK_OK(al_begin_dataentry_action("imas:hdf5?path=/tmp/pulse", 7, &pulse_ctx));
    al_status_t status =
        al_begin_slice_action(pulse_ctx, "equilibrium", 30, 1.5, 0, &context);
    CHECK(status.code == -9);
    CHECK(strcmp(status.message, "recording-stub: slice open refused") == 0);
    CHECK(strcmp(string_from_stub("recording_stub_slice_dataobjectname"), "equilibrium") == 0);
    CHECK(int_from_stub("recording_stub_read_call_count") == 0);

    printf("graph_runtime_map_test core-failure-is-a-plain-forward: a Core slice failure did not "
           "trigger stamp discovery or graph acquisition\n");
}

static void scenario_failure_closes_every_opening_family(void) {
    int pulse_ctx = -1;
    int context = -1;
    double dtime = 0.0;
    int dtime_shape = 0;

    CHECK_OK(al_begin_dataentry_action("imas:hdf5?path=/tmp/pulse", 7, &pulse_ctx));

    al_status_t status = al_begin_global_action(pulse_ctx, "unsupported_ids", "", 30, &context);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status, "conversion map acquisition failed", "unsupported_ids", "4.1.1",
                          "3.39.0");
    CHECK(int_from_stub("recording_stub_end_action_call_count") == 1);
    CHECK(int_from_stub("recording_stub_end_action_ctx_id") == context);
    check_no_loss_entry(context);

    status = al_begin_slice_action(pulse_ctx, "unsupported_ids", 30, 1.5, 0, &context);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status, "conversion map acquisition failed", "unsupported_ids", "4.1.1",
                          "3.39.0");
    CHECK(int_from_stub("recording_stub_end_action_call_count") == 2);
    CHECK(int_from_stub("recording_stub_end_action_ctx_id") == context);
    check_no_loss_entry(context);

    status = al_begin_timerange_action(pulse_ctx, "unsupported_ids", 30, 1.0, 2.0, &dtime,
                                       &dtime_shape, 0, &context);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status, "conversion map acquisition failed", "unsupported_ids", "4.1.1",
                          "3.39.0");
    CHECK(int_from_stub("recording_stub_end_action_call_count") == 3);
    CHECK(int_from_stub("recording_stub_end_action_ctx_id") == context);
    check_no_loss_entry(context);

    status = al_plugin_begin_global_action(pulse_ctx, "unsupported_ids", "", 30, &context);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status, "conversion map acquisition failed", "unsupported_ids", "4.1.1",
                          "3.39.0");
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"), "al_plugin_end_action")
          == 0);
    CHECK(int_from_stub("recording_stub_plugin_last_ctx") == context);
    check_no_loss_entry(context);

    status = al_plugin_begin_slice_action(pulse_ctx, "unsupported_ids", 30, 1.5, 0, &context);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status, "conversion map acquisition failed", "unsupported_ids", "4.1.1",
                          "3.39.0");
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"), "al_plugin_end_action")
          == 0);
    CHECK(int_from_stub("recording_stub_plugin_last_ctx") == context);
    check_no_loss_entry(context);
    printf("graph_runtime_map_test failure-closes-every-opening-family: failed graph maps "
           "left no usable Core context in ordinary or plugin families\n");
}

static void scenario_later_open_retries_a_failed_acquisition(void) {
    int pulse_ctx = -1;
    int context = -1;

    CHECK_OK(al_begin_dataentry_action("imas:hdf5?path=/tmp/pulse", 7, &pulse_ctx));
    al_status_t status =
        al_begin_global_action(pulse_ctx, "recovering_equilibrium", "time", 30, &context);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(int_from_stub("recording_stub_end_action_ctx_id") == context);

    CHECK_OK(al_begin_global_action(pulse_ctx, "recovering_equilibrium", "time", 30, &context));
    void *read_data = NULL;
    int read_size[1] = {0};
    CHECK_OK(al_read_data(context, "time", "time", &read_data, IMAS_DOUBLE_DATA, 1, read_size));
    CHECK(read_data != NULL);
    CHECK_OK(al_end_action(context));

    printf("graph_runtime_map_test later-open-retries-a-failed-acquisition: a new opening "
           "retried only after the prior terminal failure\n");
}

static void scenario_reentrant_read_is_passthrough_under_graph_open(void) {
    const char *reentrant_field = "time_slice/global_quantities/beta_tor_norm";
    int operation_ctx = open_mismatched_equilibrium();
    void *read_data = NULL;
    int read_size[1] = {0};

    arm_reentrant_read(reentrant_field);
    CHECK_OK(al_read_data(operation_ctx, "time", "time", &read_data, IMAS_DOUBLE_DATA, 1,
                          read_size));
    CHECK(int_from_stub("recording_stub_reentrant_call_count") == 1);
    CHECK(strcmp(string_from_stub("recording_stub_reentrant_seen_field"), reentrant_field) == 0);
    CHECK(strcmp(string_from_stub("recording_stub_reentrant_seen_timebase"), "") == 0);
    CHECK_OK(al_end_action(operation_ctx));

    printf("graph_runtime_map_test reentrant-read-is-passthrough-under-graph-open: a Core "
           "callback kept its already-stored path unchanged\n");
}

static void scenario_passthrough_is_unchanged_under_graph_open(void) {
    const char *hli_path = "time_slice/global_quantities/beta_tor_norm";
    int operation_ctx = open_mismatched_equilibrium();
    int calls_before = int_from_stub("recording_stub_plugin_call_count");

    CHECK_OK(al_bind_plugin(hli_path, "recording-plugin"));
    CHECK(int_from_stub("recording_stub_plugin_call_count") == calls_before + 1);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"), "al_bind_plugin") == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_first_string"), hli_path) == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_second_string"), "recording-plugin")
          == 0);
    CHECK_OK(al_end_action(operation_ctx));

    printf("graph_runtime_map_test passthrough-is-unchanged-under-graph-open: plugin binding "
           "kept the HLI spelling while graph conversion was live\n");
}

static const shim_test_scenario SCENARIOS[] = {
    {"identity-operations", scenario_identity_operations},
    {"renamed-read-hli-new", scenario_renamed_read_hli_new},
    {"renamed-read-hli-old", scenario_renamed_read_hli_old},
    {"renamed-write-hli-new", scenario_renamed_write_hli_new},
    {"renamed-write-hli-old", scenario_renamed_write_hli_old},
    {"renamed-delete-hli-new", scenario_renamed_delete_hli_new},
    {"renamed-delete-hli-old", scenario_renamed_delete_hli_old},
    {"scientific-gate-refuses-caller-path", scenario_scientific_gate_refuses_caller_path},
    {"acquisition-failure-cleans-up-open-context", scenario_acquisition_failure_cleans_up_open_context},
    {"opening-families-reuse-a-retained-map", scenario_opening_families_reuse_a_retained_map},
    {"cached-mismatch-translates-global-datapath",
     scenario_cached_mismatch_translates_global_datapath},
    {"write-mode-uses-its-read-op-stamp-probe", scenario_write_mode_uses_its_read_op_stamp_probe},
    {"nonmismatch-opening-families-are-passthrough",
     scenario_nonmismatch_opening_families_are_passthrough},
    {"malformed-stamp-refuses-and-ends-every-family",
     scenario_malformed_stamp_refuses_and_ends_every_family},
    {"conversion-disabled-is-a-plain-forward", scenario_conversion_disabled_is_a_plain_forward},
    {"core-failure-is-a-plain-forward", scenario_core_failure_is_a_plain_forward},
    {"failure-closes-every-opening-family", scenario_failure_closes_every_opening_family},
    {"later-open-retries-a-failed-acquisition", scenario_later_open_retries_a_failed_acquisition},
    {"reentrant-read-is-passthrough-under-graph-open",
     scenario_reentrant_read_is_passthrough_under_graph_open},
    {"passthrough-is-unchanged-under-graph-open",
     scenario_passthrough_is_unchanged_under_graph_open},
};

int main(int argc, char **argv) { return RUN_NAMED_SCENARIO(argc, argv, SCENARIOS); }
