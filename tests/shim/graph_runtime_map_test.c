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

    printf("graph_runtime_map_test identity-operations: declaration-only unit evidence kept "
           "identity read, write and leaf delete on the existing C ABI\n");
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

static int open_moved_parent_gap(int *operation_ctx_out) {
    int operation_ctx = open_mismatched_occurrence("moved_descendants", NULL);
    int size = -1;
    int parent_ctx = -1;
    int gap_ctx = -1;
    const char *new_parent = "/time_slice/current/profiles_1d";
    const char *old_parent = "/time_slice/legacy/profiles_1d";

    CHECK_OK(al_begin_arraystruct_action(operation_ctx, new_parent, "/time", &size, &parent_ctx));
    CHECK(strcmp(string_from_stub("recording_stub_arraystruct_path"), old_parent) == 0);
    CHECK(strcmp(string_from_stub("recording_stub_arraystruct_timebase"), "/time") == 0);
    CHECK_OK(al_begin_arraystruct_action(parent_ctx, "gap", "/time", &size, &gap_ctx));
    CHECK(strcmp(string_from_stub("recording_stub_arraystruct_path"), "gap") == 0);
    CHECK(strcmp(string_from_stub("recording_stub_arraystruct_timebase"), "/time") == 0);

    *operation_ctx_out = operation_ctx;
    return gap_ctx;
}

static void scenario_moved_parent_opens_nested_arraystruct(void) {
    int operation_ctx = -1;
    (void)open_moved_parent_gap(&operation_ctx);
    check_no_loss_entry(operation_ctx);

    printf("graph_runtime_map_test moved-parent-opens-nested-arraystruct: a relative parent "
           "move retained the stored child anchor\n");
}

static void scenario_moved_parent_reads_nested_path_and_timebase(void) {
    int operation_ctx = -1;
    int gap_ctx = open_moved_parent_gap(&operation_ctx);

    void *read_data = NULL;
    int read_size[1] = {0};
    CHECK_OK(al_read_data(gap_ctx, "r", "r", &read_data, IMAS_DOUBLE_DATA, 1, read_size));
    CHECK(read_data != NULL);
    CHECK(strcmp(string_from_stub("recording_stub_read_field"), "r") == 0);
    CHECK(strcmp(string_from_stub("recording_stub_read_timebase"), "r") == 0);
    check_no_loss_entry(operation_ctx);

    printf("graph_runtime_map_test moved-parent-reads-nested-path-and-timebase: a relative "
           "child read resolved field and timebase independently\n");
}

static void scenario_moved_parent_writes_absolute_path_and_timebase(void) {
    int operation_ctx = -1;
    (void)open_moved_parent_gap(&operation_ctx);
    const char *new_r = "/time_slice/current/profiles_1d/gap/r";
    const char *old_r = "/time_slice/legacy/profiles_1d/gap/r";

    double write_data[] = {12.5};
    int write_size[] = {1};
    CHECK_OK(al_write_data(operation_ctx, new_r, "/time", write_data, IMAS_DOUBLE_DATA, 1,
                           write_size));
    CHECK(strcmp(string_from_stub("recording_stub_write_field"), old_r) == 0);
    CHECK(strcmp(string_from_stub("recording_stub_write_timebase"), "/time") == 0);
    check_no_loss_entry(operation_ctx);

    printf("graph_runtime_map_test moved-parent-writes-absolute-path-and-timebase: a nested "
           "write resolved field and timebase independently\n");
}

static void scenario_moved_parent_deletes_a_relative_child(void) {
    int operation_ctx = -1;
    int gap_ctx = open_moved_parent_gap(&operation_ctx);

    CHECK_OK(al_delete_data(gap_ctx, "r"));
    CHECK(strcmp(string_from_stub("recording_stub_delete_path"), "r") == 0);
    check_no_loss_entry(operation_ctx);

    printf("graph_runtime_map_test moved-parent-deletes-a-relative-child: a nested delete "
           "resolved beneath the moved parent\n");
}

static void scenario_moved_parent_admits_a_trivial_child_delete(void) {
    int operation_ctx = open_mismatched_occurrence("moved_descendants", NULL);
    CHECK_OK(al_delete_data(operation_ctx, "/time_slice/current/profiles_1d/gap"));
    CHECK(strcmp(string_from_stub("recording_stub_delete_path"),
                 "/time_slice/legacy/profiles_1d/gap") == 0);
    check_no_loss_entry(operation_ctx);

    printf("graph_runtime_map_test moved-parent-admits-a-trivial-child-delete: the moved "
           "subtree retained the existing delete safety rule\n");
}

static void scenario_moved_parent_refuses_an_escaping_delete(void) {
    int operation_ctx = open_mismatched_occurrence("moved_descendants", NULL);
    int deletes_before = int_from_stub("recording_stub_delete_call_count");
    const char *caller_path = "time_slice/legacy/profiles_1d";

    al_status_t status = al_delete_data(operation_ctx, caller_path);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status,
                          "this subtree delete would leave data at a stored path outside the requested subtree",
                          caller_path, "3.39.0", "4.1.1");
    CHECK(int_from_stub("recording_stub_delete_call_count") == deletes_before);
    check_loss_at(operation_ctx, 0, caller_path, IMAS_MVDD_FIDELITY_UNMAPPABLE,
                  IMAS_MVDD_LOSS_OPERATION_DELETE);

    printf("graph_runtime_map_test moved-parent-refuses-an-escaping-delete: a child that "
           "leaves the moved subtree still protects the delete boundary\n");
}

static void scenario_graph_exact_gap_r_omits_the_xml_parent_loss(void) {
    int operation_ctx = open_mismatched_equilibrium();
    void *read_data = NULL;
    int read_size[1] = {0};
    const char *caller_path = "time_slice/boundary_separatrix/gap/r";

    CHECK_OK(al_read_data(operation_ctx, caller_path, "", &read_data, IMAS_DOUBLE_DATA, 1,
                          read_size));
    CHECK(read_data != NULL);
    CHECK(strcmp(read_data, "recording-stub: read data payload") == 0);
    CHECK(strcmp(string_from_stub("recording_stub_read_field"), "time_slice/boundary/gap/r") == 0);
    check_no_loss_entry(operation_ctx);

    printf("graph_runtime_map_test graph-exact-gap-r-omits-the-xml-parent-loss: graph "
           "evidence kept the XML path, payload and status while omitting inherited loss\n");
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

static void scenario_read_unit_refusal_preserves_caller_data_without_forwarding(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int reads_before = int_from_stub("recording_stub_read_call_count");
    int size[1] = {73};
    void *read_data = (void *)1;

    al_status_t status = al_read_data(operation_ctx, "unit_dimensionally_compatible", "time",
                                      &read_data, IMAS_DOUBLE_DATA, 1, size);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status, "this path has no safe conversion between DD versions",
                          "unit_dimensionally_compatible", "4.1.1", "3.39.0");
    CHECK(read_data == (void *)1);
    CHECK(size[0] == 73);
    CHECK(int_from_stub("recording_stub_read_call_count") == reads_before);
    check_loss_at(operation_ctx, 0, "unit_dimensionally_compatible",
                  IMAS_MVDD_FIDELITY_UNMAPPABLE, IMAS_MVDD_LOSS_OPERATION_READ);

    printf("graph_runtime_map_test read-unit-refusal-preserves-caller-data-without-forwarding: "
           "unresolved unit path refused before Core or caller-buffer mutation\n");
}

static void scenario_write_unit_refusal_preserves_caller_data_without_forwarding(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int writes_before = int_from_stub("recording_stub_write_call_count");
    int size[1] = {73};
    double write_data[] = {12.5};

    al_status_t status =
        al_write_data(operation_ctx, "unit_requires_scale_or_offset", "time", write_data,
                      IMAS_DOUBLE_DATA, 1, size);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status, "this path's unit was redefined and cannot be converted",
                          "unit_requires_scale_or_offset", "4.1.1", "3.39.0");
    CHECK(write_data[0] == 12.5);
    CHECK(size[0] == 73);
    CHECK(int_from_stub("recording_stub_write_call_count") == writes_before);
    check_loss_at(operation_ctx, 0, "unit_requires_scale_or_offset",
                  IMAS_MVDD_FIDELITY_UNMAPPABLE, IMAS_MVDD_LOSS_OPERATION_WRITE);

    printf("graph_runtime_map_test write-unit-refusal-preserves-caller-data-without-forwarding: "
           "known scale-or-offset path refused before Core or caller-buffer mutation\n");
}

static void scenario_timebase_resampling_refuses_write_without_forwarding(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int writes_before = int_from_stub("recording_stub_write_call_count");
    int write_size[1] = {73};
    double write_data[] = {12.5};

    al_status_t status = al_write_data(operation_ctx, "time", "resampling_timebase", write_data,
                                       IMAS_DOUBLE_DATA, 1, write_size);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status, "this path has no safe conversion between DD versions",
                          "resampling_timebase", "4.1.1", "3.39.0");
    CHECK(write_data[0] == 12.5);
    CHECK(write_size[0] == 73);
    CHECK(int_from_stub("recording_stub_write_call_count") == writes_before);

    check_loss_at(operation_ctx, 0, "resampling_timebase", IMAS_MVDD_FIDELITY_UNMAPPABLE,
                  IMAS_MVDD_LOSS_OPERATION_WRITE);

    printf("graph_runtime_map_test timebase-resampling-refuses-write-without-forwarding: "
           "a safe field could not mask an unsafe timebase at the write seam\n");
}

static void scenario_timebase_resampling_refuses_arraystruct_without_forwarding(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int arraystruct_before = int_from_stub("recording_stub_arraystruct_call_count");
    int array_size = 73;
    int child_ctx = -1;

    al_status_t status = al_begin_arraystruct_action(operation_ctx, "time", "resampling_timebase",
                                                      &array_size, &child_ctx);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status, "this path has no safe conversion between DD versions",
                          "resampling_timebase", "4.1.1", "3.39.0");
    CHECK(array_size == 73);
    CHECK(child_ctx == -1);
    CHECK(int_from_stub("recording_stub_arraystruct_call_count") == arraystruct_before);

    check_loss_at(operation_ctx, 0, "resampling_timebase", IMAS_MVDD_FIDELITY_UNMAPPABLE,
                  IMAS_MVDD_LOSS_OPERATION_READ);

    printf("graph_runtime_map_test timebase-resampling-refuses-arraystruct-without-forwarding: "
           "a safe path could not mask an unsafe timebase at the arraystruct seam\n");
}

static void scenario_delete_unit_refusal_does_not_forward(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int deletes_before = int_from_stub("recording_stub_delete_call_count");

    al_status_t status = al_delete_data(operation_ctx, "unit_requires_scale_or_offset");
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status, "this path's unit was redefined and cannot be converted",
                          "unit_requires_scale_or_offset", "4.1.1", "3.39.0");
    CHECK(int_from_stub("recording_stub_delete_call_count") == deletes_before);
    check_loss_at(operation_ctx, 0, "unit_requires_scale_or_offset",
                  IMAS_MVDD_FIDELITY_UNMAPPABLE, IMAS_MVDD_LOSS_OPERATION_DELETE);

    printf("graph_runtime_map_test delete-unit-refusal-does-not-forward: "
           "known scale-or-offset path refused before Core\n");
}

static void scenario_loss_unit_refusals_keep_operation_order(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int size[1] = {73};
    void *read_data = (void *)1;
    double write_data[] = {12.5};

    CHECK(al_read_data(operation_ctx, "unit_dimensionally_compatible", "time", &read_data,
                       IMAS_DOUBLE_DATA, 1, size)
              .code
          == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(al_write_data(operation_ctx, "unit_requires_scale_or_offset", "time", write_data,
                        IMAS_DOUBLE_DATA, 1, size)
              .code
          == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(al_delete_data(operation_ctx, "unit_requires_scale_or_offset").code
          == IMAS_MVDD_CONVERSION_ERROR);

    check_loss_at(operation_ctx, 0, "unit_dimensionally_compatible",
                  IMAS_MVDD_FIDELITY_UNMAPPABLE, IMAS_MVDD_LOSS_OPERATION_READ);
    check_loss_at(operation_ctx, 1, "unit_requires_scale_or_offset",
                  IMAS_MVDD_FIDELITY_UNMAPPABLE, IMAS_MVDD_LOSS_OPERATION_WRITE);
    check_loss_at(operation_ctx, 2, "unit_requires_scale_or_offset",
                  IMAS_MVDD_FIDELITY_UNMAPPABLE, IMAS_MVDD_LOSS_OPERATION_DELETE);

    printf("graph_runtime_map_test loss-unit-refusals-keep-operation-order: "
           "read, write and delete losses retain their caller-path order\n");
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

static void scenario_psi_read_flips_once_and_write_uses_its_inverse(void) {
    int operation_ctx = open_mismatched_equilibrium();
    void *read_data = NULL;
    int read_size[1] = {0};
    double write_data[] = {2.5, -9.0E40};
    int write_size[] = {2};

    CHECK_OK(al_read_data(operation_ctx, "time_slice/profiles_1d/psi", "time", &read_data,
                          IMAS_DOUBLE_DATA, 1, read_size));
    CHECK(read_data != NULL);
    CHECK(read_size[0] == 3);
    CHECK(((double *)read_data)[0] == -1.5);
    CHECK(((double *)read_data)[1] == 2.0);
    CHECK(((double *)read_data)[2] == -9.0E40);
    CHECK(strcmp(string_from_stub("recording_stub_read_field"), "time_slice/profiles_1d/psi")
          == 0);
    CHECK(strcmp(string_from_stub("recording_stub_read_timebase"), "time") == 0);

    CHECK_OK(al_write_data(operation_ctx, "time_slice/profiles_1d/psi", "time", write_data,
                           IMAS_DOUBLE_DATA, 1, write_size));
    CHECK(strcmp(string_from_stub("recording_stub_write_field"), "time_slice/profiles_1d/psi")
          == 0);
    CHECK(strcmp(string_from_stub("recording_stub_write_timebase"), "time") == 0);
    CHECK(pointer_from_stub("recording_stub_write_data") != write_data);
    CHECK(int_from_stub("recording_stub_write_double_count") == 2);
    CHECK(double_at_from_stub("recording_stub_write_double_at", 0) == -2.5);
    CHECK(double_at_from_stub("recording_stub_write_double_at", 1) == -9.0E40);
    CHECK(write_data[0] == 2.5);
    CHECK(write_data[1] == -9.0E40);
    CHECK(write_size[0] == 2);

    /* A rank-zero EMPTY_DOUBLE reaches Core through the caller's scalar
     * storage and stays unset: it is neither flipped nor copied. */
    double empty_scalar = -9.0E40;
    void *empty_scalar_data = &empty_scalar;
    CHECK_OK(al_write_data(operation_ctx, "time_slice/profiles_1d/psi", "time",
                           empty_scalar_data, IMAS_DOUBLE_DATA, 0, NULL));
    CHECK(pointer_from_stub("recording_stub_write_data") == empty_scalar_data);
    CHECK(empty_scalar == -9.0E40);

    int reads_before_unsupported = int_from_stub("recording_stub_read_call_count");
    void *unsupported_data = (void *)1;
    int unsupported_size[1] = {73};
    al_status_t unsupported =
        al_read_data(operation_ctx, "time_slice/profiles_1d/psi", "time", &unsupported_data,
                     IMAS_INTEGER_DATA, 1, unsupported_size);
    CHECK(unsupported.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(strstr(unsupported.message, "value-transform execution requires DOUBLE_DATA") != NULL);
    CHECK(unsupported_data == (void *)1);
    CHECK(unsupported_size[0] == 73);
    CHECK(int_from_stub("recording_stub_read_call_count") == reads_before_unsupported);
    CHECK(loss_count(operation_ctx) == 1);
    check_loss_at(operation_ctx, 0, "time_slice/profiles_1d/psi", IMAS_MVDD_FIDELITY_UNMAPPABLE,
                  IMAS_MVDD_LOSS_OPERATION_READ);

    printf("graph_runtime_map_test psi-read-flips-once-and-write-uses-its-inverse: "
           "the graph-derived factor flips doubles once without mutating caller storage\n");
}

static void check_cocos_refusal(const char *ids) {
    int operation_ctx = open_mismatched_occurrence(ids, NULL);
    void *read_data = (void *)1;
    int read_size[1] = {73};
    double write_data = 2.5;
    int write_size[1] = {1};
    int reads_before = int_from_stub("recording_stub_read_call_count");
    int writes_before = int_from_stub("recording_stub_write_call_count");

    al_status_t status = al_read_data(operation_ctx, "time_slice/profiles_1d/psi", "time",
                                      &read_data, IMAS_DOUBLE_DATA, 1, read_size);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status, "this path has no safe conversion between DD versions",
                          "time_slice/profiles_1d/psi", "4.1.1", "3.39.0");
    CHECK(read_data == (void *)1);
    CHECK(read_size[0] == 73);
    CHECK(int_from_stub("recording_stub_read_call_count") == reads_before);

    status = al_write_data(operation_ctx, "time_slice/profiles_1d/psi", "time", &write_data,
                           IMAS_DOUBLE_DATA, 1, write_size);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status, "this path has no safe conversion between DD versions",
                          "time_slice/profiles_1d/psi", "4.1.1", "3.39.0");
    CHECK(write_data == 2.5);
    CHECK(write_size[0] == 1);
    CHECK(int_from_stub("recording_stub_write_call_count") == writes_before);
    CHECK(loss_count(operation_ctx) == 2);
    check_loss_at(operation_ctx, 0, "time_slice/profiles_1d/psi",
                  IMAS_MVDD_FIDELITY_UNMAPPABLE, IMAS_MVDD_LOSS_OPERATION_READ);
    check_loss_at(operation_ctx, 1, "time_slice/profiles_1d/psi",
                  IMAS_MVDD_FIDELITY_UNMAPPABLE, IMAS_MVDD_LOSS_OPERATION_WRITE);

}

static void scenario_unknown_cocos_refuses_only_the_affected_operations(void) {
    check_cocos_refusal("unknown_cocos");
    printf("graph_runtime_map_test unknown-cocos-refuses-only-the-affected-operations: "
           "the C ABI retains refusal losses without changing caller buffers\n");
}

static void scenario_compound_cocos_refuses_only_the_affected_operations(void) {
    check_cocos_refusal("compound_cocos");
    printf("graph_runtime_map_test compound-cocos-refuses-only-the-affected-operations: "
           "the C ABI does not treat a compound expression as a sign flip\n");
}

static void scenario_missing_cocos_refuses_only_the_affected_operations(void) {
    check_cocos_refusal("missing_cocos");
    printf("graph_runtime_map_test missing-cocos-refuses-only-the-affected-operations: "
           "the C ABI keeps an absent endpoint convention unknown\n");
}

static const shim_test_scenario SCENARIOS[] = {
    {"identity-operations", scenario_identity_operations},
    {"renamed-read-hli-new", scenario_renamed_read_hli_new},
    {"renamed-read-hli-old", scenario_renamed_read_hli_old},
    {"renamed-write-hli-new", scenario_renamed_write_hli_new},
    {"renamed-write-hli-old", scenario_renamed_write_hli_old},
    {"renamed-delete-hli-new", scenario_renamed_delete_hli_new},
    {"renamed-delete-hli-old", scenario_renamed_delete_hli_old},
    {"moved-parent-opens-nested-arraystruct", scenario_moved_parent_opens_nested_arraystruct},
    {"moved-parent-reads-nested-path-and-timebase",
     scenario_moved_parent_reads_nested_path_and_timebase},
    {"moved-parent-writes-absolute-path-and-timebase",
     scenario_moved_parent_writes_absolute_path_and_timebase},
    {"moved-parent-deletes-a-relative-child", scenario_moved_parent_deletes_a_relative_child},
    {"moved-parent-admits-a-trivial-child-delete",
     scenario_moved_parent_admits_a_trivial_child_delete},
    {"moved-parent-refuses-an-escaping-delete", scenario_moved_parent_refuses_an_escaping_delete},
    {"graph-exact-gap-r-omits-the-xml-parent-loss",
     scenario_graph_exact_gap_r_omits_the_xml_parent_loss},
    {"scientific-gate-refuses-caller-path", scenario_scientific_gate_refuses_caller_path},
    {"acquisition-failure-cleans-up-open-context", scenario_acquisition_failure_cleans_up_open_context},
    {"read-unit-refusal-preserves-caller-data-without-forwarding",
     scenario_read_unit_refusal_preserves_caller_data_without_forwarding},
    {"write-unit-refusal-preserves-caller-data-without-forwarding",
     scenario_write_unit_refusal_preserves_caller_data_without_forwarding},
    {"timebase-resampling-refuses-write-without-forwarding",
     scenario_timebase_resampling_refuses_write_without_forwarding},
    {"timebase-resampling-refuses-arraystruct-without-forwarding",
     scenario_timebase_resampling_refuses_arraystruct_without_forwarding},
    {"delete-unit-refusal-does-not-forward", scenario_delete_unit_refusal_does_not_forward},
    {"loss-unit-refusals-keep-operation-order", scenario_loss_unit_refusals_keep_operation_order},
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
    {"psi-read-flips-once-and-write-uses-its-inverse",
     scenario_psi_read_flips_once_and_write_uses_its_inverse},
    {"unknown-cocos-refuses-only-the-affected-operations",
     scenario_unknown_cocos_refuses_only_the_affected_operations},
    {"compound-cocos-refuses-only-the-affected-operations",
     scenario_compound_cocos_refuses_only_the_affected_operations},
    {"missing-cocos-refuses-only-the-affected-operations",
     scenario_missing_cocos_refuses_only_the_affected_operations},
};

int main(int argc, char **argv) { return RUN_NAMED_SCENARIO(argc, argv, SCENARIOS); }
