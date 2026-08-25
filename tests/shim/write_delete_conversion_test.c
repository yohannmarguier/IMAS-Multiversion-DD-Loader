/* Public al_write_data / al_delete_data and al_plugin_write_data ABI scenarios
 * against the recording stub. */

#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifndef RECORDING_STUB_PATH
#error "RECORDING_STUB_PATH must be defined by CMakeLists.txt"
#endif

#include "../support/shim_test_support.h"

static al_status_t write_field(int ctx_id, const char *field, const char *timebase, void *data,
                               int *size) {
    return al_write_data(ctx_id, field, timebase, data, 52 /* DOUBLE_DATA */, 1, size);
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

static void scenario_delete_refuses_under_known_mismatched_root_before_core_call(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int deletes_before = int_from_stub("recording_stub_delete_call_count");

    al_status_t status =
        al_delete_data(operation_ctx, "time_slice/global_quantities/beta_tor_norm");

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status, "al_delete_data refuses on a context with a known DD version mismatch",
                          "time_slice/global_quantities/beta_tor_norm", "4.1.1", "3.39.0");
    CHECK(int_from_stub("recording_stub_delete_call_count") == deletes_before);

    printf("write_delete_conversion_test delete-refuses-under-known-mismatched-root-before-core-call: "
           "IMAS-Core was never called and the refusal named the path and both DD versions\n");
}

static void scenario_plugin_write_renamed_field_lands_at_stored_spelling(void) {
    int operation_ctx = open_mismatched_equilibrium();
    double sentinel = 42.0;
    int size[1] = {73};
    al_status_t status = al_plugin_write_data(
        operation_ctx, "time_slice/global_quantities/beta_tor_norm", "time", &sentinel,
        52 /* DOUBLE_DATA */, 1, size);

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
        52 /* DOUBLE_DATA */, 1, size);

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

static void scenario_write_refuses_candidate_or_transformation_on_either_argument(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int writes_before = int_from_stub("recording_stub_write_call_count");
    double sentinel = 42.0;
    int size[1] = {73};

    CHECK(write_field(operation_ctx, "time_slice/profiles_2d/b_field_phi", "time", &sentinel,
                      size)
              .code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(write_field(operation_ctx, "time_slice/boundary/elongation",
                      "time_slice/profiles_2d/b_field_phi", &sentinel, size)
              .code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(write_field(operation_ctx, "time_slice/constraints/flux_loop/measured", "time", &sentinel,
                      size)
              .code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(int_from_stub("recording_stub_write_call_count") == writes_before);
    CHECK(sentinel == 42.0);
    CHECK(size[0] == 73);

    printf("write_delete_conversion_test write-refuses-candidate-or-transformation-on-either-argument: "
           "each deferred resolution refused before Core\n");
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

static void scenario_delete_nested_child_context_refuses_through_mismatched_root(void) {
    int operation_ctx = open_mismatched_equilibrium();
    int size = -1;
    int arraystruct_ctx = -1;
    CHECK(al_begin_arraystruct_action(operation_ctx, "time_slice", "", &size, &arraystruct_ctx)
              .code == 0);

    int deletes_before = int_from_stub("recording_stub_delete_call_count");
    al_status_t status = al_delete_data(arraystruct_ctx, "global_quantities/beta_tor_norm");

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(int_from_stub("recording_stub_delete_call_count") == deletes_before);

    printf("write_delete_conversion_test delete-nested-child-context-refuses-through-mismatched-root: "
           "a child arraystruct context inherited its root's refusal\n");
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
        {"delete-refuses-under-known-mismatched-root-before-core-call", scenario_delete_refuses_under_known_mismatched_root_before_core_call},
        {"plugin-write-renamed-field-lands-at-stored-spelling", scenario_plugin_write_renamed_field_lands_at_stored_spelling},
        {"plugin-write-matching-context-forwards-unchanged", scenario_plugin_write_matching_context_forwards_unchanged},
        {"write-nested-child-context-resolves-relative-and-absolute-fields", scenario_write_nested_child_context_resolves_relative_and_absolute_fields},
        {"write-refuses-candidate-or-transformation-on-either-argument", scenario_write_refuses_candidate_or_transformation_on_either_argument},
        {"write-refuses-dd-version-stamp-but-forwards-its-siblings", scenario_write_refuses_dd_version_stamp_but_forwards_its_siblings},
        {"delete-nested-child-context-refuses-through-mismatched-root", scenario_delete_nested_child_context_refuses_through_mismatched_root},
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
