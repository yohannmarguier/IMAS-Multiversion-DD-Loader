/* Real-Core oracle for the graph-selected pulse_schedule history.
 *
 * Each scenario creates an isolated occurrence through Core, stamps it with
 * its stored DD version, then drives the graph-selected shim through the
 * public C ABI.  Raw HDF5 inspection is deliberately limited to the stored
 * effect: it never participates in the conversion call being asserted.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include <al_const.h>
#include <hdf5.h>

#include "../support/real_core_fixture_support.h"

static char pulse_dir[1024];
static char ids_file[1024];
static char master_file[1024];

typedef struct {
    const char *hli_version;
    const char *stored_version;
    const char *caller_path;
    const char *stored_core_path;
    const char *stored_disk_path;
} historical_direction;

static const historical_direction FORWARD = {
    "3.30.0",
    "3.25.0",
    "ec/launcher/steering_angle_pol",
    "ec/antenna/launching_angle_pol",
    "/pulse_schedule/ec&antenna&launching_angle_pol",
};

static const historical_direction REVERSE = {
    "3.25.0",
    "3.30.0",
    "ec/antenna/launching_angle_pol",
    "ec/launcher/steering_angle_pol",
    "/pulse_schedule/ec&launcher&steering_angle_pol",
};

static void remove_pulse(void) {
    if (ids_file[0] != '\0' && access(ids_file, F_OK) == 0) CHECK(remove(ids_file) == 0);
    if (master_file[0] != '\0' && access(master_file, F_OK) == 0)
        CHECK(remove(master_file) == 0);
    if (pulse_dir[0] != '\0' && access(pulse_dir, F_OK) == 0) CHECK(rmdir(pulse_dir) == 0);
}

static void make_pulse_directory(void) {
    int length = snprintf(pulse_dir, sizeof pulse_dir, "/tmp/imas-mvdd-pulse-schedule-XXXXXX");
    CHECK(length > 0 && (size_t)length < sizeof pulse_dir);
    CHECK(mkdtemp(pulse_dir) != NULL);
    length = snprintf(ids_file, sizeof ids_file, "%s/pulse_schedule.h5", pulse_dir);
    CHECK(length > 0 && (size_t)length < sizeof ids_file);
    length = snprintf(master_file, sizeof master_file, "%s/master.h5", pulse_dir);
    CHECK(length > 0 && (size_t)length < sizeof master_file);
    CHECK(atexit(remove_pulse) == 0);
}

static void write_stamp(const char *stored_version) {
    hid_t file = H5Fopen(ids_file, H5F_ACC_RDWR, H5P_DEFAULT);
    CHECK(file >= 0);
    hid_t string_type = H5Tcopy(H5T_C_S1);
    CHECK(string_type >= 0);
    CHECK(H5Tset_size(string_type, H5T_VARIABLE) >= 0);
    CHECK(H5Tset_cset(string_type, H5T_CSET_UTF8) >= 0);
    hid_t scalar = H5Screate(H5S_SCALAR);
    CHECK(scalar >= 0);
    hid_t dataset = H5Dcreate2(file, "/pulse_schedule/ids_properties&version_put&data_dictionary",
                               string_type, scalar, H5P_DEFAULT, H5P_DEFAULT, H5P_DEFAULT);
    CHECK(dataset >= 0);
    const char *value = stored_version;
    CHECK(H5Dwrite(dataset, string_type, H5S_ALL, H5S_ALL, H5P_DEFAULT, &value) >= 0);
    CHECK(H5Dclose(dataset) >= 0);
    CHECK(H5Sclose(scalar) >= 0);
    CHECK(H5Tclose(string_type) >= 0);
    CHECK(H5Fclose(file) >= 0);
}

static void write_unrelated_value(void) {
    hid_t file = H5Fopen(ids_file, H5F_ACC_RDWR, H5P_DEFAULT);
    CHECK(file >= 0);
    hid_t scalar = H5Screate(H5S_SCALAR);
    CHECK(scalar >= 0);
    hid_t dataset = H5Dcreate2(file, "/pulse_schedule/unrelated", H5T_IEEE_F64LE, scalar,
                               H5P_DEFAULT, H5P_DEFAULT, H5P_DEFAULT);
    CHECK(dataset >= 0);
    double value = 99.0;
    CHECK(H5Dwrite(dataset, H5T_NATIVE_DOUBLE, H5S_ALL, H5S_ALL, H5P_DEFAULT, &value) >= 0);
    CHECK(H5Dclose(dataset) >= 0);
    CHECK(H5Sclose(scalar) >= 0);
    CHECK(H5Fclose(file) >= 0);
}

static int dataset_exists(const char *path) {
    hid_t file = H5Fopen(ids_file, H5F_ACC_RDONLY, H5P_DEFAULT);
    CHECK(file >= 0);
    htri_t exists = H5Lexists(file, path, H5P_DEFAULT);
    CHECK(exists >= 0);
    CHECK(H5Fclose(file) >= 0);
    return exists > 0;
}

static double read_stored_scalar(const char *path) {
    hid_t file = H5Fopen(ids_file, H5F_ACC_RDONLY, H5P_DEFAULT);
    CHECK(file >= 0);
    hid_t dataset = H5Dopen2(file, path, H5P_DEFAULT);
    CHECK(dataset >= 0);
    double value = 0.0;
    CHECK(H5Dread(dataset, H5T_NATIVE_DOUBLE, H5S_ALL, H5S_ALL, H5P_DEFAULT, &value) >= 0);
    CHECK(H5Dclose(dataset) >= 0);
    CHECK(H5Fclose(file) >= 0);
    return value;
}

static void check_stamp(const char *stored_version) {
    hid_t file = H5Fopen(ids_file, H5F_ACC_RDONLY, H5P_DEFAULT);
    CHECK(file >= 0);
    hid_t dataset = H5Dopen2(file, "/pulse_schedule/ids_properties&version_put&data_dictionary",
                             H5P_DEFAULT);
    CHECK(dataset >= 0);
    hid_t string_type = H5Dget_type(dataset);
    CHECK(string_type >= 0);
    char *observed = NULL;
    CHECK(H5Dread(dataset, string_type, H5S_ALL, H5S_ALL, H5P_DEFAULT, &observed) >= 0);
    CHECK(observed != NULL);
    CHECK(strcmp(observed, stored_version) == 0);
    CHECK(H5free_memory(observed) >= 0);
    CHECK(H5Tclose(string_type) >= 0);
    CHECK(H5Dclose(dataset) >= 0);
    CHECK(H5Fclose(file) >= 0);
}

static int open_pulse(int action) {
    char uri[1100];
    int length = snprintf(uri, sizeof uri, "imas:hdf5?path=%s", pulse_dir);
    CHECK(length > 0 && (size_t)length < sizeof uri);
    int context = -1;
    CHECK_OK(al_begin_dataentry_action(uri, action, &context));
    return context;
}

static void seed_stored_pulse(const char *stored_version, const char *stored_path,
                              int seed_converted_leaf) {
    make_pulse_directory();
    int pulse_context = open_pulse(CREATE_PULSE);
    int operation_context = -1;
    CHECK_OK(al_begin_global_action(pulse_context, "pulse_schedule", "", WRITE_OP,
                                    &operation_context));
    if (seed_converted_leaf) {
        double value = 12.5;
        CHECK_OK(al_write_data(operation_context, stored_path, "", &value, DOUBLE_DATA, 0,
                               NULL));
    }
    CHECK_OK(al_end_action(operation_context));
    CHECK_OK(al_close_pulse(pulse_context, CLOSE_PULSE));
    write_stamp(stored_version);
    write_unrelated_value();
}

static void scenario_read(const historical_direction *direction) {
    CHECK_OK(imas_mvdd_set_hli_dd_version(direction->hli_version));
    seed_stored_pulse(direction->stored_version, direction->stored_core_path, 1);
    int pulse_context = open_pulse(OPEN_PULSE);
    int operation_context = -1;
    CHECK_OK(al_begin_global_action(pulse_context, "pulse_schedule", "", READ_OP,
                                    &operation_context));
    double value = 0.0;
    void *buffer = &value;
    int shape[MAXDIM] = {0};
    CHECK_OK(
        al_read_data(operation_context, direction->caller_path, "", &buffer, DOUBLE_DATA, 0, shape));
    CHECK(buffer == &value);
    CHECK(value == 12.5);
    CHECK_OK(al_end_action(operation_context));
    CHECK_OK(al_close_pulse(pulse_context, CLOSE_PULSE));
    check_stamp(direction->stored_version);
    CHECK(read_stored_scalar(direction->stored_disk_path) == 12.5);
    CHECK(read_stored_scalar("/pulse_schedule/unrelated") == 99.0);
}

static void scenario_write(const historical_direction *direction) {
    CHECK_OK(imas_mvdd_set_hli_dd_version(direction->hli_version));
    seed_stored_pulse(direction->stored_version, direction->stored_core_path, 0);
    int pulse_context = open_pulse(OPEN_PULSE);
    int operation_context = -1;
    CHECK_OK(al_begin_global_action(pulse_context, "pulse_schedule", "", WRITE_OP,
                                    &operation_context));
    double value = 42.0;
    CHECK_OK(
        al_write_data(operation_context, direction->caller_path, "", &value, DOUBLE_DATA, 0, NULL));
    CHECK_OK(al_end_action(operation_context));
    CHECK_OK(al_close_pulse(pulse_context, CLOSE_PULSE));
    CHECK(read_stored_scalar(direction->stored_disk_path) == 42.0);
    check_stamp(direction->stored_version);
    CHECK(read_stored_scalar("/pulse_schedule/unrelated") == 99.0);
}

static void scenario_delete(const historical_direction *direction) {
    CHECK_OK(imas_mvdd_set_hli_dd_version(direction->hli_version));
    seed_stored_pulse(direction->stored_version, direction->stored_core_path, 1);
    int pulse_context = open_pulse(OPEN_PULSE);
    int operation_context = -1;
    CHECK_OK(al_begin_global_action(pulse_context, "pulse_schedule", "", WRITE_OP,
                                    &operation_context));
    CHECK_OK(al_delete_data(operation_context, direction->caller_path));
    CHECK_OK(al_end_action(operation_context));
    CHECK_OK(al_close_pulse(pulse_context, CLOSE_PULSE));
    CHECK(!dataset_exists(direction->stored_disk_path));
    check_stamp(direction->stored_version);
    CHECK(read_stored_scalar("/pulse_schedule/unrelated") == 99.0);
}

static void scenario_forward_read(void) {
    scenario_read(&FORWARD);
}

static void scenario_reverse_read(void) {
    scenario_read(&REVERSE);
}

static void scenario_forward_write(void) {
    scenario_write(&FORWARD);
}

static void scenario_reverse_write(void) {
    scenario_write(&REVERSE);
}

static void scenario_forward_delete(void) {
    scenario_delete(&FORWARD);
}

static void scenario_reverse_delete(void) {
    scenario_delete(&REVERSE);
}

static void scenario_forward_structure_delete(void) {
    historical_direction structure = FORWARD;
    structure.caller_path = "ec/launcher";
    scenario_delete(&structure);
}

static void scenario_reverse_structure_delete(void) {
    historical_direction structure = REVERSE;
    structure.caller_path = "ec/antenna";
    scenario_delete(&structure);
}

static const shim_test_scenario SCENARIOS[] = {
    {"graph-pulse-schedule-forward-read", scenario_forward_read},
    {"graph-pulse-schedule-reverse-read", scenario_reverse_read},
    {"graph-pulse-schedule-forward-write", scenario_forward_write},
    {"graph-pulse-schedule-reverse-write", scenario_reverse_write},
    {"graph-pulse-schedule-forward-delete", scenario_forward_delete},
    {"graph-pulse-schedule-reverse-delete", scenario_reverse_delete},
    {"graph-pulse-schedule-forward-structure-delete", scenario_forward_structure_delete},
    {"graph-pulse-schedule-reverse-structure-delete", scenario_reverse_structure_delete},
};

int main(int argc, char **argv) { return RUN_NAMED_SCENARIO(argc, argv, SCENARIOS); }
