/* Public al_write_data / al_delete_data and al_plugin_write_data ABI scenarios
 * against the recording stub. */

#include <dlfcn.h>
#include <complex.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifndef RECORDING_STUB_PATH
#error "RECORDING_STUB_PATH must be defined by CMakeLists.txt"
#endif

#include "../support/shim_test_support.h"

static al_status_t write_field(int ctx_id, const char *field, const char *timebase, void *data,
                               int *size) {
    return al_write_data(ctx_id, field, timebase, data, IMAS_DOUBLE_DATA, 1, size);
}

typedef const char *(*delete_path_at_fn)(int);
typedef int (*data_event_kind_at_fn)(int);
typedef const char *(*data_event_path_at_fn)(int);
typedef al_status_t (*read_data_fn)(int, const char *, const char *, void **, int, int, int *);
typedef void (*set_reentrant_read_fn)(read_data_fn, const char *);

static const char *delete_path_at(int index) {
    return ((delete_path_at_fn)stub_symbol_or_die("recording_stub_delete_path_at"))(index);
}

static int data_event_kind_at(int index) {
    return ((data_event_kind_at_fn)stub_symbol_or_die("recording_stub_data_event_kind_at"))(index);
}

static const char *data_event_path_at(int index) {
    return ((data_event_path_at_fn)stub_symbol_or_die("recording_stub_data_event_path_at"))(index);
}

static void enable_probe_allocations(void) {
    CHECK(setenv("RECORDING_STUB_READ_ALLOCATE", "1", 1) == 0);
}

static void disable_probe_allocations(void) {
    unsetenv("RECORDING_STUB_READ_ALLOCATE");
}

static void arm_reentrant_read(read_data_fn callback, const char *field) {
    ((set_reentrant_read_fn)stub_symbol_or_die("recording_stub_set_reentrant_read"))(callback,
                                                                                       field);
}

static void check_write_lands(int ctx_id, const char *field, const char *timebase,
                              const char *stored_field, const char *stored_timebase) {
    int writes_before = int_from_stub("recording_stub_write_call_count");
    double sentinel = 42.0;
    int size[1] = {73};

    CHECK(write_field(ctx_id, field, timebase, &sentinel, size).code == 0);
    CHECK(int_from_stub("recording_stub_write_call_count") == writes_before + 1);
    CHECK(strcmp(string_from_stub("recording_stub_write_field"), stored_field) == 0);
    CHECK(strcmp(string_from_stub("recording_stub_write_timebase"), stored_timebase) == 0);
    CHECK(pointer_from_stub("recording_stub_write_data") == &sentinel);
    CHECK(sentinel == 42.0);
    CHECK(size[0] == 73);
}

static void check_delete_lands(int ctx_id, const char *path, const char *stored_path) {
    int deletes_before = int_from_stub("recording_stub_delete_call_count");
    CHECK(al_delete_data(ctx_id, path).code == 0);
    CHECK(int_from_stub("recording_stub_delete_call_count") == deletes_before + 1);
    CHECK(int_from_stub("recording_stub_delete_ctx") == ctx_id);
    CHECK(strcmp(string_from_stub("recording_stub_delete_path"), stored_path) == 0);
}

static void scenario_write_renamed_field_lands_at_stored_spelling(void) {
    int operation_ctx = open_mismatched_equilibrium();
    check_write_lands(operation_ctx, "time_slice/global_quantities/beta_tor_norm", "time",
                      "time_slice/global_quantities/beta_normal", "time");

    printf("write_delete_conversion_test write-renamed-field-lands-at-stored-spelling: a DD4 "
           "field landed at its DD3 spelling without mutating caller storage\n");
}

static void scenario_write_identity_and_moved_fields_land_at_stored_spelling(void) {
    int operation_ctx = open_mismatched_equilibrium();
    check_write_lands(operation_ctx, "time_slice/boundary/elongation", "time",
                      "time_slice/boundary/elongation", "time");
    check_write_lands(operation_ctx, "time_slice/boundary/closest_wall_point", "time",
                      "time_slice/boundary_separatrix/closest_wall_point", "time");

    printf("write_delete_conversion_test write-identity-and-moved-fields-land-at-stored-spelling: "
           "identity and moved DD4 fields landed at their DD3 spellings\n");
}

static void scenario_write_reverse_identity_renamed_and_moved_fields_land_at_stored_spelling(void) {
    int operation_ctx = open_mismatched_equilibrium();
    check_write_lands(operation_ctx, "time_slice/boundary/elongation", "time",
                      "time_slice/boundary/elongation", "time");
    check_write_lands(operation_ctx, "time_slice/global_quantities/beta_normal", "time",
                      "time_slice/global_quantities/beta_tor_norm", "time");
    check_write_lands(operation_ctx, "time_slice/boundary_separatrix/closest_wall_point", "time",
                      "time_slice/boundary/closest_wall_point", "time");

    printf("write_delete_conversion_test write-reverse-identity-renamed-and-moved-fields-land-at-stored-spelling: "
           "identity, renamed and moved DD3 fields landed at their DD4 spellings\n");
}

static void scenario_delete_identity_renamed_and_moved_fields_land_at_stored_spelling(void) {
    int operation_ctx = open_mismatched_equilibrium();
    check_delete_lands(operation_ctx, "time_slice/boundary/elongation",
                       "time_slice/boundary/elongation");
    check_delete_lands(operation_ctx, "time_slice/global_quantities/beta_tor_norm",
                       "time_slice/global_quantities/beta_normal");
    check_delete_lands(operation_ctx, "time_slice/boundary/closest_wall_point/r",
                       "time_slice/boundary_separatrix/closest_wall_point/r");
    CHECK(loss_count(operation_ctx) == 0);
}

static void scenario_delete_reverse_identity_renamed_and_moved_fields_land_at_stored_spelling(void) {
    int operation_ctx = open_mismatched_equilibrium();
    check_delete_lands(operation_ctx, "time_slice/boundary/elongation",
                       "time_slice/boundary/elongation");
    check_delete_lands(operation_ctx, "time_slice/global_quantities/beta_normal",
                       "time_slice/global_quantities/beta_tor_norm");
    check_delete_lands(operation_ctx, "time_slice/boundary_separatrix/closest_wall_point/r",
                       "time_slice/boundary/closest_wall_point/r");
}

static void scenario_plugin_write_renamed_field_lands_at_stored_spelling(void) {
    int operation_ctx = open_mismatched_equilibrium();
    double sentinel = 42.0;
    int size[1] = {73};
    al_status_t status = al_plugin_write_data(
        operation_ctx, "time_slice/global_quantities/beta_tor_norm", "time", &sentinel,
        IMAS_DOUBLE_DATA, 1, size);

    CHECK(status.code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_first_string"),
                 "time_slice/global_quantities/beta_normal") == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_second_string"), "time") == 0);
    CHECK(pointer_from_stub("recording_stub_plugin_pointer") == &sentinel);
    CHECK(sentinel == 42.0);
    CHECK(size[0] == 73);

    printf("write_delete_conversion_test plugin-write-renamed-field-lands-at-stored-spelling: "
           "the plugin reentry seam applied the ordinary write policy\n");
}

static void scenario_plugin_write_matching_context_forwards_unchanged(void) {
    int operation_ctx = open_mismatched_equilibrium();
    double sentinel = 7.5;
    int size[1] = {1};

    al_status_t status = al_plugin_write_data(
        operation_ctx, "time_slice/global_quantities/beta_tor_norm", "time", &sentinel,
        IMAS_DOUBLE_DATA, 1, size);

    CHECK(status.code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_first_string"),
                 "time_slice/global_quantities/beta_tor_norm") == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_second_string"), "time") == 0);
    CHECK(pointer_from_stub("recording_stub_plugin_pointer") == &sentinel);

    printf("write_delete_conversion_test plugin-write-matching-context-forwards-unchanged: a "
           "matching stamp was forwarded verbatim through the plugin reentry seam\n");
}

static void scenario_write_nested_child_context_resolves_relative_and_absolute_fields(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int size = -1;
    int arraystruct_ctx = -1;
    CHECK(al_begin_arraystruct_action(operation_ctx, "time_slice", "", &size, &arraystruct_ctx)
              .code == 0);

    check_write_lands(arraystruct_ctx, "global_quantities/beta_tor_norm", "",
                      "global_quantities/beta_normal", "");
    check_write_lands(arraystruct_ctx, "/time_slice/global_quantities/beta_tor_norm", "",
                      "/time_slice/global_quantities/beta_normal", "");

    printf("write_delete_conversion_test write-nested-child-context-resolves-relative-and-absolute-fields: "
           "a child write used its own anchor for relative paths and the IDS root for absolute paths\n");
}

static void scenario_write_candidate_lands_at_primary_and_retains_unwritten_candidates(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int writes_before = int_from_stub("recording_stub_write_call_count");
    double value = 42.0;
    int size[1] = {1};
    void *read_data = NULL;

    CHECK(write_field(operation_ctx, "time_slice/profiles_2d/b_field_phi", "time", &value, size)
              .code == 0);
    CHECK(int_from_stub("recording_stub_write_call_count") == writes_before + 1);
    CHECK(strcmp(string_from_stub("recording_stub_write_field"),
                 "time_slice/profiles_2d/b_field_phi") == 0);
    /* The entries name the two candidates left unwritten, by their own stored
     * 3.39.0 spellings — not the caller's `b_field_phi`, which is where the
     * value did land. Story 22 asks the log to say where a stale value may now
     * be found, so a repeated copy of the caller's own path would answer the
     * one question it exists to answer with nothing. `fold-p2d-bphi` declares
     * b_field_phi at precedence 1, b_field_tor at 2, b_tor at 3. */
    CHECK(loss_count(operation_ctx) == 2);
    check_loss_at(operation_ctx, 0, "time_slice/profiles_2d/b_field_tor",
                  IMAS_MVDD_FIDELITY_POTENTIALLY_LOSSY, IMAS_MVDD_LOSS_OPERATION_WRITE);
    check_loss_at(operation_ctx, 1, "time_slice/profiles_2d/b_tor",
                  IMAS_MVDD_FIDELITY_POTENTIALLY_LOSSY, IMAS_MVDD_LOSS_OPERATION_WRITE);
    check_no_write_lossy_verdict(operation_ctx);

    /* A CONSISTENCY CHECK, not a proof of what reached storage — issue #133's
     * acceptance criteria ask for that label to be here, so nobody later reads
     * this read as the on-disk claim it cannot make. The write flips
     * HLI-to-stored and this read flips stored-to-HLI, so the caller's own
     * value comes back whichever sign, spelling or fan-out actually landed;
     * what it does prove is that the two directions agree. The on-disk claims
     * are made natively, against real IMAS-Core, in
     * tests/real_core/write_delete_oracle_test.c (ADR 0016's "Consequences").
     * The recording stub exposes the value accepted at the stored spelling,
     * which is what lets this read close the loop at all. */
    CHECK(al_read_data(operation_ctx, "time_slice/profiles_2d/b_field_phi", "time", &read_data,
                       IMAS_DOUBLE_DATA, 1, size)
              .code == 0);
    CHECK(read_data != NULL);
    CHECK(*(double *)read_data == value);

    printf("write_delete_conversion_test write-candidate-lands-at-primary-and-retains-unwritten-candidates: "
           "only precedence 1 was written, the other candidates were retained as potential write losses, and the round trip is consistent (a consistency check, not an on-disk claim)\n");
}

static void scenario_write_non_primary_source_refuses_by_precedence(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int writes_before = int_from_stub("recording_stub_write_call_count");
    double value = 42.0;
    int size[1] = {1};
    al_status_t status = write_field(operation_ctx, "time_slice/global_quantities/psi_magnetic_axis",
                                     "time", &value, size);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status,
                          "this path is a non-primary source and cannot write a shared stored slot",
                          "time_slice/global_quantities/psi_magnetic_axis", "4.1.1", "3.39.0");
    CHECK(int_from_stub("recording_stub_write_call_count") == writes_before);
    CHECK(loss_count(operation_ctx) == 1);
    check_loss_at(operation_ctx, 0, "time_slice/global_quantities/psi_magnetic_axis",
                  IMAS_MVDD_FIDELITY_UNMAPPABLE, IMAS_MVDD_LOSS_OPERATION_WRITE);
}

static void scenario_write_split_candidate_lands_at_primary(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int writes_before = int_from_stub("recording_stub_write_call_count");
    double value = 42.0;
    int size[1] = {1};

    CHECK(write_field(operation_ctx, "time_slice/global_quantities/psi_axis", "time", &value, size)
              .code == 0);
    CHECK(int_from_stub("recording_stub_write_call_count") == writes_before + 1);
    CHECK(strcmp(string_from_stub("recording_stub_write_field"),
                 "time_slice/global_quantities/psi_axis") == 0);
    CHECK(pointer_from_stub("recording_stub_write_data") != &value);
    CHECK(double_at_from_stub("recording_stub_write_double_at", 0) == -value);
    CHECK(value == 42.0);
    /* `split-psi-axis` feeds the 3.39.0 psi_axis into two 4.1.1 slots. Only
     * precedence 1 is written, and the entry names the precedence-2 stored
     * spelling that keeps whatever it already held. */
    CHECK(loss_count(operation_ctx) == 1);
    check_loss_at(operation_ctx, 0, "time_slice/global_quantities/psi_magnetic_axis",
                  IMAS_MVDD_FIDELITY_POTENTIALLY_LOSSY, IMAS_MVDD_LOSS_OPERATION_WRITE);
    check_no_write_lossy_verdict(operation_ctx);
}

static void scenario_child_write_candidate_retains_complete_path_at_root(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int child_size = -1;
    int child_ctx = -1;
    double value = 42.0;
    int size[1] = {1};

    CHECK(al_begin_arraystruct_action(operation_ctx, "time_slice", "", &child_size, &child_ctx)
              .code == 0);
    CHECK(write_field(child_ctx, "profiles_2d/b_field_phi", "time", &value, size).code == 0);
    CHECK(loss_count(operation_ctx) == 2);
    CHECK(loss_count(child_ctx) == 2);
    /* The caller's argument was relative to the `time_slice` anchor, and the
     * stored candidate spellings are still reported as complete DD paths from
     * the IDS root — the anchor-stripped fragment IMAS-Core received would
     * not tell a draining caller where to look. */
    check_loss_at(child_ctx, 0, "time_slice/profiles_2d/b_field_tor",
                  IMAS_MVDD_FIDELITY_POTENTIALLY_LOSSY, IMAS_MVDD_LOSS_OPERATION_WRITE);
    check_loss_at(child_ctx, 1, "time_slice/profiles_2d/b_tor",
                  IMAS_MVDD_FIDELITY_POTENTIALLY_LOSSY, IMAS_MVDD_LOSS_OPERATION_WRITE);

    check_no_write_lossy_verdict(operation_ctx);
    check_no_write_lossy_verdict(child_ctx);
}

static void scenario_write_uses_the_primary_candidate_without_fanout(void) {
    int operation_ctx = open_mismatched_equilibrium();
    check_write_lands(operation_ctx, "time_slice/profiles_2d/b_field_phi", "time",
                      "time_slice/profiles_2d/b_field_phi", "time");

    printf("write_delete_conversion_test write-uses-the-primary-candidate-without-fanout: "
           "a write chose only precedence one while the paired delete fan-out removes all sources\n");
}

static void scenario_write_cocos_sign_flip_uses_a_shim_owned_rank_seven_copy(void) {
    int operation_ctx = open_mismatched_equilibrium();
    double scalar = 2.5;
    double values[128];
    int size[7] = {2, 2, 2, 2, 2, 2, 2};
    int writes_before = int_from_stub("recording_stub_write_call_count");
    for (int index = 0; index < 128; ++index) {
        values[index] = (double)(index - 64);
    }

    CHECK(al_write_data(operation_ctx, "time_slice/constraints/flux_loop/measured", "time", &scalar,
                        IMAS_DOUBLE_DATA, 0, NULL)
              .code == 0);
    CHECK(int_from_stub("recording_stub_write_call_count") == writes_before + 1);
    CHECK(pointer_from_stub("recording_stub_write_data") != &scalar);
    CHECK(int_from_stub("recording_stub_write_double_count") == 1);
    CHECK(double_at_from_stub("recording_stub_write_double_at", 0) == -2.5);
    CHECK(scalar == 2.5);

    CHECK(al_write_data(operation_ctx, "time_slice/constraints/flux_loop/measured", "time", values,
                        IMAS_DOUBLE_DATA, 7, size)
              .code == 0);
    CHECK(int_from_stub("recording_stub_write_call_count") == writes_before + 2);
    CHECK(pointer_from_stub("recording_stub_write_data") != values);
    CHECK(int_from_stub("recording_stub_write_double_count") == 128);
    for (int index = 0; index < 128; ++index) {
        CHECK(double_at_from_stub("recording_stub_write_double_at", index) == -values[index]);
        CHECK(values[index] == (double)(index - 64));
    }
    for (int index = 0; index < 7; ++index) {
        CHECK(size[index] == 2);
    }

    printf("write_delete_conversion_test write-cocos-sign-flip-uses-a-shim-owned-rank-seven-copy: "
           "Core received every negated value while caller storage stayed unchanged\n");
}

static void scenario_plugin_write_cocos_sign_flip_uses_a_shim_owned_copy(void) {
    int operation_ctx = open_mismatched_equilibrium();
    double values[2] = {1.25, -2.5};
    int size[1] = {2};

    CHECK(al_plugin_write_data(operation_ctx, "time_slice/constraints/flux_loop/measured", "time",
                               values, IMAS_DOUBLE_DATA, 1, size)
              .code == 0);
    CHECK(pointer_from_stub("recording_stub_plugin_pointer") != values);
    CHECK(int_from_stub("recording_stub_plugin_write_double_count") == 2);
    CHECK(double_at_from_stub("recording_stub_plugin_write_double_at", 0) == -1.25);
    CHECK(double_at_from_stub("recording_stub_plugin_write_double_at", 1) == 2.5);
    CHECK(values[0] == 1.25);
    CHECK(values[1] == -2.5);
    CHECK(size[0] == 2);

    printf("write_delete_conversion_test plugin-write-cocos-sign-flip-uses-a-shim-owned-copy: "
           "the plugin reentry seam used the same copy policy\n");
}

static void scenario_write_cocos_sentinel_forwards_unchanged_without_loss(void) {
    int operation_ctx = open_mismatched_equilibrium();
    double unset = -9.0E40;
    int unset_int = -999999999;
    double complex unset_complex = -9.0E40 - 9.0E40 * I;
    int writes_before = int_from_stub("recording_stub_write_call_count");

    CHECK(al_write_data(operation_ctx, "time_slice/constraints/flux_loop/measured", "time", &unset,
                        IMAS_DOUBLE_DATA, 0, NULL)
              .code == 0);
    CHECK(int_from_stub("recording_stub_write_call_count") == writes_before + 1);
    CHECK(pointer_from_stub("recording_stub_write_data") == &unset);
    CHECK(int_from_stub("recording_stub_write_double_count") == 1);
    CHECK(double_at_from_stub("recording_stub_write_double_at", 0) == -9.0E40);
    CHECK(unset == -9.0E40);
    CHECK(al_write_data(operation_ctx, "time_slice/constraints/flux_loop/measured", "time",
                        &unset_int, IMAS_INTEGER_DATA, 0, NULL)
              .code == 0);
    CHECK(pointer_from_stub("recording_stub_write_data") == &unset_int);
    CHECK(unset_int == -999999999);
    CHECK(al_write_data(operation_ctx, "time_slice/constraints/flux_loop/measured", "time",
                        &unset_complex, IMAS_COMPLEX_DATA, 0, NULL)
              .code == 0);
    CHECK(pointer_from_stub("recording_stub_write_data") == &unset_complex);
    CHECK(creal(unset_complex) == -9.0E40);
    CHECK(cimag(unset_complex) == -9.0E40);
    int loss_count = -1;
    CHECK_OK(imas_mvdd_context_loss_count(operation_ctx, &loss_count));
    CHECK(loss_count == 0);

    printf("write_delete_conversion_test write-cocos-sentinel-forwards-unchanged-without-loss: "
           "an unset scalar kept IMAS-Core's own skip sentinel\n");
}

static void scenario_write_cocos_invalid_shape_or_type_refuses_before_core(void) {
    int operation_ctx = open_mismatched_equilibrium();
    double value = 1.25;
    int oversized_shape[8] = {1, 1, 1, 1, 1, 1, 1, 1};
    int writes_before = int_from_stub("recording_stub_write_call_count");

    CHECK(al_write_data(operation_ctx, "time_slice/constraints/flux_loop/measured", "time", &value,
                        IMAS_INTEGER_DATA, 0, NULL)
              .code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(al_write_data(operation_ctx, "time_slice/constraints/flux_loop/measured", "time", &value,
                        IMAS_DOUBLE_DATA, 8, oversized_shape)
              .code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(int_from_stub("recording_stub_write_call_count") == writes_before);
    CHECK(value == 1.25);
    for (int index = 0; index < 8; ++index) {
        CHECK(oversized_shape[index] == 1);
    }

    printf("write_delete_conversion_test write-cocos-invalid-shape-or-type-refuses-before-core: "
           "the ADR-0010 gate rejected both unsupported declarations\n");
}

static void scenario_write_refuses_dd_version_stamp_but_forwards_its_siblings(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int writes_before = int_from_stub("recording_stub_write_call_count");
    double sentinel = 42.0;
    int size[1] = {73};

    CHECK(write_field(operation_ctx, "ids_properties/version_put/data_dictionary", "", &sentinel,
                      size)
              .code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(int_from_stub("recording_stub_write_call_count") == writes_before);
    check_write_lands(operation_ctx, "ids_properties/version_put/access_layer", "",
                      "ids_properties/version_put/access_layer", "");
    check_write_lands(operation_ctx, "ids_properties/version_put/access_layer_language", "",
                      "ids_properties/version_put/access_layer_language", "");

    printf("write_delete_conversion_test write-refuses-dd-version-stamp-but-forwards-its-siblings: "
           "the immutable stamp was protected while access-layer metadata remained plain writes\n");
}

static void scenario_write_without_stored_slot_refuses_and_retains_a_write_loss(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int writes_before = int_from_stub("recording_stub_write_call_count");
    double sentinel = 42.0;
    int size[1] = {73};
    al_status_t status = write_field(operation_ctx, "time_slice/boundary/phi", "", &sentinel, size);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status, "this path has no stored source", "time_slice/boundary/phi", "4.1.1",
                          "3.39.0");
    CHECK(int_from_stub("recording_stub_write_call_count") == writes_before);
    CHECK(sentinel == 42.0);
    CHECK(size[0] == 73);
    CHECK(loss_count(operation_ctx) == 1);
    check_loss_at(operation_ctx, 0, "time_slice/boundary/phi", IMAS_MVDD_FIDELITY_UNMAPPABLE,
                  IMAS_MVDD_LOSS_OPERATION_WRITE);

    printf("write_delete_conversion_test write-without-stored-slot-refuses-and-retains-a-write-loss: "
           "a DD4-only field was refused before Core and retained as a write loss\n");
}

static void scenario_write_reverse_without_stored_slot_refuses_and_retains_a_write_loss(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int writes_before = int_from_stub("recording_stub_write_call_count");
    double sentinel = 42.0;
    int size[1] = {73};
    al_status_t status = write_field(operation_ctx, "time_slice/boundary_secondary_separatrix/gap/r", "",
                                     &sentinel, size);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status, "this path has no stored source",
                          "time_slice/boundary_secondary_separatrix/gap/r", "3.39.0", "4.1.1");
    CHECK(int_from_stub("recording_stub_write_call_count") == writes_before);
    CHECK(sentinel == 42.0);
    CHECK(size[0] == 73);
    CHECK(loss_count(operation_ctx) == 1);
    check_loss_at(operation_ctx, 0, "time_slice/boundary_secondary_separatrix/gap/r",
                  IMAS_MVDD_FIDELITY_UNMAPPABLE, IMAS_MVDD_LOSS_OPERATION_WRITE);

    printf("write_delete_conversion_test write-reverse-without-stored-slot-refuses-and-retains-a-write-loss: "
           "a DD3-only field was refused before Core and retained as a write loss\n");
}

static void scenario_write_retyped_path_refuses_and_retains_a_write_loss(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int writes_before = int_from_stub("recording_stub_write_call_count");
    double sentinel = 42.0;
    int size[1] = {73};
    al_status_t status = write_field(operation_ctx, "grids_ggd/grid/space/coordinates_type", "", &sentinel,
                                     size);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status, "this path's container changed shape and cannot be served",
                          "grids_ggd/grid/space/coordinates_type", "4.1.1", "3.39.0");
    CHECK(int_from_stub("recording_stub_write_call_count") == writes_before);
    CHECK(sentinel == 42.0);
    CHECK(size[0] == 73);
    CHECK(loss_count(operation_ctx) == 1);
    check_loss_at(operation_ctx, 0, "grids_ggd/grid/space/coordinates_type",
                  IMAS_MVDD_FIDELITY_UNMAPPABLE, IMAS_MVDD_LOSS_OPERATION_WRITE);

    printf("write_delete_conversion_test write-retyped-path-refuses-and-retains-a-write-loss: a "
           "shape-changing path was refused before Core and retained as a write loss\n");
}

static void scenario_child_write_refusal_is_retained_on_its_root_with_a_complete_path(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int size = -1;
    int arraystruct_ctx = -1;
    CHECK(al_begin_arraystruct_action(operation_ctx, "time_slice", "", &size, &arraystruct_ctx)
              .code == 0);

    int writes_before = int_from_stub("recording_stub_write_call_count");
    double sentinel = 42.0;
    int shape[1] = {73};
    CHECK(write_field(arraystruct_ctx, "boundary/phi", "", &sentinel, shape).code ==
          IMAS_MVDD_CONVERSION_ERROR);
    CHECK(int_from_stub("recording_stub_write_call_count") == writes_before);
    CHECK(sentinel == 42.0);
    CHECK(shape[0] == 73);
    CHECK(loss_count(arraystruct_ctx) == 1);
    check_loss_at(arraystruct_ctx, 0, "time_slice/boundary/phi", IMAS_MVDD_FIDELITY_UNMAPPABLE,
                  IMAS_MVDD_LOSS_OPERATION_WRITE);
    CHECK(loss_count(operation_ctx) == 1);
    check_loss_at(operation_ctx, 0, "time_slice/boundary/phi", IMAS_MVDD_FIDELITY_UNMAPPABLE,
                  IMAS_MVDD_LOSS_OPERATION_WRITE);

    printf("write_delete_conversion_test child-write-refusal-is-retained-on-its-root-with-a-complete-path: "
           "a child refusal reached the root log under its joined DD path\n");
}

static void scenario_delete_nested_child_context_translates_relative_path(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int size = -1;
    int arraystruct_ctx = -1;
    CHECK(al_begin_arraystruct_action(operation_ctx, "time_slice", "", &size, &arraystruct_ctx)
              .code == 0);

    check_delete_lands(arraystruct_ctx, "global_quantities/beta_tor_norm",
                       "global_quantities/beta_normal");
}

static void scenario_delete_refuses_stamp_subtrees_before_core_call(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int deletes_before = int_from_stub("recording_stub_delete_call_count");
    const char *reason =
        "this delete would remove the DD-version stamp while stored data remains";
    const char *paths[] = {"ids_properties/version_put/data_dictionary",
                           "ids_properties/version_put", "ids_properties"};
    for (size_t index = 0; index < sizeof(paths) / sizeof(paths[0]); index++) {
        al_status_t status = al_delete_data(operation_ctx, paths[index]);
        CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
        CHECK_REFUSAL_MESSAGE(status, reason, paths[index], "4.1.1", "3.39.0");
    }
    CHECK(int_from_stub("recording_stub_delete_call_count") == deletes_before);
}

static void scenario_delete_empty_path_forwards_as_explicit_migration_route(void) {
    int operation_ctx = open_mismatched_equilibrium();
    check_delete_lands(operation_ctx, "", "");
    CHECK(loss_count(operation_ctx) == 0);
}

static void scenario_delete_refuses_no_source_unservable_and_structures(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int deletes_before = int_from_stub("recording_stub_delete_call_count");
    al_status_t status = al_delete_data(operation_ctx, "time_slice/boundary/phi");

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status, "this path has no stored source", "time_slice/boundary/phi",
                          "4.1.1", "3.39.0");
    status = al_delete_data(operation_ctx, "grids_ggd/grid/space/coordinates_type");
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status, "this path's container changed shape and cannot be served",
                          "grids_ggd/grid/space/coordinates_type", "4.1.1", "3.39.0");
    status = al_delete_data(operation_ctx, "time_slice/boundary");
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status,
                          "this subtree delete would leave data at a stored path outside the "
                          "requested subtree",
                          "time_slice/boundary", "4.1.1", "3.39.0");
    CHECK(int_from_stub("recording_stub_delete_call_count") == deletes_before);
}

/* Issue #131 / ADR 0017 decision 4: a structure delete is trivial and
 * proceeds when no rule nested underneath it targets a stored path outside
 * the subtree it resolves to. "time_slice" and "time_slice/constraints" are
 * ADR 0017's own examples of subtrees every nested rule stays inside. */
static void scenario_delete_admits_trivial_structure_deletes(void) {
    int operation_ctx = open_mismatched_equilibrium();

    check_delete_lands(operation_ctx, "time_slice", "time_slice");
    check_delete_lands(operation_ctx, "time_slice/constraints", "time_slice/constraints");
    CHECK(loss_count(operation_ctx) == 0);
}

/* ADR 0017 decision 4's "boundary_separatrix" example, under the opposite
 * HLI/stored pairing from scenario_delete_refuses_no_source_unservable_and_
 * structures (run via a CMake registration that sets HLI_DD_VERSION 3.39.0 /
 * STAMP_VERSION 4.1.1). This still refuses under a DD3 HLI, but for a
 * different, pre-existing reason: `drop-boundary-separatrix` (`left_only`)
 * claims this exact path itself, so it resolves to no stored source before
 * the escaping-rule check ever runs — it never reaches the leaf/structure
 * classification at all. The escaping check is what makes "time_slice/
 * boundary" (below) refuse in the *other* direction, where no rule directly
 * claims the whole structure. */
static void scenario_delete_refuses_boundary_separatrix_reverse_direction(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int deletes_before = int_from_stub("recording_stub_delete_call_count");

    al_status_t status = al_delete_data(operation_ctx, "time_slice/boundary_separatrix");
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status, "this path has no stored source",
                          "time_slice/boundary_separatrix", "3.39.0", "4.1.1");
    CHECK(int_from_stub("recording_stub_delete_call_count") == deletes_before);
}

static void scenario_delete_fans_out_over_candidates_in_declared_order(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int reads_before = int_from_stub("recording_stub_read_call_count");
    int deletes_before = int_from_stub("recording_stub_delete_call_count");
    int events_before = int_from_stub("recording_stub_data_event_count");

    enable_probe_allocations();
    CHECK(al_delete_data(operation_ctx, "time_slice/profiles_2d/b_field_phi").code == 0);
    disable_probe_allocations();
    CHECK(int_from_stub("recording_stub_read_call_count") == reads_before + 3);
    CHECK(int_from_stub("recording_stub_delete_call_count") == deletes_before + 3);
    CHECK(strcmp(delete_path_at(deletes_before), "time_slice/profiles_2d/b_field_phi") == 0);
    CHECK(strcmp(delete_path_at(deletes_before + 1), "time_slice/profiles_2d/b_field_tor") ==
          0);
    CHECK(strcmp(delete_path_at(deletes_before + 2), "time_slice/profiles_2d/b_tor") == 0);
    CHECK(int_from_stub("recording_stub_data_event_count") == events_before + 6);
    for (int index = 0; index < 3; ++index) {
        CHECK(data_event_kind_at(events_before + 2 * index) == IMAS_MVDD_STUB_DATA_EVENT_READ);
        CHECK(data_event_kind_at(events_before + 2 * index + 1) == IMAS_MVDD_STUB_DATA_EVENT_DELETE);
        CHECK(strcmp(data_event_path_at(events_before + 2 * index),
                     delete_path_at(deletes_before + index)) == 0);
        CHECK(strcmp(data_event_path_at(events_before + 2 * index + 1),
                     delete_path_at(deletes_before + index)) == 0);
    }
    CHECK(loss_count(operation_ctx) == 0);
}

static void scenario_delete_skips_not_found_candidates(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int reads_before = int_from_stub("recording_stub_read_call_count");
    int deletes_before = int_from_stub("recording_stub_delete_call_count");

    enable_probe_allocations();
    CHECK(setenv("RECORDING_STUB_READ_NOT_FOUND_FIELD", "time_slice/profiles_2d/b_field_tor", 1) ==
          0);
    CHECK(al_delete_data(operation_ctx, "time_slice/profiles_2d/b_field_phi").code == 0);
    unsetenv("RECORDING_STUB_READ_NOT_FOUND_FIELD");
    disable_probe_allocations();

    CHECK(int_from_stub("recording_stub_read_call_count") == reads_before + 3);
    CHECK(int_from_stub("recording_stub_delete_call_count") == deletes_before + 2);
    CHECK(strcmp(delete_path_at(deletes_before), "time_slice/profiles_2d/b_field_phi") == 0);
    CHECK(strcmp(delete_path_at(deletes_before + 1), "time_slice/profiles_2d/b_tor") == 0);
    CHECK(loss_count(operation_ctx) == 0);
}

static void scenario_delete_reports_probe_and_delete_failures_distinctly(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int reads_before = int_from_stub("recording_stub_read_call_count");
    int deletes_before = int_from_stub("recording_stub_delete_call_count");

    enable_probe_allocations();
    CHECK(setenv("RECORDING_STUB_READ_FAIL_FIELD", "time_slice/profiles_2d/b_field_tor", 1) == 0);
    al_status_t probe_failure = al_delete_data(operation_ctx, "time_slice/profiles_2d/b_field_phi");
    unsetenv("RECORDING_STUB_READ_FAIL_FIELD");
    disable_probe_allocations();
    CHECK(probe_failure.code == -23);
    CHECK(strcmp(probe_failure.message,
                 "IMAS-MVDD: probe failed for stored candidate time_slice/profiles_2d/b_field_tor") ==
          0);
    CHECK(int_from_stub("recording_stub_read_call_count") == reads_before + 3);
    CHECK(int_from_stub("recording_stub_delete_call_count") == deletes_before + 2);
    CHECK(strcmp(delete_path_at(deletes_before), "time_slice/profiles_2d/b_field_phi") == 0);
    CHECK(strcmp(delete_path_at(deletes_before + 1), "time_slice/profiles_2d/b_tor") == 0);

    reads_before = int_from_stub("recording_stub_read_call_count");
    deletes_before = int_from_stub("recording_stub_delete_call_count");
    enable_probe_allocations();
    CHECK(setenv("RECORDING_STUB_DELETE_FAIL_FIELD", "time_slice/profiles_2d/b_field_tor", 1) ==
          0);
    al_status_t delete_failure =
        al_delete_data(operation_ctx, "time_slice/profiles_2d/b_field_phi");
    unsetenv("RECORDING_STUB_DELETE_FAIL_FIELD");
    disable_probe_allocations();
    CHECK(delete_failure.code == -24);
    CHECK(strcmp(delete_failure.message,
                 "IMAS-MVDD: delete failed for stored candidate time_slice/profiles_2d/b_field_tor") ==
          0);
    CHECK(int_from_stub("recording_stub_read_call_count") == reads_before + 3);
    CHECK(int_from_stub("recording_stub_delete_call_count") == deletes_before + 3);
    CHECK(strcmp(delete_path_at(deletes_before), "time_slice/profiles_2d/b_field_phi") == 0);
    CHECK(strcmp(delete_path_at(deletes_before + 1), "time_slice/profiles_2d/b_field_tor") ==
          0);
    CHECK(strcmp(delete_path_at(deletes_before + 2), "time_slice/profiles_2d/b_tor") == 0);
    CHECK(loss_count(operation_ctx) == 0);
}

static void scenario_delete_probes_enter_the_read_reentry_guard(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int reentrant_before = int_from_stub("recording_stub_reentrant_call_count");

    arm_reentrant_read(al_read_data, "time_slice/profiles_2d/b_field_tor");
    enable_probe_allocations();
    CHECK(al_delete_data(operation_ctx, "time_slice/profiles_2d/b_field_phi").code == 0);
    disable_probe_allocations();

    CHECK(int_from_stub("recording_stub_reentrant_call_count") == reentrant_before + 3);
    CHECK(strcmp(string_from_stub("recording_stub_reentrant_seen_field"),
                 "time_slice/profiles_2d/b_field_tor") == 0);
    CHECK(loss_count(operation_ctx) == 0);
}

static void scenario_delete_refuses_non_primary_source_before_core_call(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int deletes_before = int_from_stub("recording_stub_delete_call_count");
    al_status_t status = al_delete_data(operation_ctx, "time_slice/profiles_2d/b_tor");

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status,
                          "this path is a non-primary source and cannot delete a shared stored slot",
                          "time_slice/profiles_2d/b_tor", "3.39.0", "4.1.1");
    CHECK(int_from_stub("recording_stub_delete_call_count") == deletes_before);
}

static void scenario_write_refuses_non_primary_source_before_core_call(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int writes_before = int_from_stub("recording_stub_write_call_count");
    double sentinel = 42.0;
    int size[1] = {1};
    al_status_t status =
        write_field(operation_ctx, "time_slice/profiles_2d/b_tor", "time", &sentinel, size);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status, "this path is a non-primary source and cannot write a shared stored slot",
                          "time_slice/profiles_2d/b_tor", "3.39.0", "4.1.1");
    CHECK(int_from_stub("recording_stub_write_call_count") == writes_before);
    CHECK(sentinel == 42.0);
    CHECK(size[0] == 1);
}

static void scenario_write_matching_context_forwards_unchanged(void) {
    int operation_ctx = open_mismatched_equilibrium();
    double sentinel = 7.5;
    int size[1] = {1};

    CHECK(write_field(operation_ctx, "time_slice/global_quantities/beta_tor_norm", "time",
                       &sentinel, size)
              .code == 0);

    CHECK(int_from_stub("recording_stub_write_ctx_id") == operation_ctx);
    CHECK(strcmp(string_from_stub("recording_stub_write_field"),
                 "time_slice/global_quantities/beta_tor_norm") == 0);
    CHECK(strcmp(string_from_stub("recording_stub_write_timebase"), "time") == 0);
    CHECK(pointer_from_stub("recording_stub_write_data") == &sentinel);

    printf("write_delete_conversion_test write-matching-context-forwards-unchanged: a matching "
           "stamp was forwarded verbatim\n");
}

static void scenario_write_unknown_context_forwards_unchanged(void) {
    int pulse_ctx = -1;
    CHECK(al_begin_dataentry_action("imas:hdf5?path=/tmp/pulse", 7, &pulse_ctx).code == 0);
    int operation_ctx = -1;
    CHECK(al_begin_global_action(pulse_ctx, "core_profiles", "", 30, &operation_ctx).code == 0);

    double sentinel = 1.0;
    int size[1] = {1};
    CHECK(write_field(operation_ctx, "profiles_1d/electrons/density", "time", &sentinel, size)
              .code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_write_field"), "profiles_1d/electrons/density") ==
          0);

    printf("write_delete_conversion_test write-unknown-context-forwards-unchanged: an unavailable "
           "artifact was forwarded\n");
}

static void scenario_write_unstamped_context_forwards_unchanged(void) {
    int operation_ctx = open_mismatched_equilibrium();
    double sentinel = 1.0;
    int size[1] = {1};
    CHECK(write_field(operation_ctx, "time_slice/global_quantities/beta_tor_norm", "time",
                       &sentinel, size)
              .code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_write_field"),
                 "time_slice/global_quantities/beta_tor_norm") == 0);

    printf("write_delete_conversion_test write-unstamped-context-forwards-unchanged: an unstamped "
           "occurrence was forwarded\n");
}

static void scenario_write_conversion_disabled_forwards_unchanged(void) {
    int operation_ctx = open_mismatched_equilibrium();
    double sentinel = 1.0;
    int size[1] = {1};
    CHECK(write_field(operation_ctx, "time_slice/global_quantities/beta_tor_norm", "time",
                       &sentinel, size)
              .code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_write_field"),
                 "time_slice/global_quantities/beta_tor_norm") == 0);

    printf("write_delete_conversion_test write-conversion-disabled-forwards-unchanged: an unset "
           "HLI version was forwarded\n");
}

static void scenario_delete_matching_context_forwards_unchanged(void) {
    int operation_ctx = open_mismatched_equilibrium();

    CHECK(al_delete_data(operation_ctx, "time_slice/global_quantities/beta_tor_norm").code == 0);

    CHECK(int_from_stub("recording_stub_delete_ctx") == operation_ctx);
    CHECK(strcmp(string_from_stub("recording_stub_delete_path"),
                 "time_slice/global_quantities/beta_tor_norm") == 0);

    printf("write_delete_conversion_test delete-matching-context-forwards-unchanged: a matching "
           "stamp was forwarded verbatim\n");
}

static void scenario_delete_unknown_context_forwards_unchanged(void) {
    int pulse_ctx = -1;
    CHECK(al_begin_dataentry_action("imas:hdf5?path=/tmp/pulse", 7, &pulse_ctx).code == 0);
    int operation_ctx = -1;
    CHECK(al_begin_global_action(pulse_ctx, "core_profiles", "", 30, &operation_ctx).code == 0);

    CHECK(al_delete_data(operation_ctx, "profiles_1d/electrons/density").code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_delete_path"), "profiles_1d/electrons/density") ==
          0);

    printf("write_delete_conversion_test delete-unknown-context-forwards-unchanged: an unavailable "
           "artifact was forwarded\n");
}

static void scenario_delete_unstamped_context_forwards_unchanged(void) {
    int operation_ctx = open_mismatched_equilibrium();

    CHECK(al_delete_data(operation_ctx, "time_slice/global_quantities/beta_tor_norm").code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_delete_path"),
                 "time_slice/global_quantities/beta_tor_norm") == 0);

    printf("write_delete_conversion_test delete-unstamped-context-forwards-unchanged: an unstamped "
           "occurrence was forwarded\n");
}

static void scenario_delete_conversion_disabled_forwards_unchanged(void) {
    int operation_ctx = open_mismatched_equilibrium();

    CHECK(al_delete_data(operation_ctx, "time_slice/global_quantities/beta_tor_norm").code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_delete_path"),
                 "time_slice/global_quantities/beta_tor_norm") == 0);

    printf("write_delete_conversion_test delete-conversion-disabled-forwards-unchanged: an unset "
           "HLI version was forwarded\n");
}

int main(int argc, char **argv) {
    static const shim_test_scenario scenarios[] = {
        {"write-renamed-field-lands-at-stored-spelling", scenario_write_renamed_field_lands_at_stored_spelling},
        {"write-identity-and-moved-fields-land-at-stored-spelling", scenario_write_identity_and_moved_fields_land_at_stored_spelling},
        {"write-reverse-identity-renamed-and-moved-fields-land-at-stored-spelling", scenario_write_reverse_identity_renamed_and_moved_fields_land_at_stored_spelling},
        {"delete-identity-renamed-and-moved-fields-land-at-stored-spelling", scenario_delete_identity_renamed_and_moved_fields_land_at_stored_spelling},
        {"delete-reverse-identity-renamed-and-moved-fields-land-at-stored-spelling", scenario_delete_reverse_identity_renamed_and_moved_fields_land_at_stored_spelling},
        {"plugin-write-renamed-field-lands-at-stored-spelling", scenario_plugin_write_renamed_field_lands_at_stored_spelling},
        {"plugin-write-matching-context-forwards-unchanged", scenario_plugin_write_matching_context_forwards_unchanged},
        {"write-nested-child-context-resolves-relative-and-absolute-fields", scenario_write_nested_child_context_resolves_relative_and_absolute_fields},
        {"write-candidate-lands-at-primary-and-retains-unwritten-candidates", scenario_write_candidate_lands_at_primary_and_retains_unwritten_candidates},
        {"write-non-primary-source-refuses-by-precedence", scenario_write_non_primary_source_refuses_by_precedence},
        {"write-split-candidate-lands-at-primary", scenario_write_split_candidate_lands_at_primary},
        {"child-write-candidate-retains-complete-path-at-root", scenario_child_write_candidate_retains_complete_path_at_root},
        {"write-uses-the-primary-candidate-without-fanout", scenario_write_uses_the_primary_candidate_without_fanout},
        {"write-cocos-sign-flip-uses-a-shim-owned-rank-seven-copy", scenario_write_cocos_sign_flip_uses_a_shim_owned_rank_seven_copy},
        {"plugin-write-cocos-sign-flip-uses-a-shim-owned-copy", scenario_plugin_write_cocos_sign_flip_uses_a_shim_owned_copy},
        {"write-cocos-sentinel-forwards-unchanged-without-loss", scenario_write_cocos_sentinel_forwards_unchanged_without_loss},
        {"write-cocos-invalid-shape-or-type-refuses-before-core", scenario_write_cocos_invalid_shape_or_type_refuses_before_core},
        {"write-refuses-dd-version-stamp-but-forwards-its-siblings", scenario_write_refuses_dd_version_stamp_but_forwards_its_siblings},
        {"write-without-stored-slot-refuses-and-retains-a-write-loss", scenario_write_without_stored_slot_refuses_and_retains_a_write_loss},
        {"write-reverse-without-stored-slot-refuses-and-retains-a-write-loss", scenario_write_reverse_without_stored_slot_refuses_and_retains_a_write_loss},
        {"write-retyped-path-refuses-and-retains-a-write-loss", scenario_write_retyped_path_refuses_and_retains_a_write_loss},
        {"child-write-refusal-is-retained-on-its-root-with-a-complete-path", scenario_child_write_refusal_is_retained_on_its_root_with_a_complete_path},
        {"delete-nested-child-context-translates-relative-path", scenario_delete_nested_child_context_translates_relative_path},
        {"delete-refuses-stamp-subtrees-before-core-call", scenario_delete_refuses_stamp_subtrees_before_core_call},
        {"delete-empty-path-forwards-as-explicit-migration-route", scenario_delete_empty_path_forwards_as_explicit_migration_route},
        {"delete-refuses-no-source-unservable-and-structures", scenario_delete_refuses_no_source_unservable_and_structures},
        {"delete-admits-trivial-structure-deletes", scenario_delete_admits_trivial_structure_deletes},
        {"delete-refuses-boundary-separatrix-reverse-direction", scenario_delete_refuses_boundary_separatrix_reverse_direction},
        {"delete-fans-out-over-candidates-in-declared-order", scenario_delete_fans_out_over_candidates_in_declared_order},
        {"delete-skips-not-found-candidates", scenario_delete_skips_not_found_candidates},
        {"delete-reports-probe-and-delete-failures-distinctly", scenario_delete_reports_probe_and_delete_failures_distinctly},
        {"delete-probes-enter-the-read-reentry-guard", scenario_delete_probes_enter_the_read_reentry_guard},
        {"delete-refuses-non-primary-source-before-core-call", scenario_delete_refuses_non_primary_source_before_core_call},
        {"write-refuses-non-primary-source-before-core-call", scenario_write_refuses_non_primary_source_before_core_call},
        {"write-matching-context-forwards-unchanged", scenario_write_matching_context_forwards_unchanged},
        {"write-unknown-context-forwards-unchanged", scenario_write_unknown_context_forwards_unchanged},
        {"write-unstamped-context-forwards-unchanged", scenario_write_unstamped_context_forwards_unchanged},
        {"write-conversion-disabled-forwards-unchanged", scenario_write_conversion_disabled_forwards_unchanged},
        {"delete-matching-context-forwards-unchanged", scenario_delete_matching_context_forwards_unchanged},
        {"delete-unknown-context-forwards-unchanged", scenario_delete_unknown_context_forwards_unchanged},
        {"delete-unstamped-context-forwards-unchanged", scenario_delete_unstamped_context_forwards_unchanged},
        {"delete-conversion-disabled-forwards-unchanged", scenario_delete_conversion_disabled_forwards_unchanged},
    };
    return RUN_NAMED_SCENARIO(argc, argv, scenarios);
}
