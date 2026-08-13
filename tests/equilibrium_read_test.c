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
 * scenarios are. */

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
static double read_beta_at_slice_zero(int pulse_ctx, const char *field) {
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
    double value = read_beta_at_slice_zero(pulse_ctx, "global_quantities/beta_normal");
    CHECK(value == 1.8);

    close_fixture_pulse(pulse_ctx);
    printf("equilibrium_read_test reverse-reads-renamed-value-through-own-spelling: "
           "3.39.0 HLI read beta_normal=1.8 from the 4.1.1 fixture's beta_tor_norm\n");
}

/* --- forward: an HLI declaring 4.1.1 reads the 3.39.0 fixture ------------- */

static void scenario_forward_reads_renamed_value_through_own_spelling(void) {
    CHECK_OK(imas_mvdd_set_hli_dd_version("4.1.1"));
    int pulse_ctx = open_fixture_pulse("3.39.0");

    double value = read_beta_at_slice_zero(pulse_ctx, "global_quantities/beta_tor_norm");
    CHECK(value == 1.8);

    close_fixture_pulse(pulse_ctx);
    printf("equilibrium_read_test forward-reads-renamed-value-through-own-spelling: "
           "4.1.1 HLI read beta_tor_norm=1.8 from the 3.39.0 fixture's beta_normal\n");
}

/* --- same-version and conversion-disabled scenarios remain unchanged ----- */

static void scenario_same_version_read_is_unaffected(void) {
    CHECK_OK(imas_mvdd_set_hli_dd_version("4.1.1"));
    int pulse_ctx = open_fixture_pulse("4.1.1");

    double value = read_beta_at_slice_zero(pulse_ctx, "global_quantities/beta_tor_norm");
    CHECK(value == 1.8);

    close_fixture_pulse(pulse_ctx);
    printf("equilibrium_read_test same-version-read-is-unaffected: a matching-version read "
           "was untouched by conversion wiring\n");
}

static void scenario_conversion_disabled_read_is_unaffected(void) {
    /* No imas_mvdd_set_hli_dd_version call, no IMAS_MVDD_HLI_DD_VERSION: the
     * latch stays unset for this process. */
    int pulse_ctx = open_fixture_pulse("4.1.1");

    double value = read_beta_at_slice_zero(pulse_ctx, "global_quantities/beta_tor_norm");
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
