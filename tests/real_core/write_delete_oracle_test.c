/* Issue #133: the write and delete paths' correctness claims are on-disk
 * claims. ADR 0016's own "Consequences" section states why a shim round trip
 * cannot make them: a write flips HLI-to-stored and a read flips
 * stored-to-HLI, so the caller's own value comes back whichever sign, spelling
 * or fan-out actually reached disk. A claim provable here is instead proven
 * by reading the *copied* fixture directly with HDF5 — never through the
 * shim or IMAS-Core — after the shim has been asked to mutate it.
 * tests/support/real_core_fixture_support.h (issue #132) supplies the
 * byte-copy helpers; the stamp-reading helper below is private to this file,
 * the same boundary equilibrium_read_test.c already draws.
 *
 * Only one of the five claims in #133's "What to build" is provable against
 * this real IMAS-Core version, and the reason is a discovery this file's
 * development turned up rather than a design choice:
 *
 *   IMAS-Core's HDF5 backend (`HDF5EventsHandler::beginAction`,
 *   `build/_deps/al-core-src/src/hdf5/hdf5_events_handler.cpp`) initializes
 *   the *reader's* per-IDS HDF5 group only when a context is opened
 *   `READ_OP`; a `WRITE_OP`-opened context (`al_begin_global_action` or
 *   `al_begin_slice_action`, `GLOBAL_OP` or `SLICE_OP` rangemode alike) only
 *   initializes the *writer's* group. Stored-DD-version discovery
 *   (`version_stamp::discover`, `src/version/version_stamp.rs`) reads the
 *   stamp through the *same* ctx_id a seam has just opened
 *   (`src/interpose.rs`'s `open_occurrence`), via an unconverted
 *   `al_read_data`. Through a `WRITE_OP`-opened context that read has no
 *   reader-side group to read from, so it comes back `size == 0` — not an
 *   error, just "not found" — and `version_stamp::discover` treats that
 *   exactly like a genuinely unstamped occurrence (`StampOutcome::Unstamped`,
 *   `ReadOutcome::NotFound` arm), so `decide_occurrence_registration`
 *   registers nothing. Every write or delete through that context is then a
 *   plain forward: no path translation, no refusal, no loss log — confirmed
 *   empirically (see the discovery scenario below) for both
 *   `al_begin_global_action(..., WRITE_OP, ...)` and
 *   `al_begin_slice_action(..., WRITE_OP, ...)`.
 *
 *   The reverse also holds and closes off the obvious workaround: a write
 *   issued through a `READ_OP`-opened context (which *does* discover and
 *   register correctly, and does translate/refuse) fails with a real Core
 *   exception — `HDF5Backend: unexpected value for gid in
 *   HDF5Writer::write_ND_Data()` — because the *writer's* group was never
 *   initialized for a `READ_OP` context. Opening twice (`READ_OP` to
 *   discover, then `WRITE_OP` to write) does not help either:
 *   `decide_occurrence_registration`'s `StampOutcome::Unstamped` arm not only
 *   registers nothing for the new context, it actively forgets the
 *   occurrence cache (`OccurrenceCacheEffect::Forget`) the first open built.
 *
 *   So on this real IMAS-Core version, there is no single ABI call sequence
 *   that both discovers a version mismatch on an existing occurrence *and*
 *   successfully writes to it — the write-path conversion policy ADR 0016
 *   specifies, and which `write_delete_conversion_test.c`'s stub suite
 *   thoroughly proves at the policy level, is unreachable from a real
 *   `WRITE_OP` caller. Issue #136 tracks this as its own defect, since fixing
 *   it is a shim change (`src/interpose.rs`'s discovery needs to probe the
 *   stamp some other way when the caller's own context cannot read it), not
 *   a test change, and is out of #133's scope.
 *
 *   Of the five claims, only the fifth is unaffected: "a refused write
 *   leaves no trace on disk" is decided entirely from registry state before
 *   any write reaches Core, so it needs a context real Core can actually
 *   register — `READ_OP` — not the `WRITE_OP` a literal reading of the claim
 *   might suggest. Claims 1-4 all require a *successful, translating* write
 *   or delete to actually reach Core, which is exactly what issue #136 must
 *   fix before those four can be attempted.
 *   `scenario_write_op_root_does_not_register_a_mismatch` below pins today's
 *   behavior as a regression marker: once #136 lands, this scenario's own
 *   assertions flip, which is the signal to replace it with claims 1-4. */

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

/* --- copied-fixture management (issue #132 harness, reproduced per-file --- */
/* --- the same way equilibrium_read_test.c's own copy already is)      --- */

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
                               "/tmp/imas-mvdd-write-delete-oracle-XXXXXX");
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

static void equilibrium_file_path(char *path, size_t path_size) {
    int length = snprintf(path, path_size, "%s/equilibrium.h5", copied_fixture.pulse_dir);
    CHECK(length > 0 && (size_t)length < path_size);
}

/* --- raw HDF5 access to the copied fixture, never through the shim ------ */

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

/* --- shim access to the copied fixture ----------------------------------- */

static int open_copied_fixture_pulse(void) {
    char uri[1024];
    int length = snprintf(uri, sizeof uri, "imas:hdf5?path=%s", copied_fixture.pulse_dir);
    CHECK(length > 0 && (size_t)length < sizeof uri);
    int pulse_ctx = -1;
    CHECK_OK(al_begin_dataentry_action(uri, OPEN_PULSE, &pulse_ctx));
    return pulse_ctx;
}

static void close_fixture_pulse(int pulse_ctx) {
    CHECK_OK(al_close_pulse(pulse_ctx, CLOSE_PULSE));
}

/* --- claim 5: a refused write leaves no trace on disk -------------------- */

/* Opens READ_OP, not WRITE_OP. See the file header: this real IMAS-Core
 * version's HDF5 backend only initializes the reader's per-IDS group under
 * READ_OP, and stamp discovery's internal read silently reports "not found"
 * through a WRITE_OP-opened context — so a WRITE_OP open of this same
 * mismatched occurrence never registers a conversion record at all, and the
 * refusal this claim is about never fires. The refusal itself needs no Core
 * access — it is decided entirely from registry state before any write
 * reaches Core — so proving it needs only a context real Core can actually
 * register, which READ_OP is. */
static void check_refused_write_leaves_stamp_untouched(const char *hli_version,
                                                        const char *fixture_version) {
    copy_fixture_pair(fixture_version);
    CHECK_OK(imas_mvdd_set_hli_dd_version(hli_version));
    int pulse_ctx = open_copied_fixture_pulse();

    int op_ctx = -1;
    CHECK_OK(al_begin_global_action(pulse_ctx, "equilibrium", "", READ_OP, &op_ctx));

    const char *field = "ids_properties/version_put/data_dictionary";
    double sentinel = 42.0;
    al_status_t status = al_write_data(op_ctx, field, "", &sentinel, DOUBLE_DATA, 0, NULL);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status, "the DD-version stamp is immutable under a version mismatch",
                          field, hli_version, fixture_version);
    CHECK(sentinel == 42.0);

    CHECK_OK(al_end_action(op_ctx));
    close_fixture_pulse(pulse_ctx);

    char equilibrium_file[1024];
    equilibrium_file_path(equilibrium_file, sizeof equilibrium_file);
    char stamp[64];
    read_dd_version_stamp_from_disk(equilibrium_file, stamp, sizeof stamp);
    CHECK(strcmp(stamp, fixture_version) == 0);

    remove_fixture_pair();
}

static void scenario_forward_refused_write_leaves_stamp_untouched(void) {
    check_refused_write_leaves_stamp_untouched("4.1.1", "3.39.0");
    printf("write_delete_oracle_test forward-refused-write-leaves-stamp-untouched: a refused "
           "4.1.1 stamp write left the 3.39.0 fixture's stamp exactly as it was\n");
}

static void scenario_reverse_refused_write_leaves_stamp_untouched(void) {
    check_refused_write_leaves_stamp_untouched("3.39.0", "4.1.1");
    printf("write_delete_oracle_test reverse-refused-write-leaves-stamp-untouched: a refused "
           "3.39.0 stamp write left the 4.1.1 fixture's stamp exactly as it was\n");
}

/* --- regression marker for the discovery in this file's header --------- */

/* Pins today's real-Core behavior: a WRITE_OP-opened, genuinely mismatched
 * occurrence registers no conversion record, so the very refusal claim 5
 * proves under READ_OP does *not* fire under WRITE_OP — the write instead
 * reaches Core forwarding the HLI's own untranslated path, which then fails
 * with a real backend error because the stamp dataset already exists
 * (`HDF5DataSetHandler::create` calls a bare `H5Dcreate2` with no existence
 * check). If issue #136 fixes discovery under WRITE_OP, this scenario starts
 * failing — which is the signal to delete it and implement claims 1-4. */
static void scenario_write_op_root_does_not_register_a_mismatch(void) {
    copy_fixture_pair("3.39.0");
    CHECK_OK(imas_mvdd_set_hli_dd_version("4.1.1"));
    int pulse_ctx = open_copied_fixture_pulse();

    int op_ctx = -1;
    CHECK_OK(al_begin_global_action(pulse_ctx, "equilibrium", "", WRITE_OP, &op_ctx));

    double sentinel = 42.0;
    al_status_t status = al_write_data(
        op_ctx, "ids_properties/version_put/data_dictionary", "", &sentinel, DOUBLE_DATA, 0, NULL);

    /* Today: the shim's own refusal (IMAS_MVDD_CONVERSION_ERROR) never fires;
     * Core's own backend exception surfaces instead. */
    CHECK(status.code != IMAS_MVDD_CONVERSION_ERROR);
    CHECK(status.code != 0);

    CHECK_OK(al_end_action(op_ctx));
    close_fixture_pulse(pulse_ctx);
    printf("write_delete_oracle_test write-op-root-does-not-register-a-mismatch: pinned known "
           "gap (issue #136) — a WRITE_OP-opened mismatched occurrence still forwards "
           "unconverted on real Core\n");
}

int main(int argc, char **argv) {
    static const shim_test_scenario scenarios[] = {
        {"forward-refused-write-leaves-stamp-untouched",
         scenario_forward_refused_write_leaves_stamp_untouched},
        {"reverse-refused-write-leaves-stamp-untouched",
         scenario_reverse_refused_write_leaves_stamp_untouched},
        {"write-op-root-does-not-register-a-mismatch",
         scenario_write_op_root_does_not_register_a_mismatch},
    };
    return RUN_NAMED_SCENARIO(argc, argv, scenarios);
}
