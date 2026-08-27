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
 * Issue #136 is what made four of #133's five claims reachable at all. When
 * this file was first written, a `WRITE_OP`-opened context never registered a
 * conversion record against real IMAS-Core, because stored-DD-version
 * discovery read the stamp through the caller's own context and real Core's
 * HDF5 backend initializes the *reader's* per-IDS group only under `READ_OP`.
 * ADR 0020 records the fix: discovery now probes the stamp through a
 * shim-owned read-mode context of its own, opened and closed before the
 * caller's own open, whenever the caller's access mode is not `READ_OP`.
 * Every scenario below that translates anything is downstream of that, and
 * `scenario_write_op_root_does_not_register_a_mismatch`, which pinned the gap,
 * is gone because its assertions flipped.
 *
 * Two constraints real IMAS-Core imposes on this file, neither of them the
 * shim's doing, both established empirically:
 *
 *   - **Every successful write here is a `put_slice`.** A `WRITE_OP` *global*
 *     action takes `HDF5Writer::write_ND_Data`'s non-slice branch, whose
 *     `create()` is a bare `H5Dcreate2` with no existence check, so it fails
 *     on every dataset the fixture already holds. The `SLICE_OP` branch checks
 *     `H5Lexists` and opens instead. That is also exactly ADR 0016's declared
 *     scope: the append or partial write into an existing, differently-stamped
 *     occurrence.
 *   - **The appended slice carries no `time` value.** Real IMAS-Core
 *     segfaults inside `al_write_data(op_ctx, "time", ...)` through a
 *     slice-mode operation context. It does so with matching DD versions and
 *     with the shim doing nothing at all, so it is IMAS-Core's crash; every
 *     claim below is about one leaf dataset's contents, which a slice without
 *     its time coordinate still shows.
 *
 * Of #133's five claims, four are proven below and one is not. Claim 4 has
 * two halves — the precedence-2 candidate is left alone by a write and removed
 * by a delete — and only the write half is observable on this backend. The
 * delete half had two independent obstacles. Issue #138 removed the first by
 * dropping the presence probe, so a fan-out now reaches IMAS-Core at all; the
 * second stands, because this backend's `deleteData` ignores its path and
 * removes the whole occurrence (issue #139), leaving no per-candidate effect
 * to observe. `scenario_delete_reverse_fan_out_reaches_disk` asserts what is
 * observable today: the fan-out is no longer a silent successful no-op.
 *
 * Direction labels follow equilibrium_read_test.c's convention, which names
 * the *fixture* under test rather than `conversion_map::Direction`: `forward`
 * is the 3.39.0 fixture (read or written by a 4.1.1 HLI) and `reverse` is the
 * 4.1.1 fixture (by a 3.39.0 HLI). */

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

/* Both fixture pulses hold two equilibrium time slices — the same 2 that
 * equilibrium_read_test.c asserts `al_begin_arraystruct_action` reports — so a
 * successful append is observable as a third. */
#define FIXTURE_SLICES 2
#define FIXTURE_SLICE_CAPACITY 16

/* Later than either fixture slice (TIME is 1.0 and 1.1), so an append is an
 * append rather than a replace. */
#define APPENDED_SLICE_TIME 3.0

/* HDF5 dataset paths, in the backend's own tensorized `&`-separated spelling.
 * Each names the *stored* side of one artifact rule this file exercises. */
#define BETA_NORMAL_DATASET "/equilibrium/time_slice[]&global_quantities&beta_normal"
#define BETA_TOR_NORM_DATASET "/equilibrium/time_slice[]&global_quantities&beta_tor_norm"
#define IP_DATASET "/equilibrium/time_slice[]&global_quantities&ip"
#define ENERGY_MHD_DATASET "/equilibrium/time_slice[]&global_quantities&energy_mhd"
#define W_MHD_DATASET "/equilibrium/time_slice[]&global_quantities&w_mhd"
#define PSI_AXIS_DATASET "/equilibrium/time_slice[]&global_quantities&psi_axis"
#define Q_MIN_PSI_DATASET "/equilibrium/time_slice[]&global_quantities&q_min&psi"
#define Q_MIN_VALUE_DATASET "/equilibrium/time_slice[]&global_quantities&q_min&value"
#define TIME_SLICE_AOS_SHAPE_DATASET "/equilibrium/time_slice[]&AOS_SHAPE"
#define PSI_MAGNETIC_AXIS_DATASET "/equilibrium/time_slice[]&global_quantities&psi_magnetic_axis"

/* --- copied-fixture management (issue #132 shared harness) -------------- */

static real_core_fixture_copy copied_fixture;

static void remove_fixture_pair(void) {
    remove_real_core_fixture_copy(&copied_fixture);
}

static void copy_fixture_pair(const char *dd_version) {
    create_real_core_fixture_copy(&copied_fixture, EQUILIBRIUM_FIXTURE_DIR, dd_version,
                                  "/tmp/imas-mvdd-write-delete-oracle-XXXXXX");
    CHECK(atexit(remove_fixture_pair) == 0);
}

static void equilibrium_file_path(char *path, size_t path_size) {
    real_core_fixture_ids_file(&copied_fixture, path, path_size);
}

/* --- raw HDF5 access to the copied fixture, never through the shim ------ */

static int dataset_exists_on_disk(const char *ids_file, const char *dataset_path) {
    hid_t file = H5Fopen(ids_file, H5F_ACC_RDONLY, H5P_DEFAULT);
    CHECK(file >= 0);
    htri_t exists = H5Lexists(file, dataset_path, H5P_DEFAULT);
    CHECK(exists >= 0);
    CHECK(H5Fclose(file) >= 0);
    return exists > 0;
}

static int read_double_slices_from_disk(const char *ids_file, const char *dataset_path,
                                       double *values, int capacity) {
    hid_t file = H5Fopen(ids_file, H5F_ACC_RDONLY, H5P_DEFAULT);
    CHECK(file >= 0);
    hid_t dataset = H5Dopen2(file, dataset_path, H5P_DEFAULT);
    CHECK(dataset >= 0);
    hid_t space = H5Dget_space(dataset);
    CHECK(space >= 0);
    CHECK(H5Sget_simple_extent_ndims(space) == 1);
    hsize_t extent = 0;
    CHECK(H5Sget_simple_extent_dims(space, &extent, NULL) == 1);
    CHECK(extent <= (hsize_t)capacity);
    CHECK(H5Dread(dataset, H5T_NATIVE_DOUBLE, H5S_ALL, H5S_ALL, H5P_DEFAULT, values) >= 0);
    CHECK(H5Sclose(space) >= 0);
    CHECK(H5Dclose(dataset) >= 0);
    CHECK(H5Fclose(file) >= 0);
    return (int)extent;
}

static double read_double_scalar_from_disk(const char *ids_file, const char *dataset_path) {
    hid_t file = H5Fopen(ids_file, H5F_ACC_RDONLY, H5P_DEFAULT);
    CHECK(file >= 0);
    hid_t dataset = H5Dopen2(file, dataset_path, H5P_DEFAULT);
    CHECK(dataset >= 0);
    hid_t space = H5Dget_space(dataset);
    CHECK(space >= 0);
    CHECK(H5Sget_simple_extent_ndims(space) == 0);
    double value = 0.0;
    CHECK(H5Dread(dataset, H5T_NATIVE_DOUBLE, H5S_ALL, H5S_ALL, H5P_DEFAULT, &value) >= 0);
    CHECK(H5Sclose(space) >= 0);
    CHECK(H5Dclose(dataset) >= 0);
    CHECK(H5Fclose(file) >= 0);
    return value;
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

/* --- the ABI sequence a put_slice performs ------------------------------- */

/* Appends one DOUBLE_DATA scalar to a new time slice through a WRITE_OP slice
 * action, which is the only write real IMAS-Core's HDF5 backend accepts into an
 * occurrence that already holds data. A WRITE_OP *global* action would take
 * HDF5Writer::write_ND_Data's non-slice branch, whose `create()` call is a bare
 * H5Dcreate2 with no existence check, so it fails on every dataset the fixture
 * already has; the SLICE_OP branch checks H5Lexists and opens instead. That is
 * also exactly ADR 0016's scope — the append or partial write into an
 * existing, differently-stamped occurrence.
 *
 * The slice's own `time` value is deliberately not written. Real IMAS-Core
 * segfaults on al_write_data(op_ctx, "time", ...) through a slice-mode
 * operation context, before and after the shim's own translation and with
 * matching DD versions too, so it is IMAS-Core's crash rather than the shim's
 * and there is nothing here that can avoid it. Every claim below is about one
 * leaf dataset's own contents, so a slice without its time coordinate is
 * enough to make it. */
static al_status_t append_slice_scalar(int pulse_ctx, const char *field, double value) {
    int op_ctx = -1;
    CHECK_OK(al_begin_slice_action(pulse_ctx, "equilibrium", WRITE_OP, APPENDED_SLICE_TIME,
                                   UNDEFINED_INTERP, &op_ctx));

    int size = 1;
    int aos_ctx = -1;
    CHECK_OK(al_begin_arraystruct_action(op_ctx, "time_slice", "time", &size, &aos_ctx));

    al_status_t status = al_write_data(aos_ctx, field, "time", &value, DOUBLE_DATA, 0, NULL);

    CHECK_OK(al_end_action(aos_ctx));
    CHECK_OK(al_end_action(op_ctx));
    return status;
}

/* claim 3, asserted after every successful translating write rather than once
 * on its own: a put_slice must leave the DD-version stamp reading the *stored*
 * version. ADR 0016 decision 5 makes the stamp immutable under a mismatch, and
 * the Fortran generator never writes it in slice mode at all, so this is the
 * on-disk half of both facts. */
static void check_stamp_still_reads(const char *fixture_version) {
    char equilibrium_file[1024];
    equilibrium_file_path(equilibrium_file, sizeof equilibrium_file);
    char stamp[64];
    read_fixture_dd_version_stamp(equilibrium_file, stamp, sizeof stamp);
    CHECK(strcmp(stamp, fixture_version) == 0);
}

/* --- claims 1 and 3: the stored spelling holds the value ----------------- */

/* `rename-beta-normal`: 3.39.0's time_slice/global_quantities/beta_normal is
 * 4.1.1's .../beta_tor_norm, fidelity exact both ways and no value
 * transformation. Writing through the HLI's own spelling must extend the
 * *stored* dataset and must not create the HLI's spelling beside it — which is
 * the half no shim round trip can see, because a later read translates back. */
static void check_write_lands_on_the_stored_spelling(const char *hli_version,
                                                     const char *fixture_version,
                                                     const char *hli_field,
                                                     const char *stored_dataset,
                                                     const char *hli_dataset) {
    copy_fixture_pair(fixture_version);
    CHECK_OK(imas_mvdd_set_hli_dd_version(hli_version));
    int pulse_ctx = open_copied_fixture_pulse();
    CHECK_OK(append_slice_scalar(pulse_ctx, hli_field, 7.5));
    close_fixture_pulse(pulse_ctx);

    char equilibrium_file[1024];
    equilibrium_file_path(equilibrium_file, sizeof equilibrium_file);
    double stored[FIXTURE_SLICE_CAPACITY];
    int slices = read_double_slices_from_disk(equilibrium_file, stored_dataset, stored,
                                             FIXTURE_SLICE_CAPACITY);
    CHECK(slices == FIXTURE_SLICES + 1);
    CHECK(stored[FIXTURE_SLICES] == 7.5);
    CHECK(!dataset_exists_on_disk(equilibrium_file, hli_dataset));
    check_stamp_still_reads(fixture_version);

    remove_fixture_pair();
}

static void scenario_write_forward_lands_on_the_stored_spelling(void) {
    check_write_lands_on_the_stored_spelling(
        "4.1.1", "3.39.0", "global_quantities/beta_tor_norm", BETA_NORMAL_DATASET,
        BETA_TOR_NORM_DATASET);
    printf("write_delete_oracle_test write-oracle-forward-lands-on-the-stored-spelling: a 4.1.1 write of "
           "beta_tor_norm reached the 3.39.0 fixture's beta_normal, and beta_tor_norm was never "
           "created\n");
}

static void scenario_write_reverse_lands_on_the_stored_spelling(void) {
    check_write_lands_on_the_stored_spelling("3.39.0", "4.1.1", "global_quantities/beta_normal",
                                             BETA_TOR_NORM_DATASET, BETA_NORMAL_DATASET);
    printf("write_delete_oracle_test write-oracle-reverse-lands-on-the-stored-spelling: a 3.39.0 write of "
           "beta_normal reached the 4.1.1 fixture's beta_tor_norm, and beta_normal was never "
           "created\n");
}

/* --- claims 2 and 3: the sign on disk is the stored convention ----------- */

/* time_slice/global_quantities/ip spells identically in both DD versions and
 * carries the COCOS 11-to-17 sign flip. The path therefore proves nothing; the
 * value is the whole claim, and it is only visible off the disk. */
static void check_write_flips_the_sign_on_disk(const char *hli_version,
                                              const char *fixture_version) {
    copy_fixture_pair(fixture_version);
    CHECK_OK(imas_mvdd_set_hli_dd_version(hli_version));
    int pulse_ctx = open_copied_fixture_pulse();
    CHECK_OK(append_slice_scalar(pulse_ctx, "global_quantities/ip", 7.5));
    close_fixture_pulse(pulse_ctx);

    char equilibrium_file[1024];
    equilibrium_file_path(equilibrium_file, sizeof equilibrium_file);
    double stored[FIXTURE_SLICE_CAPACITY];
    int slices =
        read_double_slices_from_disk(equilibrium_file, IP_DATASET, stored, FIXTURE_SLICE_CAPACITY);
    CHECK(slices == FIXTURE_SLICES + 1);
    CHECK(stored[FIXTURE_SLICES] == -7.5);
    check_stamp_still_reads(fixture_version);

    remove_fixture_pair();
}

static void scenario_write_forward_flips_the_sign_on_disk(void) {
    check_write_flips_the_sign_on_disk("4.1.1", "3.39.0");
    printf("write_delete_oracle_test write-oracle-forward-flips-the-sign-on-disk: a 4.1.1 write of "
           "ip=7.5 reached the 3.39.0 fixture as -7.5\n");
}

static void scenario_write_reverse_flips_the_sign_on_disk(void) {
    check_write_flips_the_sign_on_disk("3.39.0", "4.1.1");
    printf("write_delete_oracle_test write-oracle-reverse-flips-the-sign-on-disk: a 3.39.0 write of "
           "ip=7.5 reached the 4.1.1 fixture as -7.5\n");
}

/* --- claim 4: the precedence-2 candidate is left as it was --------------- */

/* `split-psi-axis` folds 3.39.0's one time_slice/global_quantities/psi_axis
 * onto two 4.1.1 paths: psi_axis at precedence 1 and psi_magnetic_axis at
 * precedence 2. ADR 0016 decision 4 writes only the precedence-1 slot, so the
 * new slice must appear in psi_axis and must *not* appear in
 * psi_magnetic_axis, which keeps exactly the slices it already had. Both take
 * the sign flip, so the written value proves the transformation reached the
 * candidate that was chosen. This claim exists only in the reverse direction:
 * the fan-out is on the artifact's right-hand (4.1.1) side, so only a 3.39.0
 * HLI writing into a 4.1.1 store has more than one stored slot to choose
 * between. */
static void scenario_write_reverse_leaves_the_precedence_two_candidate_alone(void) {
    copy_fixture_pair("4.1.1");
    CHECK_OK(imas_mvdd_set_hli_dd_version("3.39.0"));
    int pulse_ctx = open_copied_fixture_pulse();
    CHECK_OK(append_slice_scalar(pulse_ctx, "global_quantities/psi_axis", 7.5));
    close_fixture_pulse(pulse_ctx);

    char equilibrium_file[1024];
    equilibrium_file_path(equilibrium_file, sizeof equilibrium_file);
    double primary[FIXTURE_SLICE_CAPACITY];
    int primary_slices = read_double_slices_from_disk(equilibrium_file, PSI_AXIS_DATASET, primary,
                                                     FIXTURE_SLICE_CAPACITY);
    CHECK(primary_slices == FIXTURE_SLICES + 1);
    CHECK(primary[FIXTURE_SLICES] == -7.5);

    double secondary[FIXTURE_SLICE_CAPACITY];
    int secondary_slices = read_double_slices_from_disk(
        equilibrium_file, PSI_MAGNETIC_AXIS_DATASET, secondary, FIXTURE_SLICE_CAPACITY);
    CHECK(secondary_slices == FIXTURE_SLICES);
    check_stamp_still_reads("4.1.1");

    remove_fixture_pair();
    printf("write_delete_oracle_test write-oracle-reverse-leaves-the-precedence-two-candidate-alone: the "
           "3.39.0 psi_axis write extended the 4.1.1 fixture's psi_axis to -7.5 and left "
           "psi_magnetic_axis at its original %d slices\n",
           FIXTURE_SLICES);
}

/* `fold-energy-mhd` is the forward-direction complement to
 * `split-psi-axis` above: a 4.1.1 HLI's one `energy_mhd` path resolves to the
 * 3.39.0 store's precedence-1 `energy_mhd` and precedence-2 `w_mhd`
 * candidates. A write must extend only the primary dataset. This closes the
 * directional hole in the on-disk oracle: the reverse scenario above proves
 * primary-only writes for a `split`; this one proves them for a forward
 * `merged` plan. */
static void scenario_write_forward_leaves_the_precedence_two_candidate_alone(void) {
    copy_fixture_pair("3.39.0");
    CHECK_OK(imas_mvdd_set_hli_dd_version("4.1.1"));
    int pulse_ctx = open_copied_fixture_pulse();
    CHECK_OK(append_slice_scalar(pulse_ctx, "global_quantities/energy_mhd", 7.5));
    close_fixture_pulse(pulse_ctx);

    char equilibrium_file[1024];
    equilibrium_file_path(equilibrium_file, sizeof equilibrium_file);
    double primary[FIXTURE_SLICE_CAPACITY];
    int primary_slices = read_double_slices_from_disk(equilibrium_file, ENERGY_MHD_DATASET,
                                                     primary, FIXTURE_SLICE_CAPACITY);
    CHECK(primary_slices == FIXTURE_SLICES + 1);
    CHECK(primary[FIXTURE_SLICES] == 7.5);

    double secondary[FIXTURE_SLICE_CAPACITY];
    int secondary_slices = read_double_slices_from_disk(
        equilibrium_file, W_MHD_DATASET, secondary, FIXTURE_SLICE_CAPACITY);
    CHECK(secondary_slices == FIXTURE_SLICES);
    check_stamp_still_reads("3.39.0");

    remove_fixture_pair();
    printf("write_delete_oracle_test write-oracle-forward-leaves-the-precedence-two-candidate-alone: "
           "the 4.1.1 energy_mhd write extended the 3.39.0 fixture's energy_mhd to 7.5 and "
           "left w_mhd at its original %d slices\n",
           FIXTURE_SLICES);
}

/* The forward direction's counterpart to the claim above. `psi_magnetic_axis`
 * is `split-psi-axis`'s precedence-2 *source* when a 4.1.1 HLI writes into a
 * 3.39.0 store, and ADR 0016 decision 2 refuses every non-primary source
 * before IMAS-Core is called. The on-disk assertion is the one that matters:
 * the refusal must leave the stored counterpart exactly as it was, so the
 * caller cannot have half-written through a path the shim declined.
 *
 * It does *not* leave the occurrence untouched, and that is asserted here
 * rather than glossed. The caller's own `al_begin_arraystruct_action(size=1)`
 * widens `time_slice` to a third slice before any leaf write is attempted, and
 * IMAS-Core commits that shape at end-action time whether or not a leaf value
 * followed. So a refused write leaves an *empty appended slice* behind — every
 * leaf dataset unchanged, the container one element longer. That is the on-disk
 * form of the limitation this project already records in prose: a shim refusal
 * on a write aborts the put where it stands, with no rollback, so what the HLI
 * had already committed to the traversal stays committed. Reading it off the
 * disk is the point of this file. */
static void scenario_write_forward_non_primary_source_refuses(void) {
    copy_fixture_pair("3.39.0");
    CHECK_OK(imas_mvdd_set_hli_dd_version("4.1.1"));
    int pulse_ctx = open_copied_fixture_pulse();

    double sentinel = 7.5;
    int op_ctx = -1;
    CHECK_OK(al_begin_slice_action(pulse_ctx, "equilibrium", WRITE_OP, APPENDED_SLICE_TIME,
                                   UNDEFINED_INTERP, &op_ctx));
    int size = 1;
    int aos_ctx = -1;
    CHECK_OK(al_begin_arraystruct_action(op_ctx, "time_slice", "time", &size, &aos_ctx));
    const char *field = "global_quantities/psi_magnetic_axis";
    al_status_t status =
        al_write_data(aos_ctx, field, "time", &sentinel, DOUBLE_DATA, 0, NULL);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status, "this path is a non-primary source and cannot write a shared stored slot",
                          "time_slice/global_quantities/psi_magnetic_axis", "4.1.1", "3.39.0");
    CHECK(sentinel == 7.5);
    CHECK_OK(al_end_action(aos_ctx));
    CHECK_OK(al_end_action(op_ctx));
    close_fixture_pulse(pulse_ctx);

    char equilibrium_file[1024];
    equilibrium_file_path(equilibrium_file, sizeof equilibrium_file);
    double stored[FIXTURE_SLICE_CAPACITY];
    int slices = read_double_slices_from_disk(equilibrium_file, PSI_AXIS_DATASET, stored,
                                             FIXTURE_SLICE_CAPACITY);
    CHECK(slices == FIXTURE_SLICES);
    CHECK(!dataset_exists_on_disk(equilibrium_file, PSI_MAGNETIC_AXIS_DATASET));
    check_stamp_still_reads("3.39.0");
    /* The empty appended slice the refusal left behind. */
    double aos_shape[FIXTURE_SLICE_CAPACITY];
    CHECK(read_double_slices_from_disk(equilibrium_file, TIME_SLICE_AOS_SHAPE_DATASET, aos_shape,
                                       FIXTURE_SLICE_CAPACITY) == 1);
    CHECK(aos_shape[0] == (double)(FIXTURE_SLICES + 1));

    remove_fixture_pair();
    printf("write_delete_oracle_test write-oracle-forward-non-primary-source-refuses: the "
           "4.1.1 psi_magnetic_axis write was refused before IMAS-Core, the 3.39.0 fixture's "
           "psi_axis kept its original %d slices, and the refusal left one empty appended slice "
           "behind\n",
           FIXTURE_SLICES);
}

/* The third refusal ADR 0016 names, and the last of the three issue #136's
 * acceptance criteria asks for on a real write-mode context. `new-q-min-psi`
 * declares `time_slice/global_quantities/q_min/psi` `right_only`: DD 4.1.1 has
 * it and DD 3.39.0 has no slot for it at all, so decision 3 refuses the write
 * rather than reporting success for data that went nowhere. The on-disk
 * assertion is that the path really is absent afterwards — a refusal that
 * quietly created the dataset would be worse than one that reported failure. */
static void scenario_write_forward_no_stored_slot_refuses(void) {
    copy_fixture_pair("3.39.0");
    CHECK_OK(imas_mvdd_set_hli_dd_version("4.1.1"));
    int pulse_ctx = open_copied_fixture_pulse();

    int op_ctx = -1;
    CHECK_OK(al_begin_slice_action(pulse_ctx, "equilibrium", WRITE_OP, APPENDED_SLICE_TIME,
                                   UNDEFINED_INTERP, &op_ctx));
    int size = 1;
    int aos_ctx = -1;
    CHECK_OK(al_begin_arraystruct_action(op_ctx, "time_slice", "time", &size, &aos_ctx));

    double sentinel = 7.5;
    al_status_t status =
        al_write_data(aos_ctx, "global_quantities/q_min/psi", "time", &sentinel, DOUBLE_DATA, 0,
                      NULL);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status, "this path has no stored source",
                          "time_slice/global_quantities/q_min/psi", "4.1.1", "3.39.0");
    CHECK(sentinel == 7.5);

    CHECK_OK(al_end_action(aos_ctx));
    CHECK_OK(al_end_action(op_ctx));
    close_fixture_pulse(pulse_ctx);

    char equilibrium_file[1024];
    equilibrium_file_path(equilibrium_file, sizeof equilibrium_file);
    CHECK(!dataset_exists_on_disk(equilibrium_file, Q_MIN_PSI_DATASET));
    /* Its DD 3.39.0 siblings under the same structure are untouched, so the
     * refusal was scoped to the path with no slot rather than to `q_min`. */
    double siblings[FIXTURE_SLICE_CAPACITY];
    CHECK(read_double_slices_from_disk(equilibrium_file, Q_MIN_VALUE_DATASET, siblings,
                                       FIXTURE_SLICE_CAPACITY) == FIXTURE_SLICES);
    check_stamp_still_reads("3.39.0");

    remove_fixture_pair();
    printf("write_delete_oracle_test write-oracle-forward-no-stored-slot-refuses: the 4.1.1 "
           "q_min/psi write was refused before IMAS-Core and no such dataset exists in the "
           "3.39.0 fixture\n");
}

/* --- claim 4's delete half: a fan-out never reports a silent success ------ */

/* A delete fan-out cannot use a presence probe through its WRITE_OP context:
 * real IMAS-Core only prepares that context for writing, so every probe looks
 * absent and the old shim returned success without forwarding a delete. The
 * fan-out now attempts every candidate directly. This backend's deleteData
 * ignores the path and removes the whole occurrence (issue #139). A zero
 * result is therefore valid only with that observable deletion: it proves at
 * least one candidate reached Core rather than becoming a successful no-op. */
static void scenario_delete_reverse_fan_out_reaches_disk(void) {
    copy_fixture_pair("4.1.1");
    CHECK_OK(imas_mvdd_set_hli_dd_version("3.39.0"));
    int pulse_ctx = open_copied_fixture_pulse();

    int op_ctx = -1;
    CHECK_OK(al_begin_global_action(pulse_ctx, "equilibrium", "", WRITE_OP, &op_ctx));
    al_status_t deletion = al_delete_data(op_ctx, "time_slice/global_quantities/psi_axis");
    CHECK_OK(deletion);
    CHECK_OK(al_end_action(op_ctx));
    close_fixture_pulse(pulse_ctx);

    char equilibrium_file[1024];
    equilibrium_file_path(equilibrium_file, sizeof equilibrium_file);
    CHECK(access(equilibrium_file, F_OK) != 0);

    remove_fixture_pair();
    printf("write_delete_oracle_test delete-oracle-reverse-fan-out-reaches-disk: a fan-out reached "
           "IMAS-Core instead of reporting a successful no-op\n");
}

/* --- claim 5: a refused write leaves no trace on disk -------------------- */

/* This scenario carries two claims at once, and the `WRITE_OP` open is what
 * makes the second one possible. ADR 0016 decision 5 refuses any write to the
 * DD-version stamp under a mismatch, before IMAS-Core is called, and that
 * refusal can only fire if the open registered a conversion record — which,
 * before ADR 0020, a `WRITE_OP` open never did on real Core. So the refusal
 * firing here *is* the proof that a write-mode global open now discovers and
 * registers the mismatch (issue #136), and the untouched stamp on disk is
 * #133's claim 5. */
static void check_refused_write_leaves_stamp_untouched(const char *hli_version,
                                                        const char *fixture_version) {
    copy_fixture_pair(fixture_version);
    CHECK_OK(imas_mvdd_set_hli_dd_version(hli_version));
    int pulse_ctx = open_copied_fixture_pulse();

    int op_ctx = -1;
    CHECK_OK(al_begin_global_action(pulse_ctx, "equilibrium", "", WRITE_OP, &op_ctx));

    const char *field = "ids_properties/version_put/data_dictionary";
    double sentinel = 42.0;
    al_status_t status = al_write_data(op_ctx, field, "", &sentinel, DOUBLE_DATA, 0, NULL);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status, "the DD-version stamp is immutable under a version mismatch",
                          field, hli_version, fixture_version);
    CHECK(sentinel == 42.0);

    CHECK_OK(al_end_action(op_ctx));
    close_fixture_pulse(pulse_ctx);

    check_stamp_still_reads(fixture_version);

    remove_fixture_pair();
}

static void scenario_write_forward_refusal_leaves_the_stamp_untouched(void) {
    check_refused_write_leaves_stamp_untouched("4.1.1", "3.39.0");
    printf("write_delete_oracle_test write-oracle-forward-refusal-leaves-the-stamp-untouched: a refused "
           "4.1.1 stamp write left the 3.39.0 fixture's stamp exactly as it was\n");
}

static void scenario_write_reverse_refusal_leaves_the_stamp_untouched(void) {
    check_refused_write_leaves_stamp_untouched("3.39.0", "4.1.1");
    printf("write_delete_oracle_test write-oracle-reverse-refusal-leaves-the-stamp-untouched: a refused "
           "3.39.0 stamp write left the 4.1.1 fixture's stamp exactly as it was\n");
}

/* --- the probe's cost on the workflow ADR 0016 leaves untouched ---------- */

/* ADR 0020's probe fires on every write-mode open, including the one case it
 * can never learn anything from: a brand-new occurrence, which has no stamp to
 * read. That is the single most ordinary thing a writer does, so the claim that
 * a failed probe is harmless is asserted rather than argued.
 *
 * The on-disk half is the part worth having. ADR 0007 presumes an unstamped
 * occurrence matches the HLI, so a full `put` into a fresh one must reach disk
 * spelled the HLI's own way and must not be translated. `rename-beta-normal`
 * makes that observable: a 4.1.1 HLI's `beta_tor_norm` has to stay
 * `beta_tor_norm`, and the 3.39.0 spelling must not appear. If the probe ever
 * started registering a record for an occurrence it could not read, this is
 * the scenario that would catch it. */
static void scenario_write_fresh_occurrence_is_untranslated(void) {
    char temp_dir[1024];
    int temp_length = snprintf(temp_dir, sizeof temp_dir, "/tmp/imas-mvdd-fresh-occurrence-XXXXXX");
    CHECK(temp_length > 0 && (size_t)temp_length < sizeof temp_dir);
    CHECK(mkdtemp(temp_dir) != NULL);
    CHECK_OK(imas_mvdd_set_hli_dd_version("4.1.1"));

    char uri[1024];
    int uri_length = snprintf(uri, sizeof uri, "imas:hdf5?path=%s", temp_dir);
    CHECK(uri_length > 0 && (size_t)uri_length < sizeof uri);
    int pulse_ctx = -1;
    CHECK_OK(al_begin_dataentry_action(uri, CREATE_PULSE, &pulse_ctx));

    /* The probe's own READ_OP open of an occurrence that does not exist yet
     * happens here, and must not make this open fail. */
    int op_ctx = -1;
    CHECK_OK(al_begin_global_action(pulse_ctx, "equilibrium", "", WRITE_OP, &op_ctx));

    double value = 7.5;
    CHECK_OK(al_write_data(op_ctx, "time_slice/global_quantities/beta_tor_norm", "", &value,
                           DOUBLE_DATA, 0, NULL));
    CHECK_OK(al_end_action(op_ctx));
    CHECK_OK(al_close_pulse(pulse_ctx, CLOSE_PULSE));

    char equilibrium_file[1024];
    int file_length = snprintf(equilibrium_file, sizeof equilibrium_file, "%s/equilibrium.h5",
                               temp_dir);
    CHECK(file_length > 0 && (size_t)file_length < sizeof equilibrium_file);
    /* A `put` outside any arraystruct context writes the untensorized name, so
     * these are the fresh-occurrence spellings rather than the `time_slice[]&`
     * ones every other scenario in this file reads. */
    CHECK(read_double_scalar_from_disk(
              equilibrium_file, "/equilibrium/time_slice&global_quantities&beta_tor_norm") == 7.5);
    CHECK(!dataset_exists_on_disk(equilibrium_file,
                                  "/equilibrium/time_slice&global_quantities&beta_normal"));

    remove_fixture_file(temp_dir, "equilibrium.h5");
    remove_fixture_file(temp_dir, "master.h5");
    CHECK(rmdir(temp_dir) == 0 || errno == ENOENT);
    printf("write_delete_oracle_test write-oracle-fresh-occurrence-is-untranslated: a 4.1.1 put into a "
           "brand-new occurrence survived the stamp probe and reached disk spelled its own way\n");
}

int main(int argc, char **argv) {
    static const shim_test_scenario scenarios[] = {
        {"write-oracle-forward-refusal-leaves-the-stamp-untouched",
         scenario_write_forward_refusal_leaves_the_stamp_untouched},
        {"write-oracle-reverse-refusal-leaves-the-stamp-untouched",
         scenario_write_reverse_refusal_leaves_the_stamp_untouched},
        {"write-oracle-forward-lands-on-the-stored-spelling",
         scenario_write_forward_lands_on_the_stored_spelling},
        {"write-oracle-reverse-lands-on-the-stored-spelling",
         scenario_write_reverse_lands_on_the_stored_spelling},
        {"write-oracle-forward-flips-the-sign-on-disk", scenario_write_forward_flips_the_sign_on_disk},
        {"write-oracle-reverse-flips-the-sign-on-disk", scenario_write_reverse_flips_the_sign_on_disk},
        {"write-oracle-reverse-leaves-the-precedence-two-candidate-alone",
         scenario_write_reverse_leaves_the_precedence_two_candidate_alone},
        {"write-oracle-forward-leaves-the-precedence-two-candidate-alone",
         scenario_write_forward_leaves_the_precedence_two_candidate_alone},
        {"write-oracle-forward-non-primary-source-refuses",
         scenario_write_forward_non_primary_source_refuses},
        {"write-oracle-forward-no-stored-slot-refuses",
         scenario_write_forward_no_stored_slot_refuses},
        {"delete-oracle-reverse-fan-out-reaches-disk", scenario_delete_reverse_fan_out_reaches_disk},
        {"write-oracle-fresh-occurrence-is-untranslated",
         scenario_write_fresh_occurrence_is_untranslated},
    };
    return RUN_NAMED_SCENARIO(argc, argv, scenarios);
}
