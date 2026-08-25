/* Issues #69 and #134: conversion is scoped, and the scope has an outside edge.
 *
 * #69 asserted that edge while a *read* converts; #134 asserts the same edge
 * while a *write* does, once the write path became a translating seam of its
 * own (ADR 0016) rather than a blanket refusal. The two halves live in one file
 * because they are one claim about one seam list; the write half starts at the
 * "Issue #134" banner below.
 *
 * Every other C-ABI test in this suite asserts what the conversion seams
 * *do*. This one asserts what the rest of the ABI must keep *not* doing while
 * conversion is fully active: an equilibrium occurrence whose stored DD
 * version (3.39.0) differs from the HLI's (4.1.1), with an embedded artifact
 * to serve it, is open and registered for the whole of every scenario below.
 * That is the state in which a path-rewriting mistake would leak outside its
 * declared seam list, so it is the state these passthrough assertions are
 * made in.
 *
 * `al_get_occurrences` (its `ids_name`), `al_list_filled_paths` (its
 * `dataobjectname` on the way down and its returned `path_list` on the way
 * back up) and `al_bind_plugin` / `al_unbind_plugin` (their `fieldPath`) are
 * the three deliberate exclusions: CLAUDE.md lists all four as
 * conversion-relevant, and this project has decided not to translate them
 * yet. Their arguments here are therefore chosen to be paths the loaded
 * artifact does have rules for — `time_slice/global_quantities/beta_normal`
 * ⇄ `beta_tor_norm` (`rename-beta-normal`) — so that a shim which started
 * rewriting them would produce a visibly different string rather than pass
 * this test by coincidence. The remaining non-seam exports are asserted to
 * reach IMAS-Core with their arguments and results untouched.
 *
 * The recording stub is only the external IMAS-Core substitute; every call
 * enters the shim through its public C ABI. The HLI DD version latch and the
 * context registry are both process-wide, so each scenario is its own ctest
 * process, exactly like read_path_test.c's scenarios. */

#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifndef RECORDING_STUB_PATH
#error "RECORDING_STUB_PATH must be defined by CMakeLists.txt"
#endif
#ifndef EXPECTED_AL_VERSION
#error "EXPECTED_AL_VERSION must be defined by CMakeLists.txt"
#endif

#include "../support/shim_test_support.h"

/* The two spellings of the one path `rename-beta-normal` relates. Reaching
 * IMAS-Core, or coming back from it, as the *other* one of this pair is
 * exactly the failure these scenarios exist to catch. */
#define HLI_SPELLING "time_slice/global_quantities/beta_tor_norm"
#define STORED_SPELLING "time_slice/global_quantities/beta_normal"

/* Opens the mismatched occurrence every scenario here needs and then proves,
 * before the scenario's own assertions run, that the resulting record really
 * is converting: the same rule whose two spellings those assertions use
 * translates an ordinary read. Without that proof a passthrough assertion
 * could pass simply because the mismatch was never registered.
 *
 * This is deliberately a local extension of the shared
 * `open_mismatched_occurrence` rather than a widening of it — the proving read
 * moves the stub's shared call recorders, which the suites that count them
 * would see. Returns the pulse context, since the two enumeration seams below
 * are addressed by it. */
static int open_converting_equilibrium(int *operation_ctx) {
    int pulse_ctx = -1;
    *operation_ctx = open_mismatched_occurrence("equilibrium", &pulse_ctx);

    CHECK(int_from_stub("recording_stub_dataentry_call_count") == 1);
    CHECK(strcmp(string_from_stub("recording_stub_dataentry_uri"),
                 "imas:hdf5?path=/tmp/pulse")
          == 0);
    CHECK(int_from_stub("recording_stub_dataentry_mode") == 7);

    void *data = NULL;
    int size[1] = {0};
    CHECK(al_read_data(*operation_ctx, HLI_SPELLING, "", &data, IMAS_DOUBLE_DATA, 1, size).code
          == 0);
    CHECK(strcmp(string_from_stub("recording_stub_read_field"), STORED_SPELLING) == 0);

    return pulse_ctx;
}

static void scenario_get_occurrences_forwards_ids_name_unchanged(void) {
    int operation_ctx = -1;
    int pulse_ctx = open_converting_equilibrium(&operation_ctx);

    int *occurrences = NULL;
    int size = -1;
    al_status_t status = al_get_occurrences(pulse_ctx, "equilibrium", &occurrences, &size);

    CHECK(status.code == 0);
    CHECK(int_from_stub("recording_stub_occurrences_call_count") == 1);
    CHECK(int_from_stub("recording_stub_occurrences_pctx_id") == pulse_ctx);
    /* IDS names are stable across this version pair, so "forwarded unchanged"
     * and "correctly translated" agree here — the point is that the shim does
     * not invent a difference. */
    CHECK(strcmp(string_from_stub("recording_stub_occurrences_ids_name"), "equilibrium") == 0);
    CHECK(size == 3);
    CHECK(occurrences != NULL);
    CHECK(occurrences[0] == 11 && occurrences[1] == 22 && occurrences[2] == 33);

    printf("scoped_passthrough_test get-occurrences-forwards-ids-name-unchanged: a live "
           "mismatched occurrence did not change how the IDS name reached IMAS-Core\n");
}

static void scenario_list_filled_paths_forwards_name_and_returns_stored_paths_unchanged(void) {
    int operation_ctx = -1;
    int pulse_ctx = open_converting_equilibrium(&operation_ctx);

    char **paths = NULL;
    int size = -1;
    al_status_t status = al_list_filled_paths(pulse_ctx, "equilibrium", &paths, &size);

    CHECK(status.code == 0);
    CHECK(int_from_stub("recording_stub_filled_paths_call_count") == 1);
    CHECK(int_from_stub("recording_stub_filled_paths_pctx_id") == pulse_ctx);
    CHECK(strcmp(string_from_stub("recording_stub_filled_paths_dataobjectname"), "equilibrium")
          == 0);

    /* CMakeLists.txt seeds RECORDING_STUB_FILLED_PATHS_CSV with the stored
     * 3.39.0 spelling. Up-converting it would hand back the 4.1.1 spelling;
     * this project has not implemented that, and the returned list must
     * therefore still read as IMAS-Core wrote it. */
    CHECK(size == 2);
    CHECK(paths != NULL);
    CHECK(strcmp(paths[0], STORED_SPELLING) == 0);
    CHECK(strcmp(paths[1], "time_slice/global_quantities/ip") == 0);

    /* The caller owns the list and every string in it, whether or not the
     * shim rewrote them (CLAUDE.md's up-conversion ownership note). */
    for (int i = 0; i < size; ++i) {
        free(paths[i]);
    }
    free(paths);

    printf("scoped_passthrough_test list-filled-paths-forwards-name-and-returns-stored-paths-"
           "unchanged: neither the IDS name down nor the path list up was rewritten\n");
}

static void scenario_bind_and_unbind_plugin_forward_field_path_unchanged(void) {
    int operation_ctx = -1;
    (void)open_converting_equilibrium(&operation_ctx);

    int calls_before = int_from_stub("recording_stub_plugin_call_count");

    CHECK(al_bind_plugin(HLI_SPELLING, "recording-plugin").code == 0);
    CHECK(int_from_stub("recording_stub_plugin_call_count") == calls_before + 1);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"), "al_bind_plugin") == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_first_string"), HLI_SPELLING) == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_second_string"), "recording-plugin") == 0);

    CHECK(al_unbind_plugin(HLI_SPELLING, "recording-plugin").code == 0);
    CHECK(int_from_stub("recording_stub_plugin_call_count") == calls_before + 2);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"), "al_unbind_plugin") == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_first_string"), HLI_SPELLING) == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_second_string"), "recording-plugin") == 0);

    printf("scoped_passthrough_test bind-and-unbind-plugin-forward-field-path-unchanged: the "
           "HLI's own DD spelling reached IMAS-Core untranslated\n");
}

/* The plugin registration, configuration and readback family carries no DD
 * path at all, so its contract under an active conversion is the plain
 * identity forward it has always had. */
static void check_plugin_management_forwards_unchanged(int operation_ctx) {
    int calls = int_from_stub("recording_stub_plugin_call_count");

    CHECK(al_register_plugin("recording-plugin").code == 0);
    CHECK(int_from_stub("recording_stub_plugin_call_count") == ++calls);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"), "al_register_plugin") == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_first_string"), "recording-plugin") == 0);

    _Bool registered = 0;
    CHECK(al_is_plugin_registered("recording-plugin", &registered).code == 0);
    CHECK(registered);
    CHECK(int_from_stub("recording_stub_plugin_call_count") == ++calls);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"), "al_is_plugin_registered")
          == 0);

    CHECK(al_bind_readback_plugins(operation_ctx).code == 0);
    CHECK(int_from_stub("recording_stub_plugin_call_count") == ++calls);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"), "al_bind_readback_plugins")
          == 0);
    CHECK(int_from_stub("recording_stub_plugin_last_ctx") == operation_ctx);

    CHECK(al_unbind_readback_plugins(operation_ctx).code == 0);
    CHECK(int_from_stub("recording_stub_plugin_call_count") == ++calls);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"),
                 "al_unbind_readback_plugins")
          == 0);
    CHECK(int_from_stub("recording_stub_plugin_last_ctx") == operation_ctx);

    CHECK(al_write_plugins_metadata(operation_ctx).code == 0);
    CHECK(int_from_stub("recording_stub_plugin_call_count") == ++calls);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"),
                 "al_write_plugins_metadata")
          == 0);
    CHECK(int_from_stub("recording_stub_plugin_last_ctx") == operation_ctx);

    CHECK(al_unregister_plugin("recording-plugin").code == 0);
    CHECK(int_from_stub("recording_stub_plugin_call_count") == ++calls);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"), "al_unregister_plugin")
          == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_first_string"), "recording-plugin") == 0);
}

/* The three parameter setters take a *parameter* name, never a DD path. Their
 * buffer arguments are forwarded by pointer, so pointer identity is the
 * assertion: the shim must not copy or substitute caller storage here the way
 * a value transformation does on a read. */
static void check_parameter_setters_forward_unchanged(void) {
    int calls = int_from_stub("recording_stub_plugin_call_count");

    int payload = 7;
    int extent[1] = {1};
    CHECK(al_setvalue_parameter_plugin("generic", IMAS_INTEGER_DATA, 1, extent, &payload,
                                       "recording-plugin")
              .code
          == 0);
    CHECK(int_from_stub("recording_stub_plugin_call_count") == ++calls);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"),
                 "al_setvalue_parameter_plugin")
          == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_first_string"), "generic") == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_second_string"), "recording-plugin") == 0);
    CHECK(int_from_stub("recording_stub_plugin_first_int") == IMAS_INTEGER_DATA);
    CHECK(int_from_stub("recording_stub_plugin_second_int") == 1);
    CHECK(pointer_from_stub("recording_stub_plugin_pointer") == &payload);
    CHECK(pointer_from_stub("recording_stub_plugin_size_pointer") == extent);

    CHECK(al_setvalue_int_scalar_parameter_plugin("integer", 42, "recording-plugin").code == 0);
    CHECK(int_from_stub("recording_stub_plugin_call_count") == ++calls);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"),
                 "al_setvalue_int_scalar_parameter_plugin")
          == 0);
    CHECK(int_from_stub("recording_stub_plugin_first_int") == 42);

    CHECK(al_setvalue_double_scalar_parameter_plugin("double", 1.5, "recording-plugin").code == 0);
    CHECK(int_from_stub("recording_stub_plugin_call_count") == ++calls);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"),
                 "al_setvalue_double_scalar_parameter_plugin")
          == 0);
    CHECK(double_from_stub("recording_stub_plugin_double") == 1.5);
}

/* The utility and version accessors answer about IMAS-Core itself, not about
 * stored data. In particular getDDVersion() must keep returning IMAS-Core's
 * deliberate sentinel: the shim knows this occurrence's stored DD version and
 * still must not answer with it (CLAUDE.md, ADR 0005). */
static void check_utility_and_version_accessors_forward_unchanged(int operation_ctx) {
    int calls = int_from_stub("recording_stub_utility_call_count");

    int backend_id = -1;
    CHECK(al_get_backendID(operation_ctx, &backend_id).code == 0);
    CHECK(int_from_stub("recording_stub_utility_call_count") == ++calls);
    CHECK(strcmp(string_from_stub("recording_stub_utility_last_symbol"), "al_get_backendID") == 0);
    CHECK(int_from_stub("recording_stub_utility_backend_ctx") == operation_ctx);
    CHECK(pointer_from_stub("recording_stub_utility_backend_output") == &backend_id);
    CHECK(backend_id == 9001);

    char *uri = NULL;
    CHECK(al_build_uri_from_legacy_parameters(13, 44, 5, "mvdd-user", "iter", "3", "", &uri).code
          == 0);
    CHECK(int_from_stub("recording_stub_utility_call_count") == ++calls);
    CHECK(strcmp(string_from_stub("recording_stub_utility_last_symbol"),
                 "al_build_uri_from_legacy_parameters")
          == 0);
    CHECK(int_from_stub("recording_stub_utility_builder_backend") == 13);
    CHECK(int_from_stub("recording_stub_utility_builder_pulse") == 44);
    CHECK(int_from_stub("recording_stub_utility_builder_run") == 5);
    CHECK(uri != NULL);
    CHECK(strcmp(uri, "imas:recording?utility=legacy") == 0);
    free(uri);

    CHECK(strcmp(const2str(13), "recording-constant") == 0);
    CHECK(int_from_stub("recording_stub_utility_call_count") == ++calls);
    CHECK(int_from_stub("recording_stub_utility_last_int") == 13);

    CHECK(strcmp(err2str(-1), "recording-error") == 0);
    CHECK(int_from_stub("recording_stub_utility_call_count") == ++calls);
    CHECK(int_from_stub("recording_stub_utility_last_int") == -1);

    CHECK(strcmp(getDDVersion(), "!!DEPRECATED!!") == 0);
    CHECK(int_from_stub("recording_stub_utility_call_count") == ++calls);

    /* getALVersion() answers from the shim's own bootstrap check rather than a
     * fresh forward, so there is no new stub call to count — but its value must
     * still be IMAS-Core's, which for the recording stub is the pinned release
     * in IMAS_CORE_VERSION. Asserting only non-null would pin nothing. */
    CHECK(strcmp(getALVersion(), EXPECTED_AL_VERSION) == 0);

    char *info = NULL;
    CHECK(al_context_info(operation_ctx, &info).code == 0);
    CHECK(info != NULL);
    CHECK(strcmp(info, "recording-stub: context info") == 0);
    CHECK(int_from_stub("recording_stub_last_ctx") == operation_ctx);
    free(info);
}

static void scenario_remaining_non_seam_exports_forward_unchanged(void) {
    int operation_ctx = -1;
    int pulse_ctx = open_converting_equilibrium(&operation_ctx);

    check_plugin_management_forwards_unchanged(operation_ctx);
    check_parameter_setters_forward_unchanged();
    check_utility_and_version_accessors_forward_unchanged(operation_ctx);

    /* Exercise the remaining lifecycle exports while the root conversion
     * record is still live. The child inherits that record, so iteration and
     * both successful closes are also checked for identity forwarding rather
     * than only being covered by setup/teardown in other scenarios. */
    int aos_size = -1;
    int aos_ctx = -1;
    CHECK(al_begin_arraystruct_action(operation_ctx, "time_slice", "", &aos_size, &aos_ctx)
              .code
          == 0);
    CHECK(aos_size == 3003);

    CHECK(al_iterate_over_arraystruct(aos_ctx, 1).code == 0);
    CHECK(int_from_stub("recording_stub_iterate_call_count") == 1);
    CHECK(int_from_stub("recording_stub_iterate_aosctx") == aos_ctx);
    CHECK(int_from_stub("recording_stub_iterate_step") == 1);

    CHECK(al_end_action(aos_ctx).code == 0);
    CHECK(int_from_stub("recording_stub_end_action_call_count") == 1);
    CHECK(int_from_stub("recording_stub_end_action_ctx_id") == aos_ctx);

    CHECK(al_end_action(operation_ctx).code == 0);
    CHECK(int_from_stub("recording_stub_end_action_call_count") == 2);
    CHECK(int_from_stub("recording_stub_end_action_ctx_id") == operation_ctx);

    CHECK(al_close_pulse(pulse_ctx, 42).code == 0);
    CHECK(int_from_stub("recording_stub_close_pulse_call_count") == 1);
    CHECK(int_from_stub("recording_stub_close_pulse_ctx") == pulse_ctx);
    CHECK(int_from_stub("recording_stub_close_pulse_mode") == 42);

    printf("scoped_passthrough_test remaining-non-seam-exports-forward-unchanged: every export "
           "outside the declared seam list kept its identity contract under an active "
           "conversion\n");
}

/* --- Issue #134: the same edge, made while a *write* converts ------------- */

/* Everything above is asserted while a *read* is demonstrably converting.
 * That is only half the claim: ADR 0002 leaves `al_get_occurrences`,
 * `al_list_filled_paths` and the plugin bind/unbind family untranslated
 * whichever direction data is moving, so the passthrough edge must also hold
 * while the write path — a separate policy, ADR 0016, reached through separate
 * code — is actively rewriting spellings. A shim that grew a path rewrite on
 * one of these three seams could have grown it on the write side alone, and
 * nothing above would have noticed.
 *
 * WRITE_OP (31, al_const.h) rather than the READ_OP the helper above uses,
 * because that is the mode a writing caller actually opens with and, since
 * ADR 0020, the mode that makes stored-version discovery run through a
 * shim-owned probe context. Proving the passthrough edge under the mode a
 * writer really uses is stronger than proving it under a write issued through
 * a read-mode context, which `write_delete_conversion_test.c` already covers.
 *
 * The remaining non-seam exports are deliberately not re-run under a write:
 * the plugin registration/metadata family, the three parameter setters and the
 * utility/version accessors carry no DD path at all, so there is no spelling
 * for a write-side rewrite to reach. The three seams below are the ones that
 * do carry one. */
static int open_writing_equilibrium(int *operation_ctx) {
    int pulse_ctx = -1;
    CHECK(al_begin_dataentry_action("imas:hdf5?path=/tmp/pulse", 7, &pulse_ctx).code == 0);
    CHECK(al_begin_global_action(pulse_ctx, "equilibrium", "", 31, operation_ctx).code == 0);

    /* The same three the read-side helper above asserts, so the two twins can
     * be compared line for line rather than leaving a reader to wonder which
     * differences are deliberate. */
    CHECK(int_from_stub("recording_stub_dataentry_call_count") == 1);
    CHECK(strcmp(string_from_stub("recording_stub_dataentry_uri"),
                 "imas:hdf5?path=/tmp/pulse")
          == 0);
    CHECK(int_from_stub("recording_stub_dataentry_mode") == 7);

    /* One assertion the read-side twin has no need for: the mode IMAS-Core saw
     * on the caller's own open must be the caller's, not the probe's. ADR 0020
     * has the shim open a READ_OP global action of its own before this one, so
     * a 30 here would mean the two got crossed. It reads 31 because the probe
     * goes through the `al_plugin_*` family and lands in a different recorder
     * (ADR 0020 decision 3). */
    CHECK(int_from_stub("recording_stub_global_rwmode") == 31);

    /* The proving write: the same `rename-beta-normal` rule whose two
     * spellings every assertion below uses must reach IMAS-Core as the stored
     * one. Without this a passthrough assertion could pass simply because the
     * write-mode open never registered a mismatch at all — exactly the state
     * issue #136 fixed, and exactly what would silently make every scenario
     * here vacuous if it regressed. */
    double sentinel = 42.0;
    int size[1] = {1};
    CHECK(al_write_data(*operation_ctx, HLI_SPELLING, "time", &sentinel, IMAS_DOUBLE_DATA, 1, size)
              .code
          == 0);
    CHECK(int_from_stub("recording_stub_write_call_count") == 1);
    CHECK(strcmp(string_from_stub("recording_stub_write_field"), STORED_SPELLING) == 0);

    return pulse_ctx;
}

static void scenario_writing_get_occurrences_forwards_ids_name_unchanged(void) {
    int operation_ctx = -1;
    int pulse_ctx = open_writing_equilibrium(&operation_ctx);

    int *occurrences = NULL;
    int size = -1;
    al_status_t status = al_get_occurrences(pulse_ctx, "equilibrium", &occurrences, &size);

    CHECK(status.code == 0);
    CHECK(int_from_stub("recording_stub_occurrences_call_count") == 1);
    CHECK(int_from_stub("recording_stub_occurrences_pctx_id") == pulse_ctx);
    CHECK(strcmp(string_from_stub("recording_stub_occurrences_ids_name"), "equilibrium") == 0);
    CHECK(size == 3);
    CHECK(occurrences != NULL);
    CHECK(occurrences[0] == 11 && occurrences[1] == 22 && occurrences[2] == 33);

    printf("scoped_passthrough_test writing-get-occurrences-forwards-ids-name-unchanged: an "
           "actively converting write did not change how the IDS name reached IMAS-Core\n");
}

static void scenario_writing_list_filled_paths_forwards_name_and_returns_stored_paths_unchanged(
    void) {
    int operation_ctx = -1;
    int pulse_ctx = open_writing_equilibrium(&operation_ctx);

    char **paths = NULL;
    int size = -1;
    al_status_t status = al_list_filled_paths(pulse_ctx, "equilibrium", &paths, &size);

    CHECK(status.code == 0);
    CHECK(int_from_stub("recording_stub_filled_paths_call_count") == 1);
    CHECK(int_from_stub("recording_stub_filled_paths_pctx_id") == pulse_ctx);
    CHECK(strcmp(string_from_stub("recording_stub_filled_paths_dataobjectname"), "equilibrium")
          == 0);

    /* Same seeding as the read-active scenario: the list IMAS-Core hands back
     * holds the *stored* 3.39.0 spelling, so an up-conversion the shim has not
     * implemented would show up as the 4.1.1 spelling rather than pass against
     * a string no rule could have touched either way. */
    CHECK(size == 2);
    CHECK(paths != NULL);
    CHECK(strcmp(paths[0], STORED_SPELLING) == 0);
    CHECK(strcmp(paths[1], "time_slice/global_quantities/ip") == 0);

    for (int i = 0; i < size; ++i) {
        free(paths[i]);
    }
    free(paths);

    printf("scoped_passthrough_test writing-list-filled-paths-forwards-name-and-returns-stored-"
           "paths-unchanged: an actively converting write rewrote neither the IDS name down nor "
           "the path list up\n");
}

static void scenario_writing_bind_and_unbind_plugin_forward_field_path_unchanged(void) {
    int operation_ctx = -1;
    (void)open_writing_equilibrium(&operation_ctx);

    /* Snapshotted rather than assumed to be zero: the write-mode open above
     * spends two plugin calls of its own on ADR 0020's stamp probe. */
    int calls_before = int_from_stub("recording_stub_plugin_call_count");

    CHECK(al_bind_plugin(HLI_SPELLING, "recording-plugin").code == 0);
    CHECK(int_from_stub("recording_stub_plugin_call_count") == calls_before + 1);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"), "al_bind_plugin") == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_first_string"), HLI_SPELLING) == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_second_string"), "recording-plugin") == 0);

    CHECK(al_unbind_plugin(HLI_SPELLING, "recording-plugin").code == 0);
    CHECK(int_from_stub("recording_stub_plugin_call_count") == calls_before + 2);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"), "al_unbind_plugin") == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_first_string"), HLI_SPELLING) == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_second_string"), "recording-plugin") == 0);

    printf("scoped_passthrough_test writing-bind-and-unbind-plugin-forward-field-path-unchanged: "
           "the HLI's own DD spelling reached IMAS-Core untranslated while a write converted\n");
}

int main(int argc, char **argv) {
    static const shim_test_scenario scenarios[] = {
        {"get-occurrences-forwards-ids-name-unchanged", scenario_get_occurrences_forwards_ids_name_unchanged},
        {"list-filled-paths-forwards-name-and-returns-stored-paths-unchanged", scenario_list_filled_paths_forwards_name_and_returns_stored_paths_unchanged},
        {"bind-and-unbind-plugin-forward-field-path-unchanged", scenario_bind_and_unbind_plugin_forward_field_path_unchanged},
        {"remaining-non-seam-exports-forward-unchanged", scenario_remaining_non_seam_exports_forward_unchanged},
        {"writing-get-occurrences-forwards-ids-name-unchanged", scenario_writing_get_occurrences_forwards_ids_name_unchanged},
        {"writing-list-filled-paths-forwards-name-and-returns-stored-paths-unchanged", scenario_writing_list_filled_paths_forwards_name_and_returns_stored_paths_unchanged},
        {"writing-bind-and-unbind-plugin-forward-field-path-unchanged", scenario_writing_bind_and_unbind_plugin_forward_field_path_unchanged},
    };
    return RUN_NAMED_SCENARIO(argc, argv, scenarios);
}
