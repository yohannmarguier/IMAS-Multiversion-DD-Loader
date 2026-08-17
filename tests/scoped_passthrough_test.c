/* Issue #69: conversion is scoped, and the scope has an outside edge.
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

#include <imas_mvdd_loader.h>

#ifndef RECORDING_STUB_PATH
#error "RECORDING_STUB_PATH must be defined by CMakeLists.txt"
#endif
#ifndef EXPECTED_AL_VERSION
#error "EXPECTED_AL_VERSION must be defined by CMakeLists.txt"
#endif

/* The two spellings of the one path `rename-beta-normal` relates. Reaching
 * IMAS-Core, or coming back from it, as the *other* one of this pair is
 * exactly the failure these scenarios exist to catch. */
#define HLI_SPELLING "time_slice/global_quantities/beta_tor_norm"
#define STORED_SPELLING "time_slice/global_quantities/beta_normal"

/* IMAS-Core's data-type codes, spelled out because the recording-stub profile
 * deliberately acquires no IMAS-Core and so has no al_const.h to include.
 * The values are `DATA_TYPE_0` (50) plus an offset, per IMAS-Core's
 * al_defs.h.in — not small ordinals, which is an easy and silent mistake to
 * make in a test that passes a bare literal. */
#define IMAS_CHAR_DATA 50
#define IMAS_INTEGER_DATA 51
#define IMAS_DOUBLE_DATA 52
#define IMAS_COMPLEX_DATA 53

#define CHECK(condition)                                                       \
    do {                                                                       \
        if (!(condition)) {                                                    \
            fprintf(stderr, "check failed at %s:%d: %s\n", __FILE__, __LINE__, \
                    #condition);                                               \
            exit(EXIT_FAILURE);                                                \
        }                                                                      \
    } while (0)

typedef const char *(*string_accessor_fn)(void);
typedef int (*int_accessor_fn)(void);
typedef double (*double_accessor_fn)(void);
typedef const void *(*pointer_accessor_fn)(void);

static void *stub_handle(void) {
    void *stub = dlopen(RECORDING_STUB_PATH, RTLD_NOW | RTLD_LOCAL);
    if (stub == NULL) {
        fprintf(stderr, "failed to open recording stub: %s\n", dlerror());
        abort();
    }
    return stub;
}

static void *dlsym_or_die(const char *name) {
    void *symbol = dlsym(stub_handle(), name);
    if (symbol == NULL) {
        fprintf(stderr, "recording stub has no symbol '%s': %s\n", name, dlerror());
        abort();
    }
    return symbol;
}

static const char *string_from_stub(const char *symbol_name) {
    return ((string_accessor_fn)dlsym_or_die(symbol_name))();
}

static int int_from_stub(const char *symbol_name) {
    return ((int_accessor_fn)dlsym_or_die(symbol_name))();
}

static double double_from_stub(const char *symbol_name) {
    return ((double_accessor_fn)dlsym_or_die(symbol_name))();
}

static const void *pointer_from_stub(const char *symbol_name) {
    return ((pointer_accessor_fn)dlsym_or_die(symbol_name))();
}

/* Opens the pulse and an equilibrium global action whose supplied stamp makes
 * the stored DD version differ from the latched HLI one, leaving the resulting
 * mismatch record live for the caller's whole scenario. Returns the pulse
 * context, since the two enumeration seams below are addressed by it. */
static int open_mismatched_equilibrium(int *operation_ctx) {
    int pulse_ctx = -1;
    CHECK(al_begin_dataentry_action("imas:hdf5?path=/tmp/pulse", 7, &pulse_ctx).code == 0);
    CHECK(int_from_stub("recording_stub_dataentry_call_count") == 1);
    CHECK(strcmp(string_from_stub("recording_stub_dataentry_uri"),
                 "imas:hdf5?path=/tmp/pulse")
          == 0);
    CHECK(int_from_stub("recording_stub_dataentry_mode") == 7);

    *operation_ctx = -1;
    CHECK(al_begin_global_action(pulse_ctx, "equilibrium", "", 30, operation_ctx).code == 0);

    /* Conversion really is active on this context: the same rule whose two
     * spellings the passthrough assertions use translates an ordinary read.
     * Without this, a scenario below could pass simply because the mismatch
     * was never registered. */
    void *data = NULL;
    int size[1] = {0};
    CHECK(al_read_data(*operation_ctx, HLI_SPELLING, "", &data, IMAS_DOUBLE_DATA, 1, size).code
          == 0);
    CHECK(strcmp(string_from_stub("recording_stub_read_field"), STORED_SPELLING) == 0);

    return pulse_ctx;
}

static void scenario_get_occurrences_forwards_ids_name_unchanged(void) {
    int operation_ctx = -1;
    int pulse_ctx = open_mismatched_equilibrium(&operation_ctx);

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
    int pulse_ctx = open_mismatched_equilibrium(&operation_ctx);

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
    (void)open_mismatched_equilibrium(&operation_ctx);

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
    int pulse_ctx = open_mismatched_equilibrium(&operation_ctx);

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

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr,
                "usage: %s "
                "<get-occurrences-forwards-ids-name-unchanged|"
                "list-filled-paths-forwards-name-and-returns-stored-paths-unchanged|"
                "bind-and-unbind-plugin-forward-field-path-unchanged|"
                "remaining-non-seam-exports-forward-unchanged>\n",
                argv[0]);
        return 2;
    }

    const char *scenario = argv[1];
    if (strcmp(scenario, "get-occurrences-forwards-ids-name-unchanged") == 0) {
        scenario_get_occurrences_forwards_ids_name_unchanged();
    } else if (strcmp(scenario,
                      "list-filled-paths-forwards-name-and-returns-stored-paths-unchanged")
               == 0) {
        scenario_list_filled_paths_forwards_name_and_returns_stored_paths_unchanged();
    } else if (strcmp(scenario, "bind-and-unbind-plugin-forward-field-path-unchanged") == 0) {
        scenario_bind_and_unbind_plugin_forward_field_path_unchanged();
    } else if (strcmp(scenario, "remaining-non-seam-exports-forward-unchanged") == 0) {
        scenario_remaining_non_seam_exports_forward_unchanged();
    } else {
        fprintf(stderr, "unknown scenario: %s\n", scenario);
        return 2;
    }

    return 0;
}
