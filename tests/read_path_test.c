/* Issue #56: public al_read_data ABI scenarios against the recording stub.
 *
 * Every scenario opens an equilibrium occurrence whose supplied stamp makes
 * its stored DD version differ from the HLI DD version. The recording stub
 * is only the external IMAS-Core substitute: calls enter the shim through
 * its public C ABI and observe its behavior through the arguments the stub
 * receives. */

#include <string.h>

#ifndef RECORDING_STUB_PATH
#error "RECORDING_STUB_PATH must be defined by CMakeLists.txt"
#endif

#include "shim_test_support.h"

static al_status_t read_data(int ctx_id, const char *field, const char *timebase, void **data) {
    int size[1] = {0};
    return al_read_data(ctx_id, field, timebase, data, 52 /* DOUBLE_DATA */, 1, size);
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

static void check_read_refusal(int operation_ctx, const char *field, int datatype,
                               const char *expected_message) {
    int reads_before = int_from_stub("recording_stub_read_call_count");
    int losses_before = loss_count(operation_ctx);
    void *data = (void *)1;
    int size[1] = {73};

    al_status_t status = al_read_data(operation_ctx, field, "", &data, datatype, 1, size);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(strcmp(status.message, expected_message) == 0);
    CHECK(data == (void *)1);
    CHECK(size[0] == 73);
    CHECK(int_from_stub("recording_stub_read_call_count") == reads_before);
    CHECK(loss_count(operation_ctx) == losses_before + 1);
    check_loss_at(operation_ctx, losses_before, field, IMAS_MVDD_FIDELITY_UNMAPPABLE);
}

static void scenario_translates_field_and_timebase_independently(void) {
    int operation_ctx = open_mismatched_equilibrium();
    void *data = NULL;

    CHECK(read_data(operation_ctx, "time_slice/global_quantities/beta_tor_norm", "time", &data)
              .code == 0);
    CHECK(data != NULL);
    check_stub_paths("time_slice/global_quantities/beta_normal", "time");
    check_no_loss_entry(operation_ctx);

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
    CHECK(loss_count(operation_ctx) == 1);
    check_loss_at(operation_ctx, 0, "time_slice/boundary/lcfs", IMAS_MVDD_FIDELITY_LOSSY);

    printf("read_path_test forward-direction-translates-and-reports-no-source: 3.39.0 HLI "
           "paths used 4.1.1 spellings or returned not found\\n");
}

static void scenario_identity_rule_returns_data(void) {
    int operation_ctx = open_mismatched_equilibrium();
    void *data = NULL;

    CHECK(read_data(operation_ctx, "time", "", &data).code == 0);
    CHECK(data != NULL);
    check_stub_paths("time", "");
    check_no_loss_entry(operation_ctx);

    printf("read_path_test identity-rule-returns-data: identity rule read the stored path\\n");
}

static void scenario_merged_read_retains_a_lossy_verdict_in_the_loss_log(void) {
    /* fold-ggd-bfield is rel="merged", fidelity forward="lossy" — the
     * "potentially lossy and unverified" bucket (ADR 0008). This scenario's
     * HLI DD version is 3.39.0, so translating its own field down to the
     * stored 4.1.1 spelling matches this rule in the forward direction. */
    int operation_ctx = open_mismatched_equilibrium();
    void *data = NULL;

    CHECK(read_data(operation_ctx, "time_slice/ggd/b_field_phi", "", &data).code == 0);
    CHECK(data != NULL);
    check_stub_paths("time_slice/ggd/b_field_phi", "");

    CHECK(loss_count(operation_ctx) == 1);
    check_loss_at(operation_ctx, 0, "time_slice/ggd/b_field_phi",
                  IMAS_MVDD_FIDELITY_POTENTIALLY_LOSSY);

    printf("read_path_test merged-read-retains-a-lossy-verdict-in-the-loss-log: a merged "
           "rule's lossy verdict reached the queryable loss log\\n");
}

static void scenario_moved_read_retains_a_lossy_verdict_in_the_loss_log(void) {
    /* move-gap is rel="moved", fidelity forward="lossy" — the "certainly
     * lossy" bucket (ADR 0008): an unconditional single-path rule, not a
     * merged/split candidate plan. Same HLI/stored pair as the merged
     * scenario above. */
    int operation_ctx = open_mismatched_equilibrium();
    void *data = NULL;

    CHECK(read_data(operation_ctx, "time_slice/boundary_separatrix/gap/r", "", &data).code == 0);
    CHECK(data != NULL);
    check_stub_paths("time_slice/boundary/gap/r", "");

    CHECK(loss_count(operation_ctx) == 1);
    check_loss_at(operation_ctx, 0, "time_slice/boundary_separatrix/gap/r", IMAS_MVDD_FIDELITY_LOSSY);

    printf("read_path_test moved-read-retains-a-lossy-verdict-in-the-loss-log: a plain "
           "moved rule's lossy verdict reached the queryable loss log\\n");
}

static void scenario_ending_context_destroys_its_loss_log(void) {
    int operation_ctx = open_mismatched_equilibrium();
    void *data = NULL;
    CHECK(read_data(operation_ctx, "time_slice/ggd/b_field_phi", "", &data).code == 0);
    CHECK(loss_count(operation_ctx) == 1);

    CHECK(al_end_action(operation_ctx).code == 0);

    check_no_loss_entry(operation_ctx);

    printf("read_path_test ending-context-destroys-its-loss-log: al_end_action removed the "
           "loss log along with its context\\n");
}

static void scenario_loss_count_null_output_is_refused(void) {
    int operation_ctx = open_mismatched_equilibrium();
    void *data = NULL;
    CHECK(read_data(operation_ctx, "time_slice/ggd/b_field_phi", "", &data).code == 0);

    al_status_t status = imas_mvdd_context_loss_count(operation_ctx, NULL);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);

    printf("read_path_test loss-count-null-output-is-refused: a null count output was "
           "rejected without dereferencing it\\n");
}

static void scenario_loss_at_null_path_buffer_is_refused(void) {
    int operation_ctx = open_mismatched_equilibrium();
    void *data = NULL;
    CHECK(read_data(operation_ctx, "time_slice/ggd/b_field_phi", "", &data).code == 0);
    int verdict = -1;

    al_status_t status = imas_mvdd_context_loss_at(operation_ctx, 0, NULL, 256, &verdict);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(verdict == -1);

    printf("read_path_test loss-at-null-path-buffer-is-refused: a null path buffer was "
           "rejected without writing to verdict\\n");
}

static void scenario_loss_at_null_verdict_is_refused(void) {
    int operation_ctx = open_mismatched_equilibrium();
    void *data = NULL;
    CHECK(read_data(operation_ctx, "time_slice/ggd/b_field_phi", "", &data).code == 0);
    char path_buf[256] = {0};

    al_status_t status = imas_mvdd_context_loss_at(operation_ctx, 0, path_buf, sizeof(path_buf), NULL);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(path_buf[0] == 0);

    printf("read_path_test loss-at-null-verdict-is-refused: a null verdict output was "
           "rejected without writing to the path buffer\\n");
}

static void scenario_loss_at_negative_index_is_refused(void) {
    int operation_ctx = open_mismatched_equilibrium();
    void *data = NULL;
    CHECK(read_data(operation_ctx, "time_slice/ggd/b_field_phi", "", &data).code == 0);
    char path_buf[256] = {0};
    int verdict = -1;

    al_status_t status = imas_mvdd_context_loss_at(operation_ctx, -1, path_buf, sizeof(path_buf), &verdict);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(verdict == -1);
    CHECK(path_buf[0] == 0);

    printf("read_path_test loss-at-negative-index-is-refused: a negative index was "
           "rejected safely\\n");
}

static void scenario_loss_at_out_of_range_index_is_refused(void) {
    int operation_ctx = open_mismatched_equilibrium();
    void *data = NULL;
    CHECK(read_data(operation_ctx, "time_slice/ggd/b_field_phi", "", &data).code == 0);
    CHECK(loss_count(operation_ctx) == 1);
    char path_buf[256] = {0};
    int verdict = -1;

    al_status_t status = imas_mvdd_context_loss_at(operation_ctx, 1, path_buf, sizeof(path_buf), &verdict);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(verdict == -1);
    CHECK(path_buf[0] == 0);

    printf("read_path_test loss-at-out-of-range-index-is-refused: an index at the reported "
           "count was rejected safely\\n");
}

static void scenario_loss_at_insufficient_buffer_is_refused(void) {
    int operation_ctx = open_mismatched_equilibrium();
    void *data = NULL;
    CHECK(read_data(operation_ctx, "time_slice/ggd/b_field_phi", "", &data).code == 0);
    char path_buf[4] = {'X', 'X', 'X', 'X'};
    int verdict = -1;

    al_status_t status = imas_mvdd_context_loss_at(operation_ctx, 0, path_buf, sizeof(path_buf), &verdict);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(verdict == -1);
    CHECK(path_buf[0] == 'X');

    printf("read_path_test loss-at-insufficient-buffer-is-refused: a too-small buffer was "
           "rejected without a partial write\\n");
}

static void scenario_merged_read_falls_through_to_next_candidate(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int reads_before = int_from_stub("recording_stub_read_call_count");
    void *data = NULL;
    CHECK(read_data(operation_ctx, "time_slice/ggd/b_field_phi", "", &data).code == 0);
    CHECK(data != NULL);
    CHECK(int_from_stub("recording_stub_read_call_count") == reads_before + 2);
    check_stub_paths("time_slice/ggd/b_field_tor", "");
}

static void scenario_merged_read_stops_at_first_candidate_with_data(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int reads_before = int_from_stub("recording_stub_read_call_count");
    void *data = NULL;
    CHECK(read_data(operation_ctx, "time_slice/ggd/b_field_phi", "", &data).code == 0);
    CHECK(data != NULL);
    CHECK(int_from_stub("recording_stub_read_call_count") == reads_before + 1);
    check_stub_paths("time_slice/ggd/b_field_phi", "");
}

static void scenario_merged_read_returns_not_found_when_all_candidates_are_absent(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int reads_before = int_from_stub("recording_stub_read_call_count");
    void *data = (void *)1;
    CHECK(read_data(operation_ctx, "time_slice/ggd/b_field_phi", "", &data).code == 0);
    CHECK(data == NULL);
    CHECK(int_from_stub("recording_stub_read_call_count") == reads_before + 2);
    check_stub_paths("time_slice/ggd/b_field_tor", "");
    check_no_loss_entry(operation_ctx);
}

static void scenario_split_plan_reads_and_flips_its_first_stored_destination(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int reads_before = int_from_stub("recording_stub_read_call_count");
    int size[1] = {0};
    void *data = NULL;
    CHECK(al_read_data(operation_ctx, "time_slice/global_quantities/psi_axis", "", &data,
                       52 /* DOUBLE_DATA */, 1, size)
              .code == 0);
    CHECK(data != NULL);
    CHECK(*(double *)data == -1.5);
    CHECK(int_from_stub("recording_stub_read_call_count") == reads_before + 1);
    check_stub_paths("time_slice/global_quantities/psi_axis", "");
    /* split-psi-axis is exact both directions: a candidate plan with a
     * COCOS sign flip applied still creates no loss entry. */
    check_no_loss_entry(operation_ctx);
}

static void scenario_reverse_split_read_flips_its_single_stored_source(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int reads_before = int_from_stub("recording_stub_read_call_count");
    int size[1] = {0};
    void *data = NULL;
    CHECK(al_read_data(operation_ctx, "time_slice/global_quantities/psi_axis", "", &data,
                       52 /* DOUBLE_DATA */, 1, size)
              .code == 0);
    CHECK(data != NULL);
    CHECK(*(double *)data == -1.5);
    CHECK(int_from_stub("recording_stub_read_call_count") == reads_before + 1);
    check_stub_paths("time_slice/global_quantities/psi_axis", "");
    check_no_loss_entry(operation_ctx);
}

static void scenario_no_source_returns_null_without_core_call(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int reads_before = int_from_stub("recording_stub_read_call_count");
    void *data = (void *)1;

    CHECK(read_data(operation_ctx, "time_slice/contour_tree/critical_point", "", &data).code ==
          0);
    CHECK(data == NULL);
    CHECK(int_from_stub("recording_stub_read_call_count") == reads_before);
    CHECK(loss_count(operation_ctx) == 1);
    check_loss_at(operation_ctx, 0, "time_slice/contour_tree/critical_point",
                  IMAS_MVDD_FIDELITY_LOSSY);

    printf("read_path_test no-source-returns-null-without-core-call: no stored path was read\\n");
}

static void scenario_rank_changing_retype_refuses_without_core_call(void) {
    int operation_ctx = open_mismatched_equilibrium();
    check_read_refusal(
        operation_ctx, "grids_ggd/grid/space/coordinates_type", 51 /* INTEGER_DATA */,
        "IMAS-MVDD: this path's container changed shape and cannot be served; "
        "DD path: grids_ggd/grid/space/coordinates_type; HLI DD version: 4.1.1; "
        "stored DD version: 3.39.0");

    printf("read_path_test rank-changing-retype-refuses-without-core-call: refusal preserved "
           "caller storage and never reached IMAS-Core\\n");
}

static void scenario_unit_redefinition_refuses_without_core_call(void) {
    int operation_ctx = open_mismatched_equilibrium();
    check_read_refusal(
        operation_ctx, "time_slice/constraints/strike_point/chi_squared_r", 52 /* DOUBLE_DATA */,
        "IMAS-MVDD: this path's unit was redefined and cannot be converted; "
        "DD path: time_slice/constraints/strike_point/chi_squared_r; "
        "HLI DD version: 4.1.1; stored DD version: 3.39.0");

    printf("read_path_test unit-redefinition-refuses-without-core-call: refusal preserved "
           "caller storage and never reached IMAS-Core\\n");
}

static void scenario_unsupported_sign_flip_types_refuse_without_core_call(void) {
    int operation_ctx = open_mismatched_equilibrium();
    const int unsupported_types[] = {51 /* INTEGER_DATA */, 53 /* COMPLEX_DATA */};

    for (size_t i = 0; i < sizeof unsupported_types / sizeof unsupported_types[0]; ++i) {
        check_read_refusal(
            operation_ctx, "time_slice/boundary/psi", unsupported_types[i],
            "IMAS-MVDD: value-transform execution requires DOUBLE_DATA and a rank no greater "
            "than MAXDIM; "
            "DD path: time_slice/boundary/psi; HLI DD version: 4.1.1; stored DD version: "
            "3.39.0");
    }

    printf("read_path_test unsupported-sign-flip-types-refuse-without-core-call: integer "
           "and complex reads were refused before IMAS-Core\\n");
}

static void scenario_sign_flip_array_negates_values_and_preserves_empty_double(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int reads_before = int_from_stub("recording_stub_read_call_count");
    int size[1] = {0};
    void *data = NULL;

    CHECK(al_read_data(operation_ctx, "time_slice/profiles_1d/psi", "", &data,
                       52 /* DOUBLE_DATA */, 1, size)
              .code == 0);
    CHECK(data != NULL);
    CHECK(size[0] == 4);
    double *values = (double *)data;
    CHECK(values[0] == -1.5);
    CHECK(values[1] == -9e40); /* EMPTY_DOUBLE stays untouched */
    CHECK(values[2] == -3.2);
    CHECK(values[3] == 4.0);
    CHECK(int_from_stub("recording_stub_read_call_count") == reads_before + 1);
    check_stub_paths("time_slice/profiles_1d/psi", "");

    printf("read_path_test sign-flip-array-negates-values-and-preserves-empty-double: every "
           "real element was negated and the EMPTY_DOUBLE sentinel was left unchanged\\n");
}

static void scenario_sign_flip_rank_exceeding_maxdim_refuses_without_core_call(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int reads_before = int_from_stub("recording_stub_read_call_count");
    void *data = (void *)1;
    int size[8] = {73, 73, 73, 73, 73, 73, 73, 73};

    al_status_t status = al_read_data(operation_ctx, "time_slice/boundary/psi", "", &data,
                                      52 /* DOUBLE_DATA */, 8 /* rank exceeds MAXDIM == 7 */, size);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(strcmp(status.message,
                 "IMAS-MVDD: value-transform execution requires DOUBLE_DATA and a rank no "
                 "greater than MAXDIM; "
                 "DD path: time_slice/boundary/psi; HLI DD version: 4.1.1; stored DD version: "
                 "3.39.0") == 0);
    CHECK(data == (void *)1);
    for (int i = 0; i < 8; ++i) {
        CHECK(size[i] == 73);
    }
    CHECK(int_from_stub("recording_stub_read_call_count") == reads_before);

    printf("read_path_test sign-flip-rank-exceeding-maxdim-refuses-without-core-call: a "
           "rank-8 sign-flip read was refused before IMAS-Core was ever called\\n");
}

static void scenario_sign_flip_invalid_shape_refuses_without_modifying_buffer(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int reads_before = int_from_stub("recording_stub_read_call_count");
    void *data = NULL;
    int size[3] = {0, 0, 0};

    /* Three extents just under INT_MAX overflow the dimension-product
     * multiplication on the third factor; the one real element the stub
     * actually returns must still come back unflipped. */
    al_status_t status = al_read_data(operation_ctx, "time_slice/profiles_1d/psi", "", &data,
                                      52 /* DOUBLE_DATA */, 3, size);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(strcmp(status.message,
                 "IMAS-MVDD: value-transform execution received an invalid array shape; "
                 "DD path: time_slice/profiles_1d/psi; HLI DD version: 4.1.1; stored DD "
                 "version: 3.39.0") == 0);
    CHECK(data != NULL);
    CHECK(*(double *)data == 1.5);
    CHECK(int_from_stub("recording_stub_read_call_count") == reads_before + 1);

    printf("read_path_test sign-flip-invalid-shape-refuses-without-modifying-buffer: an "
           "overflowing dimension product was refused without flipping any element\\n");
}

static void scenario_sign_flip_shape_override_respects_read_rank(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int size[2] = {0, 73};
    void *data = NULL;

    CHECK(al_read_data(operation_ctx, "time_slice/profiles_1d/psi", "", &data,
                       52 /* DOUBLE_DATA */, 1, size)
              .code == 0);
    CHECK(data != NULL);
    CHECK(*(double *)data == -1.5);
    CHECK(size[0] == 1);
    CHECK(size[1] == 73);

    printf("read_path_test sign-flip-shape-override-respects-read-rank: the recording "
           "stub changed only the one extent the ABI supplied\\n");
}

static void scenario_sign_flip_not_found_skips_value_transformation(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int reads_before = int_from_stub("recording_stub_read_call_count");
    int size[1] = {73};
    void *data = (void *)1;

    CHECK(al_read_data(operation_ctx, "time_slice/profiles_1d/psi", "", &data,
                       52 /* DOUBLE_DATA */, 1, size)
              .code == 0);
    CHECK(data == NULL);
    CHECK(size[0] == 0);
    CHECK(int_from_stub("recording_stub_read_call_count") == reads_before + 1);
    check_stub_paths("time_slice/profiles_1d/psi", "");

    printf("read_path_test sign-flip-not-found-skips-value-transformation: a successful "
           "not-found COCOS read remained null\\n");
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
    check_no_loss_entry(operation_ctx);

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
    check_no_loss_entry(operation_ctx);

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
    check_no_loss_entry(operation_ctx);

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
    check_no_loss_entry(operation_ctx);

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
                "merged-read-falls-through-to-next-candidate|"
                "merged-read-stops-at-first-candidate-with-data|"
                "merged-read-returns-not-found-when-all-candidates-are-absent|"
                "split-plan-reads-and-flips-its-first-stored-destination|"
                "reverse-split-read-flips-its-single-stored-source|"
                "no-source-returns-null-without-core-call|"
                "rank-changing-retype-refuses-without-core-call|"
                "unit-redefinition-refuses-without-core-call|"
                "unsupported-sign-flip-types-refuse-without-core-call|"
                "sign-flip-array-negates-values-and-preserves-empty-double|"
                "sign-flip-rank-exceeding-maxdim-refuses-without-core-call|"
                "sign-flip-invalid-shape-refuses-without-modifying-buffer|"
                "sign-flip-shape-override-respects-read-rank|"
                "sign-flip-not-found-skips-value-transformation|"
                "resolves-relative-field-and-absolute-timebase|"
                "matching-context-bypasses-conversion|unknown-context-bypasses-conversion|"
                "unstamped-context-bypasses-conversion|conversion-disabled-bypasses-conversion|"
                "core-failure-propagates-unchanged|"
                "merged-read-retains-a-lossy-verdict-in-the-loss-log|"
                "moved-read-retains-a-lossy-verdict-in-the-loss-log|"
                "ending-context-destroys-its-loss-log|"
                "loss-count-null-output-is-refused|"
                "loss-at-null-path-buffer-is-refused|"
                "loss-at-null-verdict-is-refused|"
                "loss-at-negative-index-is-refused|"
                "loss-at-out-of-range-index-is-refused|"
                "loss-at-insufficient-buffer-is-refused>\\n",
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
    if (strcmp(argv[1], "merged-read-falls-through-to-next-candidate") == 0) {
        scenario_merged_read_falls_through_to_next_candidate(); return 0;
    }
    if (strcmp(argv[1], "merged-read-stops-at-first-candidate-with-data") == 0) {
        scenario_merged_read_stops_at_first_candidate_with_data(); return 0;
    }
    if (strcmp(argv[1], "merged-read-returns-not-found-when-all-candidates-are-absent") == 0) {
        scenario_merged_read_returns_not_found_when_all_candidates_are_absent(); return 0;
    }
    if (strcmp(argv[1], "split-plan-reads-and-flips-its-first-stored-destination") == 0) {
        scenario_split_plan_reads_and_flips_its_first_stored_destination(); return 0;
    }
    if (strcmp(argv[1], "reverse-split-read-flips-its-single-stored-source") == 0) {
        scenario_reverse_split_read_flips_its_single_stored_source(); return 0;
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
    if (strcmp(argv[1], "sign-flip-array-negates-values-and-preserves-empty-double") == 0) {
        scenario_sign_flip_array_negates_values_and_preserves_empty_double();
        return 0;
    }
    if (strcmp(argv[1], "sign-flip-rank-exceeding-maxdim-refuses-without-core-call") == 0) {
        scenario_sign_flip_rank_exceeding_maxdim_refuses_without_core_call();
        return 0;
    }
    if (strcmp(argv[1], "sign-flip-invalid-shape-refuses-without-modifying-buffer") == 0) {
        scenario_sign_flip_invalid_shape_refuses_without_modifying_buffer();
        return 0;
    }
    if (strcmp(argv[1], "sign-flip-shape-override-respects-read-rank") == 0) {
        scenario_sign_flip_shape_override_respects_read_rank();
        return 0;
    }
    if (strcmp(argv[1], "sign-flip-not-found-skips-value-transformation") == 0) {
        scenario_sign_flip_not_found_skips_value_transformation();
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
    if (strcmp(argv[1], "merged-read-retains-a-lossy-verdict-in-the-loss-log") == 0) {
        scenario_merged_read_retains_a_lossy_verdict_in_the_loss_log();
        return 0;
    }
    if (strcmp(argv[1], "moved-read-retains-a-lossy-verdict-in-the-loss-log") == 0) {
        scenario_moved_read_retains_a_lossy_verdict_in_the_loss_log();
        return 0;
    }
    if (strcmp(argv[1], "ending-context-destroys-its-loss-log") == 0) {
        scenario_ending_context_destroys_its_loss_log();
        return 0;
    }
    if (strcmp(argv[1], "loss-count-null-output-is-refused") == 0) {
        scenario_loss_count_null_output_is_refused();
        return 0;
    }
    if (strcmp(argv[1], "loss-at-null-path-buffer-is-refused") == 0) {
        scenario_loss_at_null_path_buffer_is_refused();
        return 0;
    }
    if (strcmp(argv[1], "loss-at-null-verdict-is-refused") == 0) {
        scenario_loss_at_null_verdict_is_refused();
        return 0;
    }
    if (strcmp(argv[1], "loss-at-negative-index-is-refused") == 0) {
        scenario_loss_at_negative_index_is_refused();
        return 0;
    }
    if (strcmp(argv[1], "loss-at-out-of-range-index-is-refused") == 0) {
        scenario_loss_at_out_of_range_index_is_refused();
        return 0;
    }
    if (strcmp(argv[1], "loss-at-insufficient-buffer-is-refused") == 0) {
        scenario_loss_at_insufficient_buffer_is_refused();
        return 0;
    }
    fprintf(stderr, "unknown scenario: %s\\n", argv[1]);
    return 2;
}
