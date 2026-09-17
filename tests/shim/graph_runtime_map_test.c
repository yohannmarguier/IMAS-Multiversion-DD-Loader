/* Issue #216: graph-selected runtime-map tracer through the public C ABI.
 *
 * The executable intentionally knows nothing about map construction.  It
 * opens and operates through the same ABI and recording stub as the retained
 * XML mechanism suites; only CMake selects the graph-backed shim instance.
 */

#include <string.h>

#ifndef RECORDING_STUB_PATH
#error "RECORDING_STUB_PATH must be defined by CMakeLists.txt"
#endif

#include "../support/shim_test_support.h"

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

static const shim_test_scenario SCENARIOS[] = {
    {"identity-operations", scenario_identity_operations},
    {"acquisition-failure-cleans-up-open-context", scenario_acquisition_failure_cleans_up_open_context},
};

int main(int argc, char **argv) { return RUN_NAMED_SCENARIO(argc, argv, SCENARIOS); }
