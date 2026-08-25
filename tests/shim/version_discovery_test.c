/* Issue #53, ADR 0002/0007/0009/0012: DD-version stamp discovery at the
 * al_begin_global_action seam, driven by the one read-outcome classifier,
 * plus al_begin_dataentry_action's data-entry registration. Issue #55 extends
 * the same discovery-and-registration rule to al_begin_slice_action and
 * al_begin_timerange_action, which carry no `datapath` argument to translate.
 *
 * The HLI DD version latch (ADR 0005) and the context registry (ADR 0003)
 * are both process-wide, so every scenario below is registered as its own
 * ctest process exactly like runtime_binding_test.c's and
 * hli_dd_version_test.c's scenarios are (see CMakeLists.txt).
 *
 * There is no C-level introspection into the context registry itself (it is
 * an internal Rust module, not a shim-owned export), so "a mismatch
 * registers the occurrence" is proven the only way it is externally
 * observable at the global-action seam: a *second* open of the same
 * occurrence, under the same pulse, translates `datapath` before IMAS-Core is
 * called — which is only possible if the first open's discovery actually
 * cached the mismatch. Slice and time-range actions have no `datapath` to
 * translate, so their scenarios instead prove that discovery was attempted
 * (the stamp read happened) and that a malformed stamp still refuses and
 * cleans up the just-opened context exactly like global action's does. */

#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifndef RECORDING_STUB_PATH
#error "RECORDING_STUB_PATH must be defined by the build (see CMakeLists.txt)"
#endif

#include "../support/shim_test_support.h"

typedef al_status_t (*read_data_fn)(int, const char *, const char *, void **, int, int, int *);
typedef void (*set_reentrant_read_fn)(read_data_fn, const char *);

static void arm_reentrant_read(const char *field) {
    set_reentrant_read_fn arm =
        (set_reentrant_read_fn)stub_symbol_or_die("recording_stub_set_reentrant_read");
    arm(al_read_data, field);
}

static al_status_t open_dataentry(int *dectxID) {
    return al_begin_dataentry_action("imas:hdf5?path=/tmp/pulse", 7, dectxID);
}

/* READ_OP (30, al_const.h). This used to be an arbitrary plausible rwmode;
 * since ADR 0020 it decides where the stamp is read from, so every scenario
 * using this helper discovers through the caller's own context. The
 * `*_for_write` helpers further down are the other side of that. */
static al_status_t open_global(const char *dataobjectname, const char *datapath, int *octxID) {
    return al_begin_global_action(1001, dataobjectname, datapath, 30, octxID);
}

/* CLOSEST_INTERP (0, al_const.h) and an arbitrary time — irrelevant to every
 * scenario here beyond being plausible arguments. */
static al_status_t open_slice(const char *dataobjectname, int *octxID) {
    return al_begin_slice_action(1001, dataobjectname, 30, 1.5, 0, octxID);
}

static al_status_t open_timerange(const char *dataobjectname, int *octxID) {
    double dtime_buffer = 0.0;
    int dtime_shape = 0;
    return al_begin_timerange_action(1001, dataobjectname, 30, 1.0, 2.0, &dtime_buffer,
                                      &dtime_shape, 0, octxID);
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

static void scenario_failed_stamp_read_clears_an_earlier_mismatch(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    CHECK(setenv("RECORDING_STUB_STAMP_VERSION", "3.39.0", 1) == 0);
    const char *hli_path = "time_slice/global_quantities/beta_tor_norm";
    const char *stored_path = "time_slice/global_quantities/beta_normal";

    int octxID = -1;
    CHECK(open_global("equilibrium", hli_path, &octxID).code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), hli_path) == 0);

    /* The classifier must make an IMAS-Core discovery failure an unstamped
     * passthrough occurrence, not propagate it through this successful open. */
    CHECK(unsetenv("RECORDING_STUB_STAMP_VERSION") == 0);
    CHECK(setenv("RECORDING_STUB_STAMP_READ_FAIL", "1", 1) == 0);
    int octxID2 = -1;
    CHECK(open_global("equilibrium", hli_path, &octxID2).code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), stored_path) == 0);

    CHECK(unsetenv("RECORDING_STUB_STAMP_READ_FAIL") == 0);
    int octxID3 = -1;
    CHECK(open_global("equilibrium", hli_path, &octxID3).code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), hli_path) == 0);

    printf("version_discovery_test failed-stamp-read-clears-an-earlier-mismatch: failed "
           "discovery was an unstamped passthrough and cleared the cache\n");
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

/* Issue #108 AC4: the stamp reader itself runs under ADR 0014's depth guard.
 * The first open leaves a live mismatch record at the stub's recycled context
 * ID. The second open's stamp read then makes the stub re-enter the shim with
 * an HLI-spelled renamed path: without the guard, that live record would
 * translate it before Core sees it. */
static void scenario_reentrant_read_beneath_stamp_discovery_forwards_unchanged(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    int first_octxID = -1;
    CHECK(open_global("equilibrium", "", &first_octxID).code == 0);

    const char *reentrant_field = "time_slice/global_quantities/beta_tor_norm";
    arm_reentrant_read(reentrant_field);

    int second_octxID = -1;
    CHECK(open_global("equilibrium", "", &second_octxID).code == 0);

    CHECK(int_from_stub("recording_stub_reentrant_call_count") == 1);
    CHECK(strcmp(string_from_stub("recording_stub_reentrant_seen_field"), reentrant_field) == 0);
    CHECK(strcmp(string_from_stub("recording_stub_reentrant_seen_timebase"), "") == 0);

    printf("version_discovery_test reentrant-read-beneath-stamp-discovery-forwards-unchanged: "
           "the callback beneath a stamp read reached Core without conversion\n");
}

/* --- al_begin_slice_action applies the same rule as global action (issue #55) --- */

static void scenario_slice_action_hli_unset_is_plain_forward(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    int octxID = -1;
    al_status_t status = open_slice("equilibrium", &octxID);

    CHECK(status.code == 0);
    CHECK(octxID == 2002);
    CHECK(strcmp(string_from_stub("recording_stub_slice_dataobjectname"), "equilibrium") == 0);
    /* No HLI DD version means no discovery at all: not even the version-
     * stamp read happens. */
    CHECK(int_from_stub("recording_stub_read_call_count") == 0);

    printf("version_discovery_test slice-action-hli-unset-is-plain-forward: no discovery was "
           "attempted\n");
}

static void scenario_slice_action_unstamped_forwards_ids_name_unchanged(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    int octxID = -1;
    al_status_t status = open_slice("equilibrium", &octxID);

    CHECK(status.code == 0);
    CHECK(octxID == 2002);
    CHECK(strcmp(string_from_stub("recording_stub_slice_dataobjectname"), "equilibrium") == 0);
    CHECK(int_from_stub("recording_stub_read_call_count") == 1);
    CHECK(strcmp(string_from_stub("recording_stub_read_field"),
                 "ids_properties/version_put/data_dictionary") == 0);

    printf("version_discovery_test slice-action-unstamped-forwards-ids-name-unchanged: discovery "
           "was attempted and the IDS name reached IMAS-Core unchanged\n");
}

static void scenario_slice_action_matching_version_forwards_ids_name_unchanged(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    int octxID = -1;
    al_status_t status = open_slice("equilibrium", &octxID);

    CHECK(status.code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_slice_dataobjectname"), "equilibrium") == 0);

    printf("version_discovery_test slice-action-matching-version-forwards-ids-name-unchanged: a "
           "matching stamp is a passthrough with the open still succeeding\n");
}

static void scenario_slice_action_mismatch_registers_occurrence_for_global_action(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    int octxID = -1;
    CHECK(open_slice("equilibrium", &octxID).code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_slice_dataobjectname"), "equilibrium") == 0);

    /* There is no C-level introspection into the context registry itself
     * (see file header), so a slice action's discovered mismatch being
     * classified and cached is only externally observable through another
     * seam that reads the same occurrence cache: a subsequent global action
     * on the same occurrence, under the same pulse, now translates its
     * datapath before IMAS-Core is ever called — which is only possible if
     * the slice action's own discovery already cached the mismatch. */
    const char *hli_path = "time_slice/global_quantities/beta_tor_norm";
    const char *stored_path = "time_slice/global_quantities/beta_normal";
    int octxID2 = -1;
    CHECK(open_global("equilibrium", hli_path, &octxID2).code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), stored_path) == 0);

    printf("version_discovery_test slice-action-mismatch-registers-occurrence-for-global-action: "
           "a slice action's discovered mismatch translated a later global action's datapath\n");
}

static void scenario_slice_action_malformed_stamp_refuses_and_ends_context(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    int octxID = -1;
    al_status_t status = open_slice("equilibrium", &octxID);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(strstr(status.message, "malformed") != NULL);
    CHECK(strstr(status.message, "version_put") != NULL);
    CHECK(strcmp(string_from_stub("recording_stub_slice_dataobjectname"), "equilibrium") == 0);

    /* The just-opened IMAS-Core context (id 2002, the stub's fixed slice-
     * action octxID) must be ended so a refusal here never leaks it. */
    CHECK(int_from_stub("recording_stub_end_action_call_count") == 1);
    CHECK(int_from_stub("recording_stub_end_action_ctx_id") == 2002);

    printf("version_discovery_test slice-action-malformed-stamp-refuses-and-ends-context: refused "
           "with IMAS_MVDD_CONVERSION_ERROR and cleaned up the leaked-open context\n");
}

static void scenario_slice_action_failure_forwards_status_unchanged(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    int octxID = -1;
    al_status_t status = open_slice("equilibrium", &octxID);

    CHECK(status.code != 0);
    CHECK(strstr(status.message, "recording-stub: slice open refused") != NULL);
    /* dataobjectname was still recorded faithfully: the failure comes from
     * the stub after receiving it unchanged, not from the shim withholding
     * it. */
    CHECK(strcmp(string_from_stub("recording_stub_slice_dataobjectname"), "equilibrium") == 0);
    /* A failed open must attempt no stamp discovery and leak no context to
     * clean up. */
    CHECK(int_from_stub("recording_stub_read_call_count") == 0);
    CHECK(int_from_stub("recording_stub_end_action_call_count") == 0);

    printf("version_discovery_test slice-action-failure-forwards-status-unchanged: propagated "
           "IMAS-Core's refusal without attempting discovery or registering a context\n");
}

/* --- al_begin_timerange_action applies the same rule as global action (issue #55) --- */

static void scenario_timerange_action_hli_unset_is_plain_forward(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    int octxID = -1;
    al_status_t status = open_timerange("equilibrium", &octxID);

    CHECK(status.code == 0);
    CHECK(octxID == 2003);
    CHECK(strcmp(string_from_stub("recording_stub_timerange_dataobjectname"), "equilibrium") == 0);
    /* No HLI DD version means no discovery at all: not even the version-
     * stamp read happens. */
    CHECK(int_from_stub("recording_stub_read_call_count") == 0);

    printf("version_discovery_test timerange-action-hli-unset-is-plain-forward: no discovery was "
           "attempted\n");
}

static void scenario_timerange_action_unstamped_forwards_ids_name_unchanged(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    int octxID = -1;
    al_status_t status = open_timerange("equilibrium", &octxID);

    CHECK(status.code == 0);
    CHECK(octxID == 2003);
    CHECK(strcmp(string_from_stub("recording_stub_timerange_dataobjectname"), "equilibrium") == 0);
    CHECK(int_from_stub("recording_stub_read_call_count") == 1);
    CHECK(strcmp(string_from_stub("recording_stub_read_field"),
                 "ids_properties/version_put/data_dictionary") == 0);

    printf("version_discovery_test timerange-action-unstamped-forwards-ids-name-unchanged: "
           "discovery was attempted and the IDS name reached IMAS-Core unchanged\n");
}

static void scenario_timerange_action_matching_version_forwards_ids_name_unchanged(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    int octxID = -1;
    al_status_t status = open_timerange("equilibrium", &octxID);

    CHECK(status.code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_timerange_dataobjectname"), "equilibrium") == 0);

    printf("version_discovery_test timerange-action-matching-version-forwards-ids-name-unchanged: "
           "a matching stamp is a passthrough with the open still succeeding\n");
}

static void scenario_timerange_action_mismatch_registers_occurrence_for_global_action(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    int octxID = -1;
    CHECK(open_timerange("equilibrium", &octxID).code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_timerange_dataobjectname"), "equilibrium") == 0);

    /* There is no C-level introspection into the context registry itself
     * (see file header), so a time-range action's discovered mismatch being
     * classified and cached is only externally observable through another
     * seam that reads the same occurrence cache: a subsequent global action
     * on the same occurrence, under the same pulse, now translates its
     * datapath before IMAS-Core is ever called — which is only possible if
     * the time-range action's own discovery already cached the mismatch. */
    const char *hli_path = "time_slice/global_quantities/beta_tor_norm";
    const char *stored_path = "time_slice/global_quantities/beta_normal";
    int octxID2 = -1;
    CHECK(open_global("equilibrium", hli_path, &octxID2).code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), stored_path) == 0);

    printf("version_discovery_test timerange-action-mismatch-registers-occurrence-for-global-"
           "action: a time-range action's discovered mismatch translated a later global action's "
           "datapath\n");
}

static void scenario_timerange_action_malformed_stamp_refuses_and_ends_context(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    int octxID = -1;
    al_status_t status = open_timerange("equilibrium", &octxID);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(strstr(status.message, "malformed") != NULL);
    CHECK(strstr(status.message, "version_put") != NULL);
    CHECK(strcmp(string_from_stub("recording_stub_timerange_dataobjectname"), "equilibrium") == 0);

    /* The just-opened IMAS-Core context (id 2003, the stub's fixed
     * time-range-action octxID) must be ended so a refusal here never
     * leaks it. */
    CHECK(int_from_stub("recording_stub_end_action_call_count") == 1);
    CHECK(int_from_stub("recording_stub_end_action_ctx_id") == 2003);

    printf("version_discovery_test timerange-action-malformed-stamp-refuses-and-ends-context: "
           "refused with IMAS_MVDD_CONVERSION_ERROR and cleaned up the leaked-open context\n");
}

static void scenario_timerange_action_failure_forwards_status_unchanged(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    int octxID = -1;
    al_status_t status = open_timerange("equilibrium", &octxID);

    CHECK(status.code != 0);
    CHECK(strstr(status.message, "recording-stub: timerange open refused") != NULL);
    /* dataobjectname was still recorded faithfully: the failure comes from
     * the stub after receiving it unchanged, not from the shim withholding
     * it. */
    CHECK(strcmp(string_from_stub("recording_stub_timerange_dataobjectname"), "equilibrium") == 0);
    /* A failed open must attempt no stamp discovery and leak no context to
     * clean up. */
    CHECK(int_from_stub("recording_stub_read_call_count") == 0);
    CHECK(int_from_stub("recording_stub_end_action_call_count") == 0);

    printf("version_discovery_test timerange-action-failure-forwards-status-unchanged: propagated "
           "IMAS-Core's refusal without attempting discovery or registering a context\n");
}

/* --- ADR 0020: where a write-mode open reads the stamp from -------------- */

/* WRITE_OP (31, al_const.h). Unlike the READ_OP above this one is load-bearing:
 * it is what makes the stamp probe fire. */
static al_status_t open_global_for_write(const char *dataobjectname, const char *datapath,
                                        int *octxID) {
    return al_begin_global_action(1001, dataobjectname, datapath, 31, octxID);
}

static al_status_t open_slice_for_write(const char *dataobjectname, int *octxID) {
    return al_begin_slice_action(1001, dataobjectname, 31, 1.5, 0, octxID);
}

/* The stamp probe is invisible in the shim's *own* exports, so it is observed
 * where it is externally observable: the recording stub's plugin-call
 * recorder. IMAS-Core's `al_plugin_*` pair is the plugin-free primitive that
 * ADR 0020 decision 3 has the probe use, and no scenario in this suite calls
 * that family for any other reason, so a plugin call here is the probe. */
static void scenario_write_mode_open_probes_through_the_plugin_family(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    const char *hli_path = "time_slice/global_quantities/beta_tor_norm";
    const char *stored_path = "time_slice/global_quantities/beta_normal";

    int octxID = -1;
    CHECK(open_global_for_write("equilibrium", hli_path, &octxID).code == 0);

    /* Exactly two plugin calls: the probe's own open and its own close. */
    CHECK(int_from_stub("recording_stub_plugin_call_count") == 2);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"), "al_plugin_end_action") ==
          0);
    /* Closed through the family it was opened with, on its own context id
     * (the stub hands 5001 to every al_plugin_begin_global_action) and not on
     * the caller's. */
    CHECK(int_from_stub("recording_stub_plugin_last_ctx") == 5001);
    CHECK(octxID != 5001);

    /* The caller's own open is untouched by the probe: still WRITE_OP, still
     * its own datapath on an occurrence's first use. */
    CHECK(int_from_stub("recording_stub_global_rwmode") == 31);
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), hli_path) == 0);

    /* And the probe's stamp read is what registered the mismatch: a second
     * write-mode open of the same occurrence translates datapath, which is
     * only possible if the first open's discovery cached it. */
    int octxID2 = -1;
    CHECK(open_global_for_write("equilibrium", hli_path, &octxID2).code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), stored_path) == 0);

    printf("version_discovery_test write-mode-open-probes-through-the-plugin-family: a WRITE_OP "
           "open discovered the stamp through a shim-owned plugin-family context it opened and "
           "closed itself\n");
}

/* What the probe asks IMAS-Core for, and what happens when it cannot have it.
 * RECORDING_STUB_PLUGIN_GLOBAL_FAIL refuses the probe's own open, which both
 * freezes the recorder on that call — `record_plugin_call` resets the ints, so
 * the probe's rwmode is only readable while the open is the last plugin call —
 * and drives ADR 0020 decision 5: a probe that cannot be opened is
 * `StampOutcome::Unstamped`, never a failure of the caller's own open. */
static void scenario_write_mode_probe_asks_for_a_read_context(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    const char *hli_path = "time_slice/global_quantities/beta_tor_norm";

    int octxID = -1;
    CHECK(open_global_for_write("equilibrium", hli_path, &octxID).code == 0);

    CHECK(int_from_stub("recording_stub_plugin_call_count") == 1);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"),
                 "al_plugin_begin_global_action") == 0);
    /* READ_OP (30), whatever mode the caller used — that is the whole point. */
    CHECK(int_from_stub("recording_stub_plugin_first_int") == 30);
    /* Same occurrence, and no datapath: the probe asks only for the stamp. */
    CHECK(strcmp(string_from_stub("recording_stub_plugin_first_string"), "equilibrium") == 0);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_second_string"), "") == 0);

    /* A refused probe registers nothing, so a second open still forwards the
     * caller's own datapath rather than translating it. */
    int octxID2 = -1;
    CHECK(open_global_for_write("equilibrium", hli_path, &octxID2).code == 0);
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), hli_path) == 0);

    printf("version_discovery_test write-mode-probe-asks-for-a-read-context: the probe opened "
           "READ_OP on the same occurrence with no datapath, and its refusal left the caller's "
           "own open succeeding with nothing registered\n");
}

/* ADR 0020 decision 4: the stamp is a non-timed STR_0D, so a slice seam's
 * probe is a *global* action too. Proven with the same fail knob, because a
 * refused probe is the only state in which the recorder still holds the
 * probe's own open rather than its close. */
static void scenario_write_mode_slice_open_probes_with_a_global_action(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    int octxID = -1;
    CHECK(open_slice_for_write("equilibrium", &octxID).code == 0);

    CHECK(int_from_stub("recording_stub_plugin_call_count") == 1);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"),
                 "al_plugin_begin_global_action") == 0);
    CHECK(int_from_stub("recording_stub_plugin_first_int") == 30);
    /* The caller's own slice open still carries its own arguments. */
    CHECK(int_from_stub("recording_stub_slice_rwmode") == 31);

    printf("version_discovery_test write-mode-slice-open-probes-with-a-global-action: a WRITE_OP "
           "slice open probed the stamp through a global action, not a slice one\n");
}

/* REPLACE_OP (32, al_const.h) is the mode decision 1's predicate exists for.
 * IMAS-Core's headers accept it on all three occurrence-opening seams, and
 * HDF5's own `beginAction` initializes neither a reader nor a writer for it, so
 * a `rwmode == WRITE_OP` predicate would have left it silently unconverted.
 * Nothing else in this project drives REPLACE_OP; this is its only coverage,
 * and it is here because ADR 0011 asks a mechanism to be exercised rather
 * than assumed. */
static void scenario_replace_mode_open_probes_too(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    int octxID = -1;
    CHECK(al_begin_global_action(1001, "equilibrium", "", 32, &octxID).code == 0);

    CHECK(int_from_stub("recording_stub_plugin_call_count") == 1);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"),
                 "al_plugin_begin_global_action") == 0);
    CHECK(int_from_stub("recording_stub_plugin_first_int") == 30);
    CHECK(int_from_stub("recording_stub_global_rwmode") == 32);

    printf("version_discovery_test replace-mode-open-probes-too: a REPLACE_OP open probed the "
           "stamp through a read-mode context, which a WRITE_OP-only predicate would not have "
           "done\n");
}

/* The third occurrence-opening seam. Issue #136 names only global and slice,
 * but all three share one adapter and one `rwmode`, so leaving time-range out
 * would have been a hole behind a shared function (ADR 0020 decision 4). */
static void scenario_write_mode_timerange_open_probes_with_a_global_action(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    double dtime_buffer = 0.0;
    int dtime_shape = 0;
    int octxID = -1;
    CHECK(al_begin_timerange_action(1001, "equilibrium", 31, 1.0, 2.0, &dtime_buffer, &dtime_shape,
                                     0, &octxID)
              .code == 0);

    CHECK(int_from_stub("recording_stub_plugin_call_count") == 1);
    CHECK(strcmp(string_from_stub("recording_stub_plugin_last_symbol"),
                 "al_plugin_begin_global_action") == 0);
    CHECK(int_from_stub("recording_stub_plugin_first_int") == 30);
    CHECK(int_from_stub("recording_stub_timerange_rwmode") == 31);

    printf("version_discovery_test write-mode-timerange-open-probes-with-a-global-action: a "
           "WRITE_OP time-range open probed the stamp through a global read-mode context\n");
}

/* The other half of decision 1, and what keeps this suite's other 21 scenarios
 * meaning what they meant before ADR 0020: a READ_OP open probes nothing,
 * because the context it was handed can already answer. */
static void scenario_read_mode_open_does_not_probe(void) {
    int dectxID = -1;
    CHECK(open_dataentry(&dectxID).code == 0);

    const char *hli_path = "time_slice/global_quantities/beta_tor_norm";
    const char *stored_path = "time_slice/global_quantities/beta_normal";

    int octxID = -1;
    CHECK(open_global("equilibrium", hli_path, &octxID).code == 0);
    CHECK(int_from_stub("recording_stub_plugin_call_count") == 0);

    /* Discovery still happened, through the caller's own context. */
    int octxID2 = -1;
    CHECK(open_global("equilibrium", hli_path, &octxID2).code == 0);
    CHECK(int_from_stub("recording_stub_plugin_call_count") == 0);
    CHECK(strcmp(string_from_stub("recording_stub_global_datapath"), stored_path) == 0);

    printf("version_discovery_test read-mode-open-does-not-probe: a READ_OP open discovered the "
           "stamp through its own context and opened no probe at all\n");
}

int main(int argc, char **argv) {
    static const shim_test_scenario scenarios[] = {
        {"dataentry-success-forwards-uri-and-mode", scenario_dataentry_success_forwards_uri_and_mode},
        {"dataentry-failure-forwards-status-unchanged", scenario_dataentry_failure_forwards_status_unchanged},
        {"hli-unset-global-action-is-plain-forward", scenario_hli_unset_global_action_is_plain_forward},
        {"unstamped-occurrence-forwards-datapath-unchanged", scenario_unstamped_occurrence_forwards_datapath_unchanged},
        {"matching-version-forwards-datapath-unchanged", scenario_matching_version_forwards_datapath_unchanged},
        {"mismatch-translates-datapath-on-second-open", scenario_mismatch_translates_datapath_on_second_open},
        {"write-mode-open-probes-through-the-plugin-family",
         scenario_write_mode_open_probes_through_the_plugin_family},
        {"write-mode-probe-asks-for-a-read-context",
         scenario_write_mode_probe_asks_for_a_read_context},
        {"write-mode-slice-open-probes-with-a-global-action",
         scenario_write_mode_slice_open_probes_with_a_global_action},
        {"replace-mode-open-probes-too", scenario_replace_mode_open_probes_too},
        {"write-mode-timerange-open-probes-with-a-global-action",
         scenario_write_mode_timerange_open_probes_with_a_global_action},
        {"read-mode-open-does-not-probe", scenario_read_mode_open_does_not_probe},
        {"unstamped-stamp-clears-an-earlier-mismatch", scenario_unstamped_stamp_clears_an_earlier_mismatch},
        {"failed-stamp-read-clears-an-earlier-mismatch", scenario_failed_stamp_read_clears_an_earlier_mismatch},
        {"malformed-stamp-refuses-and-ends-context", scenario_malformed_stamp_refuses_and_ends_context},
        {"reentrant-read-beneath-stamp-discovery-forwards-unchanged", scenario_reentrant_read_beneath_stamp_discovery_forwards_unchanged},
        {"slice-action-hli-unset-is-plain-forward", scenario_slice_action_hli_unset_is_plain_forward},
        {"slice-action-unstamped-forwards-ids-name-unchanged", scenario_slice_action_unstamped_forwards_ids_name_unchanged},
        {"slice-action-matching-version-forwards-ids-name-unchanged", scenario_slice_action_matching_version_forwards_ids_name_unchanged},
        {"slice-action-mismatch-registers-occurrence-for-global-action", scenario_slice_action_mismatch_registers_occurrence_for_global_action},
        {"slice-action-malformed-stamp-refuses-and-ends-context", scenario_slice_action_malformed_stamp_refuses_and_ends_context},
        {"slice-action-failure-forwards-status-unchanged", scenario_slice_action_failure_forwards_status_unchanged},
        {"timerange-action-hli-unset-is-plain-forward", scenario_timerange_action_hli_unset_is_plain_forward},
        {"timerange-action-unstamped-forwards-ids-name-unchanged", scenario_timerange_action_unstamped_forwards_ids_name_unchanged},
        {"timerange-action-matching-version-forwards-ids-name-unchanged", scenario_timerange_action_matching_version_forwards_ids_name_unchanged},
        {"timerange-action-mismatch-registers-occurrence-for-global-action", scenario_timerange_action_mismatch_registers_occurrence_for_global_action},
        {"timerange-action-malformed-stamp-refuses-and-ends-context", scenario_timerange_action_malformed_stamp_refuses_and_ends_context},
        {"timerange-action-failure-forwards-status-unchanged", scenario_timerange_action_failure_forwards_status_unchanged},
    };
    return RUN_NAMED_SCENARIO(argc, argv, scenarios);
}
