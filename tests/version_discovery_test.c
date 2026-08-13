/* Issue #53, ADR 0002/0007/0009/0012: DD-version stamp discovery at the
 * al_begin_global_action seam, driven by the one read-outcome classifier,
 * plus al_begin_dataentry_action's data-entry registration.
 *
 * The HLI DD version latch (ADR 0005) and the context registry (ADR 0003)
 * are both process-wide, so every scenario below is registered as its own
 * ctest process exactly like runtime_binding_test.c's and
 * hli_dd_version_test.c's scenarios are (see CMakeLists.txt).
 *
 * There is no C-level introspection into the context registry itself (it is
 * an internal Rust module, not a shim-owned export), so "a mismatch
 * registers the occurrence" is proven the only way it is externally
 * observable at this seam: a *second* open of the same occurrence, under the
 * same pulse, translates `datapath` before IMAS-Core is called — which is
 * only possible if the first open's discovery actually cached the mismatch. */

#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <imas_mvdd_loader.h>

#ifndef RECORDING_STUB_PATH
#error "RECORDING_STUB_PATH must be defined by the build (see CMakeLists.txt)"
#endif

#define CHECK(condition)                                                       \
    do {                                                                       \
        if (!(condition)) {                                                    \
            fprintf(stderr, "check failed at %s:%d: %s\n", __FILE__, __LINE__, \
                    #condition);                                               \
            exit(EXIT_FAILURE);                                                \
        }                                                                      \
    } while (0)

typedef int (*int_accessor_fn)(void);
typedef const char *(*string_accessor_fn)(void);

static void *dlsym_or_die(void *handle, const char *name) {
    void *symbol = dlsym(handle, name);
    if (symbol == NULL) {
        fprintf(stderr, "recording stub has no symbol '%s': %s\n", name, dlerror());
        abort();
    }
    return symbol;
}

static void *open_stub_for_introspection(void) {
    void *handle = dlopen(RECORDING_STUB_PATH, RTLD_NOW | RTLD_LOCAL);
    if (handle == NULL) {
        fprintf(stderr, "failed to dlopen the recording stub for introspection: %s\n", dlerror());
        abort();
    }
    return handle;
}

static int int_from_stub(const char *symbol_name) {
    void *stub = open_stub_for_introspection();
    int_accessor_fn accessor = (int_accessor_fn)dlsym_or_die(stub, symbol_name);
    return accessor();
}

static const char *string_from_stub(const char *symbol_name) {
    void *stub = open_stub_for_introspection();
    string_accessor_fn accessor = (string_accessor_fn)dlsym_or_die(stub, symbol_name);
    return accessor();
}

static al_status_t open_dataentry(int *dectxID) {
    return al_begin_dataentry_action("imas:hdf5?path=/tmp/pulse", 7, dectxID);
}

/* READ_OP (30, al_const.h) — irrelevant to every scenario here beyond being
 * a plausible rwmode. */
static al_status_t open_global(const char *dataobjectname, const char *datapath, int *octxID) {
    return al_begin_global_action(1001, dataobjectname, datapath, 30, octxID);
}

/* --- al_begin_dataentry_action registration (ADR 0002) --------------------- */

static void scenario_dataentry_success_forwards_uri_and_mode(void) {
    int dectxID = -1;
    al_status_t status = open_dataentry(&dectxID);

    CHECK(status.code == 0);
    CHECK(dectxID == 1001);
    CHECK(int_from_stub("recording_stub_dataentry_call_count") == 1);
    CHECK(strcmp(string_from_stub("recording_stub_dataentry_uri"), "imas:hdf5?path=/tmp/pulse") == 0);
    CHECK(int_from_stub("recording_stub_dataentry_mode") == 7);

    printf("version_discovery_test dataentry-success-forwards-uri-and-mode: registered on "
           "success with uri/mode unchanged\n");
}

static void scenario_dataentry_failure_forwards_status_unchanged(void) {
    int dectxID = -1;
    al_status_t status = open_dataentry(&dectxID);

    CHECK(status.code != 0);
    CHECK(strstr(status.message, "recording-stub: dataentry open refused") != NULL);
    /* uri/mode were still recorded faithfully: the failure comes from the
     * stub after receiving them unchanged, not from the shim withholding
     * them. */
    CHECK(int_from_stub("recording_stub_dataentry_call_count") == 1);
    CHECK(strcmp(string_from_stub("recording_stub_dataentry_uri"), "imas:hdf5?path=/tmp/pulse") == 0);
    CHECK(int_from_stub("recording_stub_dataentry_mode") == 7);

    printf("version_discovery_test dataentry-failure-forwards-status-unchanged: propagated "
           "IMAS-Core's refusal without registering a context\n");
}

/* --- al_begin_global_action: HLI_V unset is a plain forward ----------------- */

static void scenario_hli_unset_global_action_is_plain_forward(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    int octxID = -1;
    al_status_t status = open_global("equilibrium", "some/datapath", &octxID);

    CHECK(status.code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), "some/datapath") == 0);
    /* No HLI DD version means no discovery at all: not even the version-
     * stamp read happens. */
    CHECK(int_from_stub("recording_stub_read_call_count") == 0);

    printf("version_discovery_test hli-unset-global-action-is-plain-forward: no discovery was "
           "attempted\n");
}

/* --- Stamp discovery outcomes (ADR 0007, ADR 0009, ADR 0012) --------------- */

static void scenario_unstamped_occurrence_forwards_datapath_unchanged(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    const char *path = "time_slice/global_quantities/beta_tor_norm";

    int octxID = -1;
    al_status_t status = open_global("equilibrium", path, &octxID);
    CHECK(status.code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), path) == 0);
    CHECK(int_from_stub("recording_stub_read_call_count") == 1);
    CHECK(strcmp(string_from_stub("recording_stub_read_field"),
                 "ids_properties/version_put/data_dictionary") == 0);

    /* Reopening the same occurrence must still forward datapath unchanged:
     * an absent stamp is never cached as a mismatch. */
    int octxID2 = -1;
    CHECK(open_global("equilibrium", path, &octxID2).code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), path) == 0);

    printf("version_discovery_test unstamped-occurrence-forwards-datapath-unchanged: no "
           "conversion record for an absent stamp\n");
}

static void scenario_matching_version_forwards_datapath_unchanged(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    const char *path = "time_slice/global_quantities/beta_tor_norm";

    int octxID = -1;
    CHECK(open_global("equilibrium", path, &octxID).code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), path) == 0);

    int octxID2 = -1;
    CHECK(open_global("equilibrium", path, &octxID2).code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), path) == 0);

    printf("version_discovery_test matching-version-forwards-datapath-unchanged: no conversion "
           "record for a matching stamp\n");
}

static void scenario_mismatch_translates_datapath_on_second_open(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    /* rename-beta-normal in docs/3.39.0--4.1.1.xml: 4.1.1's spelling on the
     * right, 3.39.0's on the left. */
    const char *hli_path = "time_slice/global_quantities/beta_tor_norm";
    const char *stored_path = "time_slice/global_quantities/beta_normal";

    int octxID = -1;
    CHECK(open_global("equilibrium", hli_path, &octxID).code == 0);
    /* First use: the stored version isn't known until this very call's
     * stamp read completes, so datapath is forwarded unchanged even though
     * it will turn out to need translation. */
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), hli_path) == 0);

    int octxID2 = -1;
    CHECK(open_global("equilibrium", hli_path, &octxID2).code == 0);
    /* Second open of the same occurrence under the same pulse: the mismatch
     * discovered by the first open's stamp read is now known, so datapath
     * is translated into the stored (3.39.0) spelling before IMAS-Core is
     * ever called. */
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), stored_path) == 0);

    printf("version_discovery_test mismatch-translates-datapath-on-second-open: a discovered "
           "mismatch translated a later open's datapath\n");
}

static void scenario_unstamped_stamp_clears_an_earlier_mismatch(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    /* Start by learning a real mismatch. The fixture changes its stamp
     * between opens to model an occurrence that is subsequently unstamped. */
    CHECK(setenv("RECORDING_STUB_STAMP_VERSION", "3.39.0", 1) == 0);
    const char *hli_path = "time_slice/global_quantities/beta_tor_norm";
    const char *stored_path = "time_slice/global_quantities/beta_normal";

    int octxID = -1;
    CHECK(open_global("equilibrium", hli_path, &octxID).code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), hli_path) == 0);

    /* This call necessarily starts from the prior discovery, so it still
     * translates before its own absent-stamp read clears that cache. */
    CHECK(unsetenv("RECORDING_STUB_STAMP_VERSION") == 0);
    int octxID2 = -1;
    CHECK(open_global("equilibrium", hli_path, &octxID2).code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), stored_path) == 0);

    /* Once the stamp is known absent, later opens must be identity forwards. */
    int octxID3 = -1;
    CHECK(open_global("equilibrium", hli_path, &octxID3).code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), hli_path) == 0);

    printf("version_discovery_test unstamped-stamp-clears-an-earlier-mismatch: absent "
           "discovery invalidated the mismatch cache\n");
}

static void scenario_malformed_stamp_refuses_and_ends_context(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    int octxID = -1;
    al_status_t status = open_global("equilibrium", "", &octxID);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(strstr(status.message, "malformed") != NULL);
    CHECK(strstr(status.message, "version_put") != NULL);

    /* The just-opened IMAS-Core context (id 2001, the stub's fixed global-
     * action octxID) must be ended so a refusal here never leaks it — the
     * HLI, told this open failed, will never call al_end_action itself. */
    CHECK(int_from_stub("recording_stub_end_action_call_count") == 1);
    CHECK(int_from_stub("recording_stub_end_action_ctx_id") == 2001);

    printf("version_discovery_test malformed-stamp-refuses-and-ends-context: refused with "
           "IMAS_MVDD_CONVERSION_ERROR and cleaned up the leaked-open context\n");
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr,
                "usage: %s "
                "<dataentry-success-forwards-uri-and-mode|"
                "dataentry-failure-forwards-status-unchanged|"
                "hli-unset-global-action-is-plain-forward|"
                "unstamped-occurrence-forwards-datapath-unchanged|"
                "matching-version-forwards-datapath-unchanged|"
                "mismatch-translates-datapath-on-second-open|"
                "unstamped-stamp-clears-an-earlier-mismatch|"
                "malformed-stamp-refuses-and-ends-context>\n",
                argv[0]);
        return 2;
    }

    const char *scenario = argv[1];
    if (strcmp(scenario, "dataentry-success-forwards-uri-and-mode") == 0) {
        scenario_dataentry_success_forwards_uri_and_mode();
    } else if (strcmp(scenario, "dataentry-failure-forwards-status-unchanged") == 0) {
        scenario_dataentry_failure_forwards_status_unchanged();
    } else if (strcmp(scenario, "hli-unset-global-action-is-plain-forward") == 0) {
        scenario_hli_unset_global_action_is_plain_forward();
    } else if (strcmp(scenario, "unstamped-occurrence-forwards-datapath-unchanged") == 0) {
        scenario_unstamped_occurrence_forwards_datapath_unchanged();
    } else if (strcmp(scenario, "matching-version-forwards-datapath-unchanged") == 0) {
        scenario_matching_version_forwards_datapath_unchanged();
    } else if (strcmp(scenario, "mismatch-translates-datapath-on-second-open") == 0) {
        scenario_mismatch_translates_datapath_on_second_open();
    } else if (strcmp(scenario, "unstamped-stamp-clears-an-earlier-mismatch") == 0) {
        scenario_unstamped_stamp_clears_an_earlier_mismatch();
    } else if (strcmp(scenario, "malformed-stamp-refuses-and-ends-context") == 0) {
        scenario_malformed_stamp_refuses_and_ends_context();
    } else {
        fprintf(stderr, "unknown scenario: %s\n", scenario);
        return 2;
    }

    return 0;
}
