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
#include <sys/stat.h>
#include <unistd.h>

#include <al_const.h>
#include <hdf5.h>
#include "../support/real_core_fixture_support.h"

#ifndef EQUILIBRIUM_FIXTURE_DIR
#error "EQUILIBRIUM_FIXTURE_DIR must name the imas-python-fixtures/fixtures directory"
#endif

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

/* The HDF5 backend flattens this one AOS component as `time_slice[]` and
 * joins the remaining DD path components with `&`. */
static void time_slice_dataset_path(const char *dd_path, char *dataset, size_t dataset_size) {
    int length = snprintf(dataset, dataset_size, "/equilibrium/time_slice[]&%s", dd_path);
    CHECK(length > 0 && (size_t)length < dataset_size);
    for (char *component = dataset + strlen("/equilibrium/time_slice[]&"); *component != '\0';
         ++component) {
        if (*component == '/') {
            *component = '&';
        }
    }
}

/* Read one numeric value directly from the copied fixture. This accepts a DD
 * path relative to `time_slice`, not an HDF5 object name, and validates the
 * fixture's expected one-dimensional double representation. */
static double read_time_slice_double_from_disk(const char *ids_file, const char *dd_path,
                                               hsize_t slice) {
    char dataset_path[1024];
    time_slice_dataset_path(dd_path, dataset_path, sizeof dataset_path);
    hid_t file = H5Fopen(ids_file, H5F_ACC_RDONLY, H5P_DEFAULT);
    CHECK(file >= 0);
    hid_t dataset = H5Dopen2(file, dataset_path, H5P_DEFAULT);
    CHECK(dataset >= 0);
    hid_t datatype = H5Dget_type(dataset);
    CHECK(datatype >= 0);
    CHECK(H5Tget_class(datatype) == H5T_FLOAT);
    hid_t dataspace = H5Dget_space(dataset);
    CHECK(dataspace >= 0);
    CHECK(H5Sget_simple_extent_ndims(dataspace) == 1);
    hsize_t length = 0;
    CHECK(H5Sget_simple_extent_dims(dataspace, &length, NULL) == 1);
    CHECK(slice < length);
    double *values = malloc((size_t)length * sizeof *values);
    CHECK(values != NULL);
    CHECK(H5Dread(dataset, H5T_NATIVE_DOUBLE, H5S_ALL, H5S_ALL, H5P_DEFAULT, values) >= 0);
    double value = values[slice];
    free(values);
    CHECK(H5Sclose(dataspace) >= 0);
    CHECK(H5Tclose(datatype) >= 0);
    CHECK(H5Dclose(dataset) >= 0);
    CHECK(H5Fclose(file) >= 0);
    return value;
}

/* The DD-version stamp is a scalar UTF-8 variable-length string. The caller
 * owns only its fixed output buffer; HDF5 allocates and releases `stored`. */
static void read_dd_version_stamp_from_disk(const char *ids_file, char *version,
                                            size_t version_size) {
    hid_t file = H5Fopen(ids_file, H5F_ACC_RDONLY, H5P_DEFAULT);
    CHECK(file >= 0);
    hid_t dataset = H5Dopen2(file, "/equilibrium/ids_properties&version_put&data_dictionary",
                             H5P_DEFAULT);
    CHECK(dataset >= 0);
    hid_t datatype = H5Dget_type(dataset);
    CHECK(datatype >= 0);
    CHECK(H5Tget_class(datatype) == H5T_STRING);
    CHECK(H5Tis_variable_str(datatype) > 0);
    char *stored = NULL;
    CHECK(H5Dread(dataset, datatype, H5S_ALL, H5S_ALL, H5P_DEFAULT, &stored) >= 0);
    CHECK(stored != NULL);
    int length = snprintf(version, version_size, "%s", stored);
    CHECK(length >= 0 && (size_t)length < version_size);
    CHECK(H5free_memory(stored) >= 0);
    CHECK(H5Tclose(datatype) >= 0);
    CHECK(H5Dclose(dataset) >= 0);
    CHECK(H5Fclose(file) >= 0);
}

typedef struct {
    char temp_dir[1024];
    char pulse_dir[1024];
    int is_live;
} fixture_copy;

static fixture_copy copied_fixture;

static void remove_fixture_pair(void) {
    if (!copied_fixture.is_live) {
        return;
    }
    if (copied_fixture.pulse_dir[0] != '\0') {
        remove_fixture_file(copied_fixture.pulse_dir, "equilibrium.h5");
        remove_fixture_file(copied_fixture.pulse_dir, "master.h5");
        CHECK(rmdir(copied_fixture.pulse_dir) == 0 || errno == ENOENT);
    }
    CHECK(rmdir(copied_fixture.temp_dir) == 0 || errno == ENOENT);
    copied_fixture.is_live = 0;
}

static void copy_fixture_pair(const char *dd_version) {
    int temp_length = snprintf(copied_fixture.temp_dir, sizeof copied_fixture.temp_dir,
                               "/tmp/imas-mvdd-equilibrium-XXXXXX");
    CHECK(temp_length > 0 && (size_t)temp_length < sizeof copied_fixture.temp_dir);
    CHECK(mkdtemp(copied_fixture.temp_dir) != NULL);
    copied_fixture.is_live = 1;
    CHECK(atexit(remove_fixture_pair) == 0);
    int pulse_length = snprintf(copied_fixture.pulse_dir, sizeof copied_fixture.pulse_dir,
                                "%s/dd-%s", copied_fixture.temp_dir, dd_version);
    CHECK(pulse_length > 0 && (size_t)pulse_length < sizeof copied_fixture.pulse_dir);
    CHECK(mkdir(copied_fixture.pulse_dir, 0700) == 0);
    static const char *const files[] = {"equilibrium.h5", "master.h5"};
    for (size_t i = 0; i < sizeof files / sizeof files[0]; ++i) {
        char source[1024];
        char copy[1024];
        int source_length = snprintf(source, sizeof source, "%s/dd-%s/%s", EQUILIBRIUM_FIXTURE_DIR,
                                     dd_version, files[i]);
        int copy_length = snprintf(copy, sizeof copy, "%s/%s", copied_fixture.pulse_dir, files[i]);
        CHECK(source_length > 0 && (size_t)source_length < sizeof source);
        CHECK(copy_length > 0 && (size_t)copy_length < sizeof copy);
        copy_fixture_file(source, copy);
    }
}

/* Issue #132: prove the mutable-fixture harness against an existing read
 * claim. The source fixture is copied byte-for-byte and never opened through
 * HDF5 or IMAS-Core; all raw and ABI access targets the private copy. */
static void scenario_copied_fixture_harness_reproves_renamed_read(void) {
    copy_fixture_pair("3.39.0");
    char equilibrium_file[1024];
    int file_length = snprintf(equilibrium_file, sizeof equilibrium_file, "%s/equilibrium.h5",
                               copied_fixture.pulse_dir);
    CHECK(file_length > 0 && (size_t)file_length < sizeof equilibrium_file);
    char stamp[64];
    read_dd_version_stamp_from_disk(equilibrium_file, stamp, sizeof stamp);
    CHECK(strcmp(stamp, "3.39.0") == 0);
    CHECK(read_time_slice_double_from_disk(equilibrium_file, "global_quantities/beta_normal", 0)
          == 1.8);
    CHECK_OK(imas_mvdd_set_hli_dd_version("4.1.1"));
    char uri[1024];
    int uri_length = snprintf(uri, sizeof uri, "imas:hdf5?path=%s", copied_fixture.pulse_dir);
    CHECK(uri_length > 0 && (size_t)uri_length < sizeof uri);
    int pulse_ctx = -1;
    CHECK_OK(al_begin_dataentry_action(uri, OPEN_PULSE, &pulse_ctx));
    CHECK(read_scalar_at_slice_zero(pulse_ctx, "global_quantities/beta_tor_norm") == 1.8);
    close_fixture_pulse(pulse_ctx);
    remove_fixture_pair();
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
     * nested arraystruct contexts (issue #66).
     *
     * The count is pinned, not bounded. The loss log is wholly shim-owned (ADR
     * 0003, ADR 0012): IMAS-Core cannot add, drop or reorder an entry, so no
     * Core or backend combination can change it. This read passes "" as its
     * timebase, and `retain_read_fidelity` skips an empty argument before the
     * log is touched, so the field is the only thing that can be retained; and
     * one attempt returning data retains exactly once. Exactly one entry is
     * therefore the only correct answer on any platform.
     *
     * A count of 2 means an entry for a read this caller never issued - a
     * reentrant read through the shim retaining its own loss - which is a
     * defect in the shim, not platform noise to absorb. The dump below names
     * the extra entry so a failure here diagnoses itself instead of leaving
     * the next reader to re-derive it.
     *
     * That is exactly what this assertion caught. On Linux real-Core (CI run
     * 32046056999) the dump reported, in order:
     *   [0] time_slice/profiles_2d/b_field_phi (POTENTIALLY_LOSSY)
     *   [1] time_slice/profiles_2d/b_tor       (POTENTIALLY_LOSSY)
     * [1] is this read; [0] was keyed on the *stored* spelling this read had
     * just translated `b_tor` into, and was logged first - from inside the
     * outer read. IMAS-Core's internal call to its own public `al_read_data`
     * binds to the shim's exported definition on ELF but not under macOS's
     * two-level namespace, so the same read converted twice on Linux only.
     * ADR 0014 fixed that: a read entered while one is already in flight is
     * forwarded untouched. `read-path-reentrant-*` now covers the policy on
     * every platform, so this assertion is the end-to-end witness rather than
     * the only one. */
    int count = -1;
    CHECK_OK(imas_mvdd_context_loss_count(op_ctx, &count));
    if (count != 1) {
        fprintf(stderr, "root loss log holds %d entries, expected exactly 1:\n", count);
        for (int index = 0; index < count; ++index) {
            char entry[256] = {0};
            int entry_verdict = -1;
            CHECK_OK(imas_mvdd_context_loss_at(op_ctx, index, entry, sizeof entry, &entry_verdict));
            fprintf(stderr, "  [%d] %s (verdict %d)\n", index, entry, entry_verdict);
        }
    }
    CHECK(count == 1);
    char path[256] = {0};
    int verdict = -1;
    CHECK_OK(imas_mvdd_context_loss_at(op_ctx, 0, path, sizeof path, &verdict));
    CHECK(strcmp(path, "time_slice/profiles_2d/b_tor") == 0);
    CHECK(verdict == IMAS_MVDD_FIDELITY_POTENTIALLY_LOSSY);

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

/* --- Issue #125: the remaining real-Core refusal boundary ------------------ */

/* Safe writes are proven at the recording-stub boundary, where the exact
 * stored spelling delivered to IMAS-Core is observable. The checked-in HDF5
 * fixture is opened only for reads, so its real-Core probe keeps the one
 * remaining mismatched mutation policy: delete must still refuse. */
static void scenario_forward_delete_refuses_against_mismatch(void) {
    CHECK_OK(imas_mvdd_set_hli_dd_version("4.1.1"));
    int pulse_ctx = open_fixture_pulse("3.39.0");

    int op_ctx = -1;
    CHECK_OK(al_begin_global_action(pulse_ctx, "equilibrium", "", READ_OP, &op_ctx));

    const char *field = "time_slice/global_quantities/beta_tor_norm";

    al_status_t delete_status = al_delete_data(op_ctx, field);
    CHECK(delete_status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(delete_status,
                          "al_delete_data refuses on a context with a known DD version mismatch",
                          field, "4.1.1", "3.39.0");

    /* The same context still converts a read, so the refusal was policy,
     * rather than an unusable real-Core context. */
    int slice_size = -1;
    int aos_ctx = -1;
    CHECK_OK(al_begin_arraystruct_action(op_ctx, "time_slice", "", &slice_size, &aos_ctx));
    CHECK(slice_size == 2);
    int shape[MAXDIM] = {0};
    double value = -1.0;
    void *buffer = &value;
    CHECK_OK(al_read_data(aos_ctx, "global_quantities/beta_tor_norm", "", &buffer, DOUBLE_DATA, 0,
                         shape));
    CHECK(value == 1.8);

    CHECK_OK(al_end_action(aos_ctx));
    CHECK_OK(al_end_action(op_ctx));
    close_fixture_pulse(pulse_ctx);
    printf("equilibrium_read_test forward-delete-refuses-against-mismatch: real Core kept delete "
           "refused while the same mismatched context continued to convert reads\n");
}

/* Obligation (g): "non-LIFO context closure, recycled context IDs,
 * al_close_pulse leaving the registry untouched, and AoS iteration requiring no
 * registry update." tests/context_lifecycle_test.c proves these against the
 * recording stub. There is no C-level registry introspection, so here as there
 * the only externally observable consequence is whether a later read through a
 * still-live context still translates — but here the whole lifecycle is a real
 * IMAS-Core one.
 *
 * Ending the parent operation context before its live child is left to the stub
 * suite deliberately: real IMAS-Core owns that ordering, and a test asserting
 * the shim's registry behaviour must not also depend on Core tolerating an
 * ordering the HLI contract does not promise. What is proven here is every part
 * of (g) that a legal real-Core lifecycle can reach. */
static void scenario_forward_context_lifecycle_keeps_conversion_live(void) {
    CHECK_OK(imas_mvdd_set_hli_dd_version("4.1.1"));
    int pulse_ctx = open_fixture_pulse("3.39.0");

    int op_ctx = -1;
    CHECK_OK(al_begin_global_action(pulse_ctx, "equilibrium", "", READ_OP, &op_ctx));
    int slice_size = -1;
    int aos_ctx = -1;
    CHECK_OK(al_begin_arraystruct_action(op_ctx, "time_slice", "", &slice_size, &aos_ctx));
    CHECK(slice_size == 2);

    int shape[MAXDIM] = {0};
    double value = -1.0;
    void *buffer = &value;

    /* AoS iteration keeps no registry state: the child still translates after
     * stepping, and the second slice's own value comes back. */
    CHECK_OK(al_iterate_over_arraystruct(aos_ctx, 1));
    value = -1.0;
    buffer = &value;
    CHECK_OK(al_read_data(aos_ctx, "global_quantities/beta_tor_norm", "", &buffer, DOUBLE_DATA, 0,
                         shape));
    /* Written as the sum the fixture generator computes (1.8 + 0.1*i), not as
     * the literal 1.9: in IEEE double 1.8 + 0.1 is not the double nearest 1.9,
     * so the literal would fail against a value that is exactly right. Slice 0
     * needs no such care, which is why every scenario above compares to 1.8. */
    CHECK(value == 1.8 + 0.1);

    /* Ending the child leaves the parent's own record live, so a read through
     * the parent still translates. */
    CHECK_OK(al_end_action(aos_ctx));
    int reopened_size = -1;
    int reopened_ctx = -1;
    CHECK_OK(al_begin_arraystruct_action(op_ctx, "time_slice", "", &reopened_size, &reopened_ctx));
    CHECK(reopened_size == 2);
    value = -1.0;
    buffer = &value;
    CHECK_OK(al_read_data(reopened_ctx, "global_quantities/beta_tor_norm", "", &buffer, DOUBLE_DATA,
                         0, shape));
    CHECK(value == 1.8);
    CHECK_OK(al_end_action(reopened_ctx));
    CHECK_OK(al_end_action(op_ctx));

    /* al_close_pulse releases no context ID and must not clear the pulse's
     * discovered-version cache: reopening the same occurrence still converts,
     * which it could not do if closing had lost the stored version. */
    close_fixture_pulse(pulse_ctx);
    int reopened_pulse = open_fixture_pulse("3.39.0");
    int second_op_ctx = -1;
    CHECK_OK(al_begin_global_action(reopened_pulse, "equilibrium", "", READ_OP, &second_op_ctx));
    int second_size = -1;
    int second_aos_ctx = -1;
    CHECK_OK(
        al_begin_arraystruct_action(second_op_ctx, "time_slice", "", &second_size, &second_aos_ctx));
    value = -1.0;
    buffer = &value;
    CHECK_OK(al_read_data(second_aos_ctx, "global_quantities/beta_tor_norm", "", &buffer,
                         DOUBLE_DATA, 0, shape));
    CHECK(value == 1.8);
    CHECK_OK(al_end_action(second_aos_ctx));
    CHECK_OK(al_end_action(second_op_ctx));
    close_fixture_pulse(reopened_pulse);

    printf("equilibrium_read_test forward-context-lifecycle-keeps-conversion-live: real-Core AoS "
           "iteration, child close and pulse close each left conversion working through the "
           "contexts that were still open\n");
}

int main(int argc, char **argv) {
    static const shim_test_scenario scenarios[] = {
        {"reverse-reads-renamed-value-through-own-spelling", scenario_reverse_reads_renamed_value_through_own_spelling},
        {"forward-reads-renamed-value-through-own-spelling", scenario_forward_reads_renamed_value_through_own_spelling},
        {"forward-merged-read-falls-through-to-stored-alias", scenario_forward_merged_read_falls_through_to_stored_alias},
        {"reverse-merged-read-resolves-single-stored-destination", scenario_reverse_merged_read_resolves_single_stored_destination},
        {"reverse-refuses-unservable-paths", scenario_reverse_refuses_unservable_paths},
        {"forward-refuses-unservable-paths", scenario_forward_refuses_unservable_paths},
        {"reverse-split-read-uses-first-destination-and-flips-value", scenario_reverse_split_read_uses_first_destination_and_flips_value},
        {"forward-split-read-uses-single-source-and-flips-value", scenario_forward_split_read_uses_single_source_and_flips_value},
        {"reverse-reads-renamed-nested-container-field", scenario_reverse_reads_renamed_nested_container_field},
        {"forward-reads-renamed-nested-container-field", scenario_forward_reads_renamed_nested_container_field},
        {"reverse-sign-flip-applies-through-nested-container", scenario_reverse_sign_flip_applies_through_nested_container},
        {"forward-sign-flip-applies-through-nested-container", scenario_forward_sign_flip_applies_through_nested_container},
        {"same-version-read-is-unaffected", scenario_same_version_read_is_unaffected},
        {"conversion-disabled-read-is-unaffected", scenario_conversion_disabled_read_is_unaffected},
        {"forward-delete-refuses-against-mismatch", scenario_forward_delete_refuses_against_mismatch},
        {"forward-context-lifecycle-keeps-conversion-live", scenario_forward_context_lifecycle_keeps_conversion_live},
        {"copied-fixture-harness-reproves-renamed-read",
         scenario_copied_fixture_harness_reproves_renamed_read},
    };
    return RUN_NAMED_SCENARIO(argc, argv, scenarios);
}
