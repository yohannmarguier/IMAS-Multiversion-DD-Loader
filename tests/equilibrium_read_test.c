/* Issue #54: the first complete real-Core tracer bullet — version opt-in,
 * stamp discovery, context registration, rule resolution, translated read,
 * and HLI-owned result buffer — proven bidirectionally against the checked-in
 * equilibrium HDF5 fixture pair (`imas-python-fixtures/fixtures`).
 *
 * `time_slice/global_quantities/beta_normal` (DD 3.39.0) is
 * `time_slice/global_quantities/beta_tor_norm` (DD 4.1.1) — `rename-beta-
 * normal` in docs/3.39.0--4.1.1.xml, `fidelity exact` both ways, no value
 * transformation. `equilibrium_values.py`'s PINNED `beta_tor_norm` is
 * `1.8 + 0.1*i`; slice 0 (`TIME[0] == 1.0`) is `1.8`.
 *
 * Every scenario opens a real HDF5 pulse and calls al_begin_global_action,
 * al_begin_arraystruct_action("time_slice"), and al_read_data through the
 * public C ABI only — there is no way to introspect the context registry
 * from C, so a translated read is observed the only way it is externally
 * observable: the read succeeds and returns the pinned literal, addressed by
 * the HLI's own DD spelling, from a fixture that spells the field the other
 * way. The HLI DD version latch is process-wide, so each scenario below is
 * registered as its own ctest process, exactly like version_discovery_test.c's
 * scenarios are.
 *
 * Issue #62 adds the scenarios beneath
 * `read_nested_constraint_scalar_at_slice_zero`: every scenario above reads
 * relative to `time_slice`, whose own anchor spells identically on both DD
 * sides, so it never exercises translating a *renamed* child context's own
 * anchor before stripping it back off a relative read (`resolve::
 * stored_anchor`). `constraints/bpol_probe` / `constraints/b_field_pol_probe`
 * (`rename-bpol-probe`) is such an anchor; `constraints/flux_loop` (identical
 * on both sides) carries a COCOS sign flip on `measured` instead, proving a
 * supported value transformation also applies unchanged beneath a nested
 * context. `equilibrium_values.py`'s PINNED `b_field_pol_probe_measured` and
 * `flux_loop_measured` are `0.42 + 0.01*i + 0.10*k` and `1.15 + 0.01*i +
 * 0.10*k`; slice 0, constraint 0 (`i == k == 0`) are `0.42` and `1.15`. The
 * 4.1.1 fixture writes `flux_loop`'s COCOS-17 value, `-1.15`.
 *
 * A note on the direction labels, because two conventions meet in this file:
 * "reverse" below always means *an HLI declaring 3.39.0 reads the 4.1.1
 * fixture*, and "forward" *an HLI declaring 4.1.1 reads the 3.39.0 fixture*.
 * That is the opposite of `conversion_map::Direction`, which is named after
 * which side of the map the *supplied* path comes from — a 3.39.0 HLI supplies
 * a left path and so travels `Direction::Forward`. The labels here name the
 * fixture under test, which is what a reader of the ctest list is choosing
 * between; CMakeLists.txt's read_path_test comments name the shim's enum.
 *
 * Issue #69 adds the refusal scenarios: every scenario above proves a read the
 * shim can serve, and a validation matrix that only ever demonstrates success
 * would not distinguish a working converter from one that silently serves the
 * wrong bytes when a rule says it must not. The two paths used refuse for
 * deliberately different reasons, and only one of them refuses because of its
 * declared fidelity:
 *   - `time_slice/constraints/strike_point/chi_squared_r` is declared
 *     `unmappable` in both directions by a `redefine` entry — its unit changed
 *     from `m` to `m^-2`, and the variance needed to invert that is not stored.
 *   - `grids_ggd/grid/space/coordinates_type` is the artifact's one `retyped`
 *     rule, and it is declared `exact` both ways ("integers preserved; only the
 *     container changes"). It still refuses, because `Rel::Retyped` resolves to
 *     `RefusalReason::UnservableRetype` regardless of fidelity: the shim cannot
 *     reshape an int array into an array of identifier structures, so a
 *     conversion that is lossless in principle is unavailable in practice.
 *     That distinction is the reason this path is worth a scenario — refusal
 *     follows what the shim can serve, not only what the artifact calls lossy.
 * Both are logged `UNMAPPABLE`: from the caller's side a refused read yielded
 * no value, whatever the rule's declared fidelity was. Both refuse before
 * IMAS-Core is called, so each is asserted against a real open pulse whose data
 * is deliberately never reached. */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <al_const.h>
#include <imas_mvdd_loader.h>

#ifndef EQUILIBRIUM_FIXTURE_DIR
#error "EQUILIBRIUM_FIXTURE_DIR must name the imas-python-fixtures/fixtures directory"
#endif

#define CHECK(condition)                                                     \
    do {                                                                     \
        if (!(condition)) {                                                  \
            fprintf(stderr, "check failed at %s:%d: %s\n", __FILE__, __LINE__, \
                    #condition);                                             \
            exit(EXIT_FAILURE);                                              \
        }                                                                     \
    } while (0)

static void check_ok(al_status_t status, const char *expression, int line) {
    if (status.code != 0) {
        fprintf(stderr, "IMAS-Core call failed at %s:%d: %s: code=%d message=%s\n", __FILE__,
                line, expression, status.code, status.message);
        exit(EXIT_FAILURE);
    }
}

#define CHECK_OK(expression) check_ok((expression), #expression, __LINE__)

/* Opens the checked-in equilibrium fixture for `dd_version` ("3.39.0" or
 * "4.1.1") read-only-in-practice: nothing here ever writes to it. */
static int open_fixture_pulse(const char *dd_version) {
    char uri[1024];
    int length = snprintf(uri, sizeof uri, "imas:hdf5?path=%s/dd-%s", EQUILIBRIUM_FIXTURE_DIR,
                           dd_version);
    CHECK(length > 0 && (size_t)length < sizeof uri);

    int pulse_ctx = -1;
    CHECK_OK(al_begin_dataentry_action(uri, OPEN_PULSE, &pulse_ctx));
    return pulse_ctx;
}

/* Opens "equilibrium", then the "time_slice" AOS, and reads `field` (in the
 * caller's own DD spelling) from time slice 0. IMAS-Core's scalar ABI
 * requires HLI-provided storage: it copies into that buffer and frees its own
 * temporary allocation before returning. Pointer identity therefore proves
 * the shim neither substitutes nor frees the HLI-owned result buffer. */
static double read_scalar_at_slice_zero(int pulse_ctx, const char *field) {
    int op_ctx = -1;
    CHECK_OK(al_begin_global_action(pulse_ctx, "equilibrium", "", READ_OP, &op_ctx));

    int size = -1;
    int aos_ctx = -1;
    CHECK_OK(al_begin_arraystruct_action(op_ctx, "time_slice", "", &size, &aos_ctx));
    CHECK(size == 2);

    int shape[MAXDIM] = {0};
    double value = -1.0;
    void *buffer = &value;
    CHECK_OK(al_read_data(aos_ctx, field, "", &buffer, DOUBLE_DATA, 0, shape));
    CHECK(buffer == &value);

    CHECK_OK(al_end_action(aos_ctx));
    CHECK_OK(al_end_action(op_ctx));
    return value;
}

/* Like `read_scalar_at_slice_zero`, but reads `leaf_field` from constraint 0
 * of the AOS at `aos_field`, itself nested beneath `time_slice`'s own AOS
 * context (in the caller's own DD spelling — `aos_field` and `leaf_field`
 * are never translated by this helper). */
static double read_nested_constraint_scalar_at_slice_zero(int pulse_ctx, const char *aos_field,
                                                           const char *leaf_field) {
    int op_ctx = -1;
    CHECK_OK(al_begin_global_action(pulse_ctx, "equilibrium", "", READ_OP, &op_ctx));

    int time_slice_size = -1;
    int time_slice_ctx = -1;
    CHECK_OK(
        al_begin_arraystruct_action(op_ctx, "time_slice", "", &time_slice_size, &time_slice_ctx));
    CHECK(time_slice_size == 2);

    int aos_size = -1;
    int aos_ctx = -1;
    CHECK_OK(al_begin_arraystruct_action(time_slice_ctx, aos_field, "", &aos_size, &aos_ctx));

    int shape[MAXDIM] = {0};
    double value = -1.0;
    void *buffer = &value;
    CHECK_OK(al_read_data(aos_ctx, leaf_field, "", &buffer, DOUBLE_DATA, 0, shape));
    CHECK(buffer == &value);

    CHECK_OK(al_end_action(aos_ctx));
    CHECK_OK(al_end_action(time_slice_ctx));
    CHECK_OK(al_end_action(op_ctx));
    return value;
}

static void close_fixture_pulse(int pulse_ctx) {
    CHECK_OK(al_close_pulse(pulse_ctx, CLOSE_PULSE));
}

/* --- reverse: an HLI declaring 3.39.0 reads the 4.1.1 fixture ------------- */

static void scenario_reverse_reads_renamed_value_through_own_spelling(void) {
    CHECK_OK(imas_mvdd_set_hli_dd_version("3.39.0"));
    int pulse_ctx = open_fixture_pulse("4.1.1");

    /* The 4.1.1 fixture stores this under "beta_tor_norm"; the HLI asks for
     * its own 3.39.0 name, "beta_normal". A plain forward would ask the
     * fixture for a field it does not have. */
    double value = read_scalar_at_slice_zero(pulse_ctx, "global_quantities/beta_normal");
    CHECK(value == 1.8);

    close_fixture_pulse(pulse_ctx);
    printf("equilibrium_read_test reverse-reads-renamed-value-through-own-spelling: "
           "3.39.0 HLI read beta_normal=1.8 from the 4.1.1 fixture's beta_tor_norm\n");
}

/* --- forward: an HLI declaring 4.1.1 reads the 3.39.0 fixture ------------- */

static void scenario_forward_reads_renamed_value_through_own_spelling(void) {
    CHECK_OK(imas_mvdd_set_hli_dd_version("4.1.1"));
    int pulse_ctx = open_fixture_pulse("3.39.0");

    double value = read_scalar_at_slice_zero(pulse_ctx, "global_quantities/beta_tor_norm");
    CHECK(value == 1.8);

    close_fixture_pulse(pulse_ctx);
    printf("equilibrium_read_test forward-reads-renamed-value-through-own-spelling: "
           "4.1.1 HLI read beta_tor_norm=1.8 from the 3.39.0 fixture's beta_normal\n");
}

/* `fold-p2d-bphi` folds DD3's b_field_phi / b_field_tor / b_tor into DD4's
 * single b_field_phi, so it is the one rule with a genuinely different shape
 * per direction: the DD4 HLI below gets an ordered candidate plan over the
 * three DD3 spellings (ADR 0006), while the DD3 HLI in the scenario after it
 * gets one unambiguous destination. Latching 4.1.1 against the 3.39.0 fixture
 * makes this the *forward* direction by this file's labelling, matching the
 * eight scenarios around it. */
static void scenario_forward_merged_read_falls_through_to_stored_alias(void) {
    CHECK_OK(imas_mvdd_set_hli_dd_version("4.1.1"));
    int pulse_ctx = open_fixture_pulse("3.39.0");
    int op_ctx = -1;
    CHECK_OK(al_begin_global_action(pulse_ctx, "equilibrium", "", READ_OP, &op_ctx));
    int time_slice_size = -1, time_slice_ctx = -1;
    CHECK_OK(al_begin_arraystruct_action(op_ctx, "time_slice", "", &time_slice_size, &time_slice_ctx));
    int profiles_size = -1, profiles_ctx = -1;
    CHECK_OK(al_begin_arraystruct_action(time_slice_ctx, "profiles_2d", "", &profiles_size, &profiles_ctx));
    int shape[MAXDIM] = {0};
    void *buffer = NULL;
    CHECK_OK(al_read_data(profiles_ctx, "b_field_phi", "", &buffer, DOUBLE_DATA, 2, shape));
    CHECK(buffer != NULL);
    CHECK(shape[0] == 2 && shape[1] == 3);
    CHECK(((double *)buffer)[0] == 3.1);
    free(buffer);
    CHECK_OK(al_end_action(profiles_ctx));
    CHECK_OK(al_end_action(time_slice_ctx));
    CHECK_OK(al_end_action(op_ctx));
    close_fixture_pulse(pulse_ctx);
}

static void scenario_reverse_split_read_uses_first_destination_and_flips_value(void) {
    CHECK_OK(imas_mvdd_set_hli_dd_version("3.39.0"));
    int pulse_ctx = open_fixture_pulse("4.1.1");

    /* DD4 has both split destinations; precedence chooses psi_axis. Its
     * COCOS-17 fixture value is +0.75, so the DD3 HLI must receive -0.75. */
    double value = read_scalar_at_slice_zero(pulse_ctx, "global_quantities/psi_axis");
    CHECK(value == -0.75);

    close_fixture_pulse(pulse_ctx);
}

static void scenario_forward_split_read_uses_single_source_and_flips_value(void) {
    CHECK_OK(imas_mvdd_set_hli_dd_version("4.1.1"));
    int pulse_ctx = open_fixture_pulse("3.39.0");

    /* DD3 holds the split's single source at -0.75; the DD4 HLI receives
     * the COCOS-17 spelling and therefore +0.75. */
    double value = read_scalar_at_slice_zero(pulse_ctx, "global_quantities/psi_axis");
    CHECK(value == 0.75);

    close_fixture_pulse(pulse_ctx);
}

static void scenario_reverse_merged_read_resolves_single_stored_destination(void) {
    CHECK_OK(imas_mvdd_set_hli_dd_version("3.39.0"));
    int pulse_ctx = open_fixture_pulse("4.1.1");
    int op_ctx = -1;
    CHECK_OK(al_begin_global_action(pulse_ctx, "equilibrium", "", READ_OP, &op_ctx));
    int time_slice_size = -1, time_slice_ctx = -1;
    CHECK_OK(
        al_begin_arraystruct_action(op_ctx, "time_slice", "", &time_slice_size, &time_slice_ctx));
    int profiles_size = -1, profiles_ctx = -1;
    CHECK_OK(
        al_begin_arraystruct_action(time_slice_ctx, "profiles_2d", "", &profiles_size, &profiles_ctx));

    /* `b_tor` is the oldest of the three DD3 spellings the fold collapses, and
     * the 4.1.1 fixture has only the one survivor to serve it from. The value
     * is the same pinned 3.1 the forward scenario reads, which is the point:
     * the merged rule is a path relation, not a value transformation. */
    int shape[MAXDIM] = {0};
    void *buffer = NULL;
    CHECK_OK(al_read_data(profiles_ctx, "b_tor", "", &buffer, DOUBLE_DATA, 2, shape));
    CHECK(buffer != NULL);
    CHECK(shape[0] == 2 && shape[1] == 3);
    CHECK(((double *)buffer)[0] == 3.1);
    free(buffer);

    /* Serving a DD3 spelling from the folded DD4 path cannot prove which of the
     * three the stored value originally was, so the artifact declares this
     * direction lossy — and a merged rule's lossy is ADR 0008's "potentially
     * lossy and unverified" bucket, not its "certainly lossy" one, because the
     * read deliberately does not go looking for evidence either way. The loss
     * lands on the root context even though the read was issued through two
     * nested arraystruct contexts (issue #66). */
    int count = -1;
    CHECK_OK(imas_mvdd_context_loss_count(op_ctx, &count));
    CHECK(count >= 1);
    int found_loss = 0;
    for (int index = 0; index < count; ++index) {
        char path[256] = {0};
        int verdict = -1;
        CHECK_OK(imas_mvdd_context_loss_at(op_ctx, index, path, sizeof path, &verdict));
        if (strcmp(path, "time_slice/profiles_2d/b_tor") == 0 &&
            verdict == IMAS_MVDD_FIDELITY_POTENTIALLY_LOSSY) {
            found_loss = 1;
        }
    }
    /* Core/backend combinations may retain field and timebase outcomes as
     * separate entries; the merged field's loss is the behavior under test. */
    CHECK(found_loss);

    CHECK_OK(al_end_action(profiles_ctx));
    CHECK_OK(al_end_action(time_slice_ctx));
    CHECK_OK(al_end_action(op_ctx));
    close_fixture_pulse(pulse_ctx);
    printf("equilibrium_read_test reverse-merged-read-resolves-single-stored-destination: 3.39.0 "
           "HLI read profiles_2d/b_tor=3.1 from the 4.1.1 fixture's folded b_field_phi, logged "
           "potentially lossy\n");
}

/* --- issue #69: refusal outcomes, in both fixture directions ------------- */

/* Attempts one read the artifact declares unservable and asserts the refusal
 * IMAS-Core never saw: the shim's own status code and message, caller storage
 * left exactly as the caller set it, and an unmappable entry on the context's
 * loss log. `expected_loss_index` is the entry this read is expected to add,
 * so a scenario can make several refusals against one context. */
static void check_read_refused(int op_ctx, const char *field, int datatype, const char *reason,
                               const char *hli_version, const char *stored_version,
                               int expected_loss_index) {
    char expected[MAX_ERR_MSG_LEN];
    int length = snprintf(expected, sizeof expected,
                          "IMAS-MVDD: %s; DD path: %s; HLI DD version: %s; stored DD version: %s",
                          reason, field, hli_version, stored_version);
    CHECK(length > 0 && (size_t)length < sizeof expected);

    /* Deliberate sentinels: a refusal must not write through either of these. */
    void *buffer = (void *)1;
    int shape[MAXDIM] = {73};

    al_status_t status = al_read_data(op_ctx, field, "", &buffer, datatype, 1, shape);

    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(strcmp(status.message, expected) == 0);
    CHECK(buffer == (void *)1);
    CHECK(shape[0] == 73);

    int count = -1;
    CHECK_OK(imas_mvdd_context_loss_count(op_ctx, &count));
    CHECK(count == expected_loss_index + 1);
    char path[256] = {0};
    int verdict = -1;
    CHECK_OK(imas_mvdd_context_loss_at(op_ctx, expected_loss_index, path, sizeof path, &verdict));
    CHECK(strcmp(path, field) == 0);
    CHECK(verdict == IMAS_MVDD_FIDELITY_UNMAPPABLE);
}

/* Each direction asserts both refusals, since neither is direction-specific:
 * that is what distinguishes a refusal the rule genuinely demands from one that
 * merely happens to fall out of whichever direction the resolver was written
 * for first. See the file header for why these two refuse for different
 * reasons. */
static void check_both_refusals(const char *hli_version, const char *fixture_version) {
    CHECK_OK(imas_mvdd_set_hli_dd_version(hli_version));
    int pulse_ctx = open_fixture_pulse(fixture_version);
    int op_ctx = -1;
    CHECK_OK(al_begin_global_action(pulse_ctx, "equilibrium", "", READ_OP, &op_ctx));

    check_read_refused(op_ctx, "time_slice/constraints/strike_point/chi_squared_r", DOUBLE_DATA,
                       "this path's unit was redefined and cannot be converted", hli_version,
                       fixture_version, 0);
    check_read_refused(op_ctx, "grids_ggd/grid/space/coordinates_type", INTEGER_DATA,
                       "this path's container changed shape and cannot be served", hli_version,
                       fixture_version, 1);

    CHECK_OK(al_end_action(op_ctx));
    close_fixture_pulse(pulse_ctx);
}

static void scenario_reverse_refuses_unservable_paths(void) {
    check_both_refusals("3.39.0", "4.1.1");
    printf("equilibrium_read_test reverse-refuses-unservable-paths: a 3.39.0 HLI was refused the "
           "redefined unit and the reshaped container of the 4.1.1 fixture\n");
}

static void scenario_forward_refuses_unservable_paths(void) {
    check_both_refusals("4.1.1", "3.39.0");
    printf("equilibrium_read_test forward-refuses-unservable-paths: a 4.1.1 HLI was refused the "
           "redefined unit and the reshaped container of the 3.39.0 fixture\n");
}

/* --- issue #62: reads beneath a nested, *renamed* child context --------- */

static void scenario_reverse_reads_renamed_nested_container_field(void) {
    CHECK_OK(imas_mvdd_set_hli_dd_version("3.39.0"));
    int pulse_ctx = open_fixture_pulse("4.1.1");

    double value =
        read_nested_constraint_scalar_at_slice_zero(pulse_ctx, "constraints/bpol_probe", "measured");
    CHECK(value == 0.42);

    close_fixture_pulse(pulse_ctx);
    printf("equilibrium_read_test reverse-reads-renamed-nested-container-field: 3.39.0 HLI read "
           "constraints/bpol_probe/measured=0.42 from the 4.1.1 fixture's "
           "constraints/b_field_pol_probe\n");
}

static void scenario_forward_reads_renamed_nested_container_field(void) {
    CHECK_OK(imas_mvdd_set_hli_dd_version("4.1.1"));
    int pulse_ctx = open_fixture_pulse("3.39.0");

    double value = read_nested_constraint_scalar_at_slice_zero(
        pulse_ctx, "constraints/b_field_pol_probe", "measured");
    CHECK(value == 0.42);

    close_fixture_pulse(pulse_ctx);
    printf("equilibrium_read_test forward-reads-renamed-nested-container-field: 4.1.1 HLI read "
           "constraints/b_field_pol_probe/measured=0.42 from the 3.39.0 fixture's "
           "constraints/bpol_probe\n");
}

/* --- issue #62: a supported value transformation nested beneath an ------ */
/* --- unrenamed child context --------------------------------------------- */

static void scenario_reverse_sign_flip_applies_through_nested_container(void) {
    CHECK_OK(imas_mvdd_set_hli_dd_version("3.39.0"));
    int pulse_ctx = open_fixture_pulse("4.1.1");

    /* The 4.1.1 fixture stores flux_loop/measured's COCOS-17 value, -1.15;
     * the 3.39.0 HLI must receive it flipped back to COCOS-11, +1.15. */
    double value = read_nested_constraint_scalar_at_slice_zero(pulse_ctx, "constraints/flux_loop",
                                                                "measured");
    CHECK(value == 1.15);

    close_fixture_pulse(pulse_ctx);
    printf("equilibrium_read_test reverse-sign-flip-applies-through-nested-container: 3.39.0 HLI "
           "read constraints/flux_loop/measured=1.15 from the 4.1.1 fixture's flipped -1.15\n");
}

static void scenario_forward_sign_flip_applies_through_nested_container(void) {
    CHECK_OK(imas_mvdd_set_hli_dd_version("4.1.1"));
    int pulse_ctx = open_fixture_pulse("3.39.0");

    /* The 3.39.0 fixture stores flux_loop/measured's native COCOS-11 value,
     * +1.15; the 4.1.1 HLI must receive it flipped to COCOS-17, -1.15. */
    double value = read_nested_constraint_scalar_at_slice_zero(pulse_ctx, "constraints/flux_loop",
                                                                "measured");
    CHECK(value == -1.15);

    close_fixture_pulse(pulse_ctx);
    printf("equilibrium_read_test forward-sign-flip-applies-through-nested-container: 4.1.1 HLI "
           "read constraints/flux_loop/measured=-1.15 from the 3.39.0 fixture's native 1.15\n");
}

/* --- same-version and conversion-disabled scenarios remain unchanged ----- */

static void scenario_same_version_read_is_unaffected(void) {
    CHECK_OK(imas_mvdd_set_hli_dd_version("4.1.1"));
    int pulse_ctx = open_fixture_pulse("4.1.1");

    double value = read_scalar_at_slice_zero(pulse_ctx, "global_quantities/beta_tor_norm");
    CHECK(value == 1.8);

    close_fixture_pulse(pulse_ctx);
    printf("equilibrium_read_test same-version-read-is-unaffected: a matching-version read "
           "was untouched by conversion wiring\n");
}

static void scenario_conversion_disabled_read_is_unaffected(void) {
    /* No imas_mvdd_set_hli_dd_version call, no IMAS_MVDD_HLI_DD_VERSION: the
     * latch stays unset for this process. */
    int pulse_ctx = open_fixture_pulse("4.1.1");

    double value = read_scalar_at_slice_zero(pulse_ctx, "global_quantities/beta_tor_norm");
    CHECK(value == 1.8);

    close_fixture_pulse(pulse_ctx);
    printf("equilibrium_read_test conversion-disabled-read-is-unaffected: an unset HLI DD "
           "version left the read a plain forward\n");
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr,
                "usage: %s "
                "<reverse-reads-renamed-value-through-own-spelling|"
                "forward-reads-renamed-value-through-own-spelling|"
                "forward-merged-read-falls-through-to-stored-alias|"
                "reverse-merged-read-resolves-single-stored-destination|"
                "reverse-split-read-uses-first-destination-and-flips-value|"
                "forward-split-read-uses-single-source-and-flips-value|"
                "reverse-refuses-unservable-paths|"
                "forward-refuses-unservable-paths|"
                "reverse-reads-renamed-nested-container-field|"
                "forward-reads-renamed-nested-container-field|"
                "reverse-sign-flip-applies-through-nested-container|"
                "forward-sign-flip-applies-through-nested-container|"
                "same-version-read-is-unaffected|"
                "conversion-disabled-read-is-unaffected>\n",
                argv[0]);
        return 2;
    }

    const char *scenario = argv[1];
    if (strcmp(scenario, "reverse-reads-renamed-value-through-own-spelling") == 0) {
        scenario_reverse_reads_renamed_value_through_own_spelling();
    } else if (strcmp(scenario, "forward-reads-renamed-value-through-own-spelling") == 0) {
        scenario_forward_reads_renamed_value_through_own_spelling();
    } else if (strcmp(scenario, "forward-merged-read-falls-through-to-stored-alias") == 0) {
        scenario_forward_merged_read_falls_through_to_stored_alias();
    } else if (strcmp(scenario, "reverse-merged-read-resolves-single-stored-destination") == 0) {
        scenario_reverse_merged_read_resolves_single_stored_destination();
    } else if (strcmp(scenario, "reverse-refuses-unservable-paths") == 0) {
        scenario_reverse_refuses_unservable_paths();
    } else if (strcmp(scenario, "forward-refuses-unservable-paths") == 0) {
        scenario_forward_refuses_unservable_paths();
    } else if (strcmp(scenario, "reverse-split-read-uses-first-destination-and-flips-value") == 0) {
        scenario_reverse_split_read_uses_first_destination_and_flips_value();
    } else if (strcmp(scenario, "forward-split-read-uses-single-source-and-flips-value") == 0) {
        scenario_forward_split_read_uses_single_source_and_flips_value();
    } else if (strcmp(scenario, "reverse-reads-renamed-nested-container-field") == 0) {
        scenario_reverse_reads_renamed_nested_container_field();
    } else if (strcmp(scenario, "forward-reads-renamed-nested-container-field") == 0) {
        scenario_forward_reads_renamed_nested_container_field();
    } else if (strcmp(scenario, "reverse-sign-flip-applies-through-nested-container") == 0) {
        scenario_reverse_sign_flip_applies_through_nested_container();
    } else if (strcmp(scenario, "forward-sign-flip-applies-through-nested-container") == 0) {
        scenario_forward_sign_flip_applies_through_nested_container();
    } else if (strcmp(scenario, "same-version-read-is-unaffected") == 0) {
        scenario_same_version_read_is_unaffected();
    } else if (strcmp(scenario, "conversion-disabled-read-is-unaffected") == 0) {
        scenario_conversion_disabled_read_is_unaffected();
    } else {
        fprintf(stderr, "unknown scenario: %s\n", scenario);
        return 2;
    }

    return 0;
}
