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
    {"acquisition-failure-cleans-up-open-context", scenario_acquisition_failure_cleans_up_open_context},
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
