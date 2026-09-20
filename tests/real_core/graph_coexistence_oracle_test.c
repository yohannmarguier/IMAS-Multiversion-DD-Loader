/* Issue #228: graph-derived 3.42.0 coexistence must affect genuine HDF5
 * storage, not only the recording stub.  The private fixture begins as the
 * checked-in 4.1.1 equilibrium pulse, then gains the two valid 3.42.0
 * predecessor spellings and a 3.42.0 stamp.  Raw HDF5 inspection is the
 * oracle: a converted read has no path trace, and a shim round trip would
 * hide both a primary-only write and a delete that missed one candidate. */

#include <stdio.h>
#include <dlfcn.h>
#include <stdlib.h>
#include <string.h>

#include <al_const.h>
#include <hdf5.h>
#include "../support/real_core_fixture_support.h"

#ifndef EQUILIBRIUM_FIXTURE_DIR
#error "EQUILIBRIUM_FIXTURE_DIR must name the imas-python-fixtures/fixtures directory"
#endif

#define FIXTURE_SLICES 2
#define FIXTURE_SLICE_CAPACITY 16
#define APPENDED_SLICE_TIME 3.0
#define EMPTY_DOUBLE (-9e40)

#define B_FIELD_PHI_DATASET "/equilibrium/time_slice[]&global_quantities&magnetic_axis&b_field_phi"
#define B_FIELD_TOR_DATASET "/equilibrium/time_slice[]&global_quantities&magnetic_axis&b_field_tor"
#define J_PHI_DATASET_PREFIX "/equilibrium/time_slice[]&constraints&j_phi[]&"
#define J_TOR_DATASET_PREFIX "/equilibrium/time_slice[]&constraints&j_tor[]&"
#define J_PHI_RECONSTRUCTED_DATASET J_PHI_DATASET_PREFIX "reconstructed"
#define J_TOR_RECONSTRUCTED_DATASET J_TOR_DATASET_PREFIX "reconstructed"
#define IP_DATASET "/equilibrium/time_slice[]&global_quantities&ip"

static real_core_fixture_copy copied_fixture;

static void remove_fixture_pair(void) {
    remove_real_core_fixture_copy(&copied_fixture);
}

static void equilibrium_file_path(char *path, size_t path_size) {
    real_core_fixture_ids_file(&copied_fixture, path, path_size);
}

static int dataset_exists_on_disk(const char *ids_file, const char *dataset_path) {
    hid_t file = H5Fopen(ids_file, H5F_ACC_RDONLY, H5P_DEFAULT);
    CHECK(file >= 0);
    htri_t exists = H5Lexists(file, dataset_path, H5P_DEFAULT);
    CHECK(exists >= 0);
    CHECK(H5Fclose(file) >= 0);
    return exists > 0;
}

static void copy_dataset(hid_t file, const char *from, const char *to) {
    CHECK(H5Lexists(file, from, H5P_DEFAULT) > 0);
    CHECK(H5Lexists(file, to, H5P_DEFAULT) == 0);
    CHECK(H5Ocopy(file, from, file, to, H5P_DEFAULT, H5P_DEFAULT) >= 0);
}

static const char *const j_field_suffixes[] = {
    "AOS_SHAPE",       "chi_squared", "exact",        "measured", "position&phi",
    "position&psi",    "position&r",  "position&rho_tor_norm", "position&z",
    "reconstructed",   "source",      "time_measurement", "weight",
};

static void copy_j_phi_candidates(hid_t file) {
    for (size_t index = 0; index < sizeof j_field_suffixes / sizeof *j_field_suffixes; ++index) {
        char phi_path[256];
        char tor_path[256];
        CHECK(snprintf(phi_path, sizeof phi_path, "%s%s", J_PHI_DATASET_PREFIX,
                       j_field_suffixes[index]) > 0);
        CHECK(snprintf(tor_path, sizeof tor_path, "%s%s", J_TOR_DATASET_PREFIX,
                       j_field_suffixes[index]) > 0);
        copy_dataset(file, phi_path, tor_path);
    }
}

static void remove_j_phi_candidates(const char *ids_file) {
    hid_t file = H5Fopen(ids_file, H5F_ACC_RDWR, H5P_DEFAULT);
    CHECK(file >= 0);
    for (size_t index = 0; index < sizeof j_field_suffixes / sizeof *j_field_suffixes; ++index) {
        char phi_path[256];
        CHECK(snprintf(phi_path, sizeof phi_path, "%s%s", J_PHI_DATASET_PREFIX,
                       j_field_suffixes[index]) > 0);
        CHECK(H5Ldelete(file, phi_path, H5P_DEFAULT) >= 0);
    }
    CHECK(H5Fclose(file) >= 0);
}

static void fill_double_dataset(const char *ids_file, const char *dataset_path, double value) {
    hid_t file = H5Fopen(ids_file, H5F_ACC_RDWR, H5P_DEFAULT);
    CHECK(file >= 0);
    hid_t dataset = H5Dopen2(file, dataset_path, H5P_DEFAULT);
    CHECK(dataset >= 0);
    hid_t space = H5Dget_space(dataset);
    CHECK(space >= 0);
    hssize_t points = H5Sget_simple_extent_npoints(space);
    CHECK(points > 0);
    double *values = malloc((size_t)points * sizeof *values);
    CHECK(values != NULL);
    for (hssize_t index = 0; index < points; ++index) values[index] = value;
    CHECK(H5Dwrite(dataset, H5T_NATIVE_DOUBLE, H5S_ALL, H5S_ALL, H5P_DEFAULT, values) >= 0);
    free(values);
    CHECK(H5Sclose(space) >= 0);
    CHECK(H5Dclose(dataset) >= 0);
    CHECK(H5Fclose(file) >= 0);
}

static int read_double_slices_from_disk(const char *ids_file, const char *dataset_path,
                                        double *values, int capacity) {
    hid_t file = H5Fopen(ids_file, H5F_ACC_RDONLY, H5P_DEFAULT);
    CHECK(file >= 0);
    hid_t dataset = H5Dopen2(file, dataset_path, H5P_DEFAULT);
    CHECK(dataset >= 0);
    hid_t space = H5Dget_space(dataset);
    CHECK(space >= 0);
    int rank = H5Sget_simple_extent_ndims(space);
    CHECK(rank >= 1);
    hsize_t dimensions[4] = {0};
    CHECK(H5Sget_simple_extent_dims(space, dimensions, NULL) == rank);
    CHECK(dimensions[0] <= (hsize_t)capacity);
    hsize_t points = 1;
    for (int index = 0; index < rank; ++index) points *= dimensions[index];
    double *all_values = malloc((size_t)points * sizeof *all_values);
    CHECK(all_values != NULL);
    CHECK(H5Dread(dataset, H5T_NATIVE_DOUBLE, H5S_ALL, H5S_ALL, H5P_DEFAULT, all_values) >= 0);
    for (hsize_t index = 0; index < dimensions[0]; ++index) values[index] = all_values[index];
    free(all_values);
    CHECK(H5Sclose(space) >= 0);
    CHECK(H5Dclose(dataset) >= 0);
    CHECK(H5Fclose(file) >= 0);
    return (int)dimensions[0];
}

static void prepare_coexisting_fixture(void) {
    create_real_core_fixture_copy(&copied_fixture, EQUILIBRIUM_FIXTURE_DIR, "4.1.1",
                                  "/tmp/imas-mvdd-graph-coexistence-XXXXXX");
    CHECK(atexit(remove_fixture_pair) == 0);
    char equilibrium_file[1024];
    equilibrium_file_path(equilibrium_file, sizeof equilibrium_file);

    hid_t file = H5Fopen(equilibrium_file, H5F_ACC_RDWR, H5P_DEFAULT);
    CHECK(file >= 0);
    copy_dataset(file, B_FIELD_PHI_DATASET, B_FIELD_TOR_DATASET);
    copy_j_phi_candidates(file);
    hid_t stamp = H5Dopen2(file, "/equilibrium/ids_properties&version_put&data_dictionary",
                            H5P_DEFAULT);
    CHECK(stamp >= 0);
    hid_t stamp_type = H5Dget_type(stamp);
    CHECK(stamp_type >= 0);
    const char *version = "3.42.0";
    CHECK(H5Dwrite(stamp, stamp_type, H5S_ALL, H5S_ALL, H5P_DEFAULT, &version) >= 0);
    CHECK(H5Tclose(stamp_type) >= 0);
    CHECK(H5Dclose(stamp) >= 0);
    CHECK(H5Fclose(file) >= 0);

    /* Distinct real values make candidate selection visible without a shim
     * side channel.  The predecessor remains valid and populated. */
    fill_double_dataset(equilibrium_file, B_FIELD_TOR_DATASET, 29.0);
    fill_double_dataset(equilibrium_file, J_PHI_RECONSTRUCTED_DATASET, 5.0);
    fill_double_dataset(equilibrium_file, J_TOR_RECONSTRUCTED_DATASET, 29.0);
}

static int open_fixture_pulse(void) {
    char uri[1024];
    int length = snprintf(uri, sizeof uri, "imas:hdf5?path=%s", copied_fixture.pulse_dir);
    CHECK(length > 0 && (size_t)length < sizeof uri);
    int pulse_ctx = -1;
    CHECK_OK(al_begin_dataentry_action(uri, OPEN_PULSE, &pulse_ctx));
    return pulse_ctx;
}

static int open_slice_context(int pulse_ctx, int rwmode, int *operation_ctx) {
    CHECK_OK(al_begin_global_action(pulse_ctx, "equilibrium", "", rwmode, operation_ctx));
    int slices = -1;
    int slice_ctx = -1;
    CHECK_OK(al_begin_arraystruct_action(*operation_ctx, "time_slice", "", &slices, &slice_ctx));
    return slice_ctx;
}

static void close_slice_context(int slice_ctx, int operation_ctx, int pulse_ctx) {
    CHECK_OK(al_end_action(slice_ctx));
    CHECK_OK(al_end_action(operation_ctx));
    CHECK_OK(al_close_pulse(pulse_ctx, CLOSE_PULSE));
}

static double read_b_field_phi_through_shim(void) {
    int pulse_ctx = open_fixture_pulse();
    int operation_ctx = -1;
    int slice_ctx = open_slice_context(pulse_ctx, READ_OP, &operation_ctx);
    double result = EMPTY_DOUBLE;
    void *data = &result;
    int shape[MAXDIM] = {0};
    CHECK_OK(al_read_data(slice_ctx, "global_quantities/magnetic_axis/b_field_phi", "", &data,
                          DOUBLE_DATA, 0, shape));
    CHECK(data == &result);
    CHECK(result != EMPTY_DOUBLE);
    close_slice_context(slice_ctx, operation_ctx, pulse_ctx);
    return result;
}

static double open_j_phi_and_read_relative_reconstructed(void) {
    int pulse_ctx = open_fixture_pulse();
    int operation_ctx = -1;
    int slice_ctx = open_slice_context(pulse_ctx, READ_OP, &operation_ctx);
    int entries = -1;
    int j_ctx = -1;
    CHECK_OK(al_begin_arraystruct_action(slice_ctx, "constraints/j_phi", "time", &entries,
                                         &j_ctx));
    CHECK(entries > 0);
    double value = EMPTY_DOUBLE;
    void *data = &value;
    int shape[MAXDIM] = {0};
    CHECK_OK(al_read_data(j_ctx, "reconstructed", "", &data, DOUBLE_DATA, 0, shape));
    CHECK(data == &value);
    CHECK(value != EMPTY_DOUBLE);
    CHECK_OK(al_end_action(j_ctx));
    close_slice_context(slice_ctx, operation_ctx, pulse_ctx);
    return value;
}

static double read_b_field_tor_through_shim(void) {
    int pulse_ctx = open_fixture_pulse();
    int operation_ctx = -1;
    int slice_ctx = open_slice_context(pulse_ctx, READ_OP, &operation_ctx);
    double result = EMPTY_DOUBLE;
    void *data = &result;
    int shape[MAXDIM] = {0};
    CHECK_OK(al_read_data(slice_ctx, "global_quantities/magnetic_axis/b_field_tor", "", &data,
                          DOUBLE_DATA, 0, shape));
    CHECK(data == &result);
    CHECK(result != EMPTY_DOUBLE);
    close_slice_context(slice_ctx, operation_ctx, pulse_ctx);
    return result;
}

static void remove_primary_b_field(void) {
    char equilibrium_file[1024];
    equilibrium_file_path(equilibrium_file, sizeof equilibrium_file);
    hid_t file = H5Fopen(equilibrium_file, H5F_ACC_RDWR, H5P_DEFAULT);
    CHECK(file >= 0);
    CHECK(H5Ldelete(file, B_FIELD_PHI_DATASET, H5P_DEFAULT) >= 0);
    CHECK(H5Fclose(file) >= 0);
}

static void check_stamp_still_reads(const char *fixture_version) {
    char equilibrium_file[1024];
    equilibrium_file_path(equilibrium_file, sizeof equilibrium_file);
    char stamp[64];
    read_fixture_dd_version_stamp(equilibrium_file, stamp, sizeof stamp);
    CHECK(strcmp(stamp, fixture_version) == 0);
}

static al_status_t append_b_field_phi(int pulse_ctx, double value) {
    int operation_ctx = -1;
    CHECK_OK(al_begin_slice_action(pulse_ctx, "equilibrium", WRITE_OP, APPENDED_SLICE_TIME,
                                   UNDEFINED_INTERP, &operation_ctx));
    int size = 1;
    int slice_ctx = -1;
    CHECK_OK(al_begin_arraystruct_action(operation_ctx, "time_slice", "", &size, &slice_ctx));
    al_status_t status = al_write_data(slice_ctx, "global_quantities/magnetic_axis/b_field_phi",
                                       "", &value, DOUBLE_DATA, 0, NULL);
    CHECK_OK(status);
    check_loss_at(slice_ctx, 0, "time_slice/global_quantities/magnetic_axis/b_field_tor",
                  IMAS_MVDD_FIDELITY_POTENTIALLY_LOSSY, IMAS_MVDD_LOSS_OPERATION_WRITE);
    CHECK_OK(al_end_action(slice_ctx));
    CHECK_OK(al_end_action(operation_ctx));
    return status;
}

static void scenario_forward_write_is_primary_only_and_delete_fans_out(void) {
    prepare_coexisting_fixture();
    CHECK_OK(imas_mvdd_set_hli_dd_version("4.1.1"));
    char equilibrium_file[1024];
    equilibrium_file_path(equilibrium_file, sizeof equilibrium_file);

    int pulse_ctx = open_fixture_pulse();
    CHECK_OK(append_b_field_phi(pulse_ctx, 7.5));
    CHECK_OK(al_close_pulse(pulse_ctx, CLOSE_PULSE));

    double primary[FIXTURE_SLICE_CAPACITY];
    int primary_slices = read_double_slices_from_disk(equilibrium_file, B_FIELD_PHI_DATASET,
                                                       primary, FIXTURE_SLICE_CAPACITY);
    CHECK(primary_slices == FIXTURE_SLICES || primary_slices == FIXTURE_SLICES + 1);
    CHECK(primary[0] == 7.5 || (primary_slices > 1 && primary[1] == 7.5));
    double secondary[FIXTURE_SLICE_CAPACITY];
    CHECK(read_double_slices_from_disk(equilibrium_file, B_FIELD_TOR_DATASET, secondary,
                                       FIXTURE_SLICE_CAPACITY) == FIXTURE_SLICES);
    CHECK(secondary[0] == 29.0);
    check_stamp_still_reads("3.42.0");

    double unrelated_before[FIXTURE_SLICE_CAPACITY];
    int unrelated_slices = read_double_slices_from_disk(equilibrium_file, IP_DATASET,
                                                         unrelated_before, FIXTURE_SLICE_CAPACITY);
    pulse_ctx = open_fixture_pulse();
    int operation_ctx = -1;
    CHECK_OK(al_begin_global_action(pulse_ctx, "equilibrium", "", WRITE_OP, &operation_ctx));
    CHECK_OK(al_delete_data(operation_ctx,
                            "time_slice/global_quantities/magnetic_axis/b_field_phi"));
    check_loss_at(operation_ctx, 0, "time_slice/global_quantities/magnetic_axis/b_field_phi",
                  IMAS_MVDD_FIDELITY_POTENTIALLY_LOSSY, IMAS_MVDD_LOSS_OPERATION_DELETE);
    check_loss_at(operation_ctx, 1, "time_slice/global_quantities/magnetic_axis/b_field_tor",
                  IMAS_MVDD_FIDELITY_POTENTIALLY_LOSSY, IMAS_MVDD_LOSS_OPERATION_DELETE);
    CHECK_OK(al_end_action(operation_ctx));
    CHECK_OK(al_close_pulse(pulse_ctx, CLOSE_PULSE));

    CHECK(!dataset_exists_on_disk(equilibrium_file, B_FIELD_PHI_DATASET));
    CHECK(!dataset_exists_on_disk(equilibrium_file, B_FIELD_TOR_DATASET));
    double unrelated_after[FIXTURE_SLICE_CAPACITY];
    CHECK(read_double_slices_from_disk(equilibrium_file, IP_DATASET, unrelated_after,
                                       FIXTURE_SLICE_CAPACITY) == unrelated_slices);
    for (int index = 0; index < unrelated_slices; ++index) {
        CHECK(unrelated_after[index] == unrelated_before[index]);
    }
    check_stamp_still_reads("3.42.0");
    remove_fixture_pair();
    printf("graph_coexistence_oracle_test write-delete-coexistence-forward-is-primary-only-and-fans-out: "
           "a 4.1.1 write left b_field_tor untouched and delete removed both 3.42.0 candidates\n");
}

static void scenario_reverse_non_primary_write_refuses(void) {
    create_real_core_fixture_copy(&copied_fixture, EQUILIBRIUM_FIXTURE_DIR, "4.1.1",
                                  "/tmp/imas-mvdd-graph-coexistence-XXXXXX");
    CHECK(atexit(remove_fixture_pair) == 0);
    CHECK_OK(imas_mvdd_set_hli_dd_version("3.42.0"));
    int pulse_ctx = open_fixture_pulse();
    int operation_ctx = -1;
    CHECK_OK(al_begin_global_action(pulse_ctx, "equilibrium", "", WRITE_OP, &operation_ctx));
    double value = 7.5;
    al_status_t status = al_write_data(operation_ctx,
                                       "time_slice/global_quantities/magnetic_axis/b_field_tor",
                                       "", &value, DOUBLE_DATA, 0, NULL);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK_REFUSAL_MESSAGE(status,
                          "this path is a non-primary source and cannot write a shared stored slot",
                          "time_slice/global_quantities/magnetic_axis/b_field_tor", "3.42.0",
                          "4.1.1");
    int loss_count = -1;
    CHECK_OK(imas_mvdd_context_loss_count(operation_ctx, &loss_count));
    CHECK(loss_count == 1);
    CHECK_OK(al_end_action(operation_ctx));
    CHECK_OK(al_close_pulse(pulse_ctx, CLOSE_PULSE));
    check_stamp_still_reads("4.1.1");
    remove_fixture_pair();
    printf("graph_coexistence_oracle_test write-coexistence-reverse-non-primary-refuses: "
           "a 3.42.0 predecessor write refused before real Core\n");
}

static void scenario_forward_read_selects_primary_then_falls_back(void) {
    prepare_coexisting_fixture();
    CHECK_OK(imas_mvdd_set_hli_dd_version("4.1.1"));
    char equilibrium_file[1024];
    equilibrium_file_path(equilibrium_file, sizeof equilibrium_file);
    CHECK(dataset_exists_on_disk(equilibrium_file, B_FIELD_PHI_DATASET));
    CHECK(dataset_exists_on_disk(equilibrium_file, B_FIELD_TOR_DATASET));
    CHECK(read_b_field_phi_through_shim() != 29.0);
    remove_primary_b_field();
    CHECK(read_b_field_phi_through_shim() == 29.0);
    remove_fixture_pair();
    printf("graph_coexistence_oracle_test read-coexistence-forward-selects-primary-then-falls-back: "
           "a 4.1.1 read selected b_field_phi then fell back to populated 3.42.0 b_field_tor\n");
}

static void scenario_forward_arraystruct_read_falls_back_between_j_candidates(void) {
    prepare_coexisting_fixture();
    CHECK_OK(imas_mvdd_set_hli_dd_version("4.1.1"));
    char equilibrium_file[1024];
    equilibrium_file_path(equilibrium_file, sizeof equilibrium_file);
    CHECK(open_j_phi_and_read_relative_reconstructed() == 5.0);
    remove_j_phi_candidates(equilibrium_file);
    CHECK(open_j_phi_and_read_relative_reconstructed() == 29.0);
    remove_fixture_pair();
    printf("graph_coexistence_oracle_test read-coexistence-forward-arraystruct-falls-back-between-j-candidates: "
           "a relative child read retained j_phi then the fallback j_tor anchor\n");
}

static void scenario_reverse_read_selects_the_4_1_successor(void) {
    create_real_core_fixture_copy(&copied_fixture, EQUILIBRIUM_FIXTURE_DIR, "4.1.1",
                                  "/tmp/imas-mvdd-graph-coexistence-XXXXXX");
    CHECK(atexit(remove_fixture_pair) == 0);
    CHECK_OK(imas_mvdd_set_hli_dd_version("3.42.0"));
    char equilibrium_file[1024];
    equilibrium_file_path(equilibrium_file, sizeof equilibrium_file);
    fill_double_dataset(equilibrium_file, B_FIELD_PHI_DATASET, 47.0);
    CHECK(read_b_field_tor_through_shim() == 47.0);
    check_stamp_still_reads("4.1.1");
    remove_fixture_pair();
    printf("graph_coexistence_oracle_test read-coexistence-reverse-selects-the-4.1-successor: "
           "a 3.42.0 b_field_tor read resolved to stored 4.1.1 b_field_phi\n");
}

static void check_dataset_contains(const char *file_path, const char *dataset_path, double expected,
                                   int every_value) {
    hid_t file = H5Fopen(file_path, H5F_ACC_RDONLY, H5P_DEFAULT);
    CHECK(file >= 0);
    hid_t dataset = H5Dopen2(file, dataset_path, H5P_DEFAULT);
    CHECK(dataset >= 0);
    hid_t space = H5Dget_space(dataset);
    CHECK(space >= 0);
    hssize_t count = H5Sget_simple_extent_npoints(space);
    CHECK(count > 0);
    double *values = malloc((size_t)count * sizeof *values);
    CHECK(values != NULL);
    CHECK(H5Dread(dataset, H5T_NATIVE_DOUBLE, H5S_ALL, H5S_ALL, H5P_DEFAULT, values) >= 0);
    int found = 0;
    for (hssize_t index = 0; index < count; ++index) {
        if (values[index] == expected) found = 1;
        if (every_value) CHECK(values[index] == expected);
    }
    CHECK(found);
    free(values);
    CHECK(H5Sclose(space) >= 0);
    CHECK(H5Dclose(dataset) >= 0);
    CHECK(H5Fclose(file) >= 0);
}

static void live_j_operations(int reverse) {
    const char *stored_version = reverse ? "4.1.1" : "3.42.0";
    const char *read_anchor = reverse ? "constraints/j_tor" : "constraints/j_phi";
    if (reverse) {
        create_real_core_fixture_copy(&copied_fixture, EQUILIBRIUM_FIXTURE_DIR, stored_version,
                                      "/tmp/imas-mvdd-live-coexistence-XXXXXX");
        CHECK(atexit(remove_fixture_pair) == 0);
    } else {
        prepare_coexisting_fixture();
    }
    CHECK_OK(imas_mvdd_set_hli_dd_version(reverse ? "3.42.0" : "4.1.1"));
    char file[1024];
    equilibrium_file_path(file, sizeof file);
    fill_double_dataset(file, J_PHI_RECONSTRUCTED_DATASET, 47.0);
    double unrelated[FIXTURE_SLICE_CAPACITY], after[FIXTURE_SLICE_CAPACITY];
    int unrelated_count = read_double_slices_from_disk(file, IP_DATASET, unrelated, FIXTURE_SLICE_CAPACITY);

    int pulse = open_fixture_pulse(), operation = -1;
    int slice = open_slice_context(pulse, READ_OP, &operation);
    int entries = -1, j = -1;
    CHECK_OK(al_begin_arraystruct_action(slice, read_anchor, "", &entries, &j));
    CHECK(entries > 0);
    double value = EMPTY_DOUBLE;
    void *data = &value;
    CHECK_OK(al_read_data(j, "reconstructed", "", &data, DOUBLE_DATA, 0, NULL));
    CHECK(value == 47.0);
    /* Establish the pinned HDF5 backend's absolute-path behavior through Core
     * directly, using the same stored context and its stored spelling. */
    void *core = dlopen(REAL_CORE_LIBRARY_PATH, RTLD_NOW | RTLD_LOCAL);
    CHECK(core != NULL);
    typedef al_status_t (*core_read_fn)(int, const char *, const char *, void **, int, int, int *);
    /* The plugin twin enters the backend directly; Core's al_read_data
     * calls it by symbol and could be interposed again on ELF platforms. */
    core_read_fn core_read = (core_read_fn)dlsym(core, "al_plugin_read_data");
    CHECK(core_read != NULL);
    value = EMPTY_DOUBLE;
    CHECK_OK(core_read(j, "reconstructed", "", &data, DOUBLE_DATA, 0, NULL));
    CHECK(value == 47.0);
    value = EMPTY_DOUBLE;
    CHECK_OK(core_read(j, "/time_slice/constraints/j_phi/reconstructed", "", &data,
                       DOUBLE_DATA, 0, NULL));
    CHECK(value == 47.0);
    value = EMPTY_DOUBLE;
    CHECK_OK(al_read_data(j, reverse ? "/time_slice/constraints/j_tor/reconstructed"
                                     : "/time_slice/constraints/j_phi/reconstructed",
                          "", &data, DOUBLE_DATA, 0, NULL));
    CHECK(value == 47.0);
    CHECK(dlclose(core) == 0);
    printf("absolute read beneath a child returns the seeded value both directly "
           "and through the live shim\n");
    CHECK_OK(al_end_action(j));
    close_slice_context(slice, operation, pulse);

    pulse = open_fixture_pulse();
    CHECK_OK(al_begin_slice_action(pulse, "equilibrium", WRITE_OP, APPENDED_SLICE_TIME,
                                   UNDEFINED_INTERP, &operation));
    entries = 1;
    CHECK_OK(al_begin_arraystruct_action(operation, "time_slice", "", &entries, &slice));
    entries = 1;
    CHECK_OK(al_begin_arraystruct_action(slice, "constraints/j_phi", "", &entries, &j));
    value = 77.0;
    CHECK_OK(al_write_data(j, "reconstructed", "", &value, DOUBLE_DATA, 0, NULL));
    check_no_loss_entry(j); /* the fixed primary anchor filters its sibling */
    CHECK_OK(al_end_action(j));
    close_slice_context(slice, operation, pulse);
    check_dataset_contains(file, J_PHI_RECONSTRUCTED_DATASET, 77.0, 0);
    if (!reverse) check_dataset_contains(file, J_TOR_RECONSTRUCTED_DATASET, 29.0, 1);
    check_stamp_still_reads(stored_version);

    pulse = open_fixture_pulse();
    CHECK_OK(al_begin_global_action(pulse, "equilibrium", "", WRITE_OP, &operation));
    if (reverse) {
        al_status_t refusal = al_delete_data(operation, "time_slice/constraints/j_tor/reconstructed");
        CHECK(refusal.code == IMAS_MVDD_CONVERSION_ERROR);
        CHECK_REFUSAL_MESSAGE(refusal,
            "this path is a non-primary source and cannot delete a shared stored slot",
            "time_slice/constraints/j_tor/reconstructed", "3.42.0", "4.1.1");
        check_loss_at(operation, 0, "time_slice/constraints/j_tor/reconstructed",
                      IMAS_MVDD_FIDELITY_UNMAPPABLE, IMAS_MVDD_LOSS_OPERATION_DELETE);
    }
    CHECK_OK(al_delete_data(operation, "time_slice/constraints/j_phi/reconstructed"));
    if (!reverse) {
        check_loss_at(operation, 0, "time_slice/constraints/j_phi/reconstructed",
                      IMAS_MVDD_FIDELITY_POTENTIALLY_LOSSY, IMAS_MVDD_LOSS_OPERATION_DELETE);
        check_loss_at(operation, 1, "time_slice/constraints/j_tor/reconstructed",
                      IMAS_MVDD_FIDELITY_POTENTIALLY_LOSSY, IMAS_MVDD_LOSS_OPERATION_DELETE);
    }
    CHECK_OK(al_end_action(operation));
    CHECK_OK(al_close_pulse(pulse, CLOSE_PULSE));
    CHECK(!dataset_exists_on_disk(file, J_PHI_RECONSTRUCTED_DATASET));
    if (!reverse) CHECK(!dataset_exists_on_disk(file, J_TOR_RECONSTRUCTED_DATASET));
    CHECK(read_double_slices_from_disk(file, IP_DATASET, after, FIXTURE_SLICE_CAPACITY) == unrelated_count);
    for (int index = 0; index < unrelated_count; ++index) CHECK(after[index] == unrelated[index]);
    check_stamp_still_reads(stored_version);

    pulse = open_fixture_pulse();
    slice = open_slice_context(pulse, READ_OP, &operation);
    entries = -1;
    CHECK_OK(al_begin_arraystruct_action(slice, read_anchor, "", &entries, &j));
    value = 123.0;
    data = &value;
    CHECK_OK(al_read_data(j, "reconstructed", "", &data, DOUBLE_DATA, 0, NULL));
    CHECK(data == &value && value == EMPTY_DOUBLE);
    CHECK_OK(al_end_action(j));
    close_slice_context(slice, operation, pulse);
}

static void scenario_live_forward(void) { live_j_operations(0); }
static void scenario_live_reverse(void) { live_j_operations(1); }

int main(int argc, char **argv) {
    static const shim_test_scenario scenarios[] = {
        {"live-j-forward", scenario_live_forward},
        {"live-j-reverse", scenario_live_reverse},
        {"read-coexistence-forward-selects-primary-then-falls-back",
         scenario_forward_read_selects_primary_then_falls_back},
        {"read-coexistence-forward-arraystruct-falls-back-between-j-candidates",
         scenario_forward_arraystruct_read_falls_back_between_j_candidates},
        {"read-coexistence-reverse-selects-the-4.1-successor",
         scenario_reverse_read_selects_the_4_1_successor},
        {"write-delete-coexistence-forward-is-primary-only-and-fans-out",
         scenario_forward_write_is_primary_only_and_delete_fans_out},
        {"write-coexistence-reverse-non-primary-refuses",
         scenario_reverse_non_primary_write_refuses},
    };
    return RUN_NAMED_SCENARIO(argc, argv, scenarios);
}
