/* Required integration test for issues #6 through #8: exercise every mirrored
 * symbol through the shim against the CMake-acquired IMAS-Core libal. Unlike
 * the recording-stub test, this test uses only the public C ABI and legal
 * context lifecycles. */

#if defined(__APPLE__)
#define _DARWIN_C_SOURCE
#else
#define _XOPEN_SOURCE 700
#endif

#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include <al_const.h>
#include <hdf5.h>
#include <imas_mvdd_loader.h>

#ifndef REAL_CORE_TEST_PLUGIN_DIR
#error "REAL_CORE_TEST_PLUGIN_DIR must name the directory containing the fixture plugin"
#endif
#ifndef REAL_CORE_TEST_PLUGIN_NAME
#error "REAL_CORE_TEST_PLUGIN_NAME must name the fixture plugin"
#endif

#define CHECK(condition)                                                        \
    do {                                                                        \
        if (!(condition)) {                                                     \
            fprintf(stderr, "check failed at %s:%d: %s\n", __FILE__, __LINE__, \
                    #condition);                                                \
            exit(EXIT_FAILURE);                                                 \
        }                                                                       \
    } while (0)

static void check_ok(al_status_t status, const char *expression, int line) {
    if (status.code != 0) {
        fprintf(stderr, "IMAS-Core call failed at %s:%d: %s: code=%d message=%s\n", __FILE__,
                line, expression, status.code, status.message);
        exit(EXIT_FAILURE);
    }
}

#define CHECK_OK(expression) check_ok((expression), #expression, __LINE__)

static int contains_int(const int *values, int size, int expected) {
    for (int i = 0; i < size; ++i) {
        if (values[i] == expected) {
            return 1;
        }
    }
    return 0;
}

static int contains_path(char **paths, int size, const char *expected) {
    for (int i = 0; i < size; ++i) {
        if (strcmp(paths[i], expected) == 0) {
            return 1;
        }
    }
    return 0;
}

static void remove_temp_file(const char *pulse_dir, const char *name) {
    char path[1024];
    int path_length = snprintf(path, sizeof path, "%s/%s", pulse_dir, name);
    CHECK(path_length > 0 && (size_t)path_length < sizeof path);
    CHECK(unlink(path) == 0 || errno == ENOENT);
}

static int file_contains(const char *path, const char *needle) {
    FILE *file = fopen(path, "r");
    CHECK(file != NULL);
    char contents[1024] = {0};
    size_t size = fread(contents, 1, sizeof contents - 1, file);
    CHECK(!ferror(file));
    CHECK(fclose(file) == 0);
    contents[size] = '\0';
    return strstr(contents, needle) != NULL;
}

static void check_logged_parameter(const char *path, const char *name, int datatype,
                                   const char *value) {
    char expected[256];
    int length = snprintf(expected, sizeof expected, "%s|%d|0|%s", name, datatype, value);
    CHECK(length > 0 && (size_t)length < sizeof expected);
    CHECK(file_contains(path, expected));
}

static void write_int_scalar(int ctx, const char *field, int value) {
    CHECK_OK(al_write_data(ctx, field, "", &value, INTEGER_DATA, 0, NULL));
}

/* IMAS-Core's public write API cannot create or update this scalar metadata
 * dataset, so seed stored DD-version metadata directly after the write action
 * has closed.
 * Slice/time-range calls below still traverse only the public shim/Core ABI. */
static void set_dd_version_stamp(const char *ids_file, const char *version) {
    hid_t file = H5Fopen(ids_file, H5F_ACC_RDWR, H5P_DEFAULT);
    CHECK(file >= 0);
    hid_t group = H5Gopen2(file, "/magnetics", H5P_DEFAULT);
    CHECK(group >= 0);
    const char *dataset_name = "ids_properties&version_put&data_dictionary";
    htri_t exists = H5Lexists(group, dataset_name, H5P_DEFAULT);
    CHECK(exists >= 0);
    hid_t datatype = H5I_INVALID_HID;
    hid_t dataset = H5I_INVALID_HID;
    if (exists) {
        dataset = H5Dopen2(group, dataset_name, H5P_DEFAULT);
        CHECK(dataset >= 0);
        datatype = H5Dget_type(dataset);
        CHECK(datatype >= 0);
    } else {
        datatype = H5Tcopy(H5T_C_S1);
        CHECK(datatype >= 0);
        CHECK(H5Tset_size(datatype, H5T_VARIABLE) >= 0);
        CHECK(H5Tset_cset(datatype, H5T_CSET_UTF8) >= 0);
        hid_t dataspace = H5Screate(H5S_SCALAR);
        CHECK(dataspace >= 0);
        dataset = H5Dcreate2(group, dataset_name, datatype, dataspace, H5P_DEFAULT, H5P_DEFAULT,
                             H5P_DEFAULT);
        CHECK(H5Sclose(dataspace) >= 0);
    }
    CHECK(dataset >= 0);
    CHECK(H5Tis_variable_str(datatype) > 0);
    const char *value = version;
    CHECK(H5Dwrite(dataset, datatype, H5S_ALL, H5S_ALL, H5P_DEFAULT, &value) >= 0);
    CHECK(H5Tclose(datatype) >= 0);
    CHECK(H5Dclose(dataset) >= 0);
    CHECK(H5Gclose(group) >= 0);
    CHECK(H5Fclose(file) >= 0);
}

static int read_int_scalar(int ctx, const char *field) {
    int value = -1;
    int shape[MAXDIM] = {0};
    void *buffer = &value;
    CHECK_OK(al_read_data(ctx, field, "", &buffer, INTEGER_DATA, 0, shape));
    return value;
}

static void seed_dynamic_signal(int op_ctx) {
    int homogeneous_time = 1;
    double time[2] = {1.0, 2.0};
    double signal[2] = {10.0, 20.0};
    int shape[1] = {2};

    write_int_scalar(op_ctx, "ids_properties/homogeneous_time", homogeneous_time);
    CHECK_OK(al_write_data(op_ctx, "time", "time", time, DOUBLE_DATA, 1, shape));
    CHECK_OK(al_write_data(op_ctx, "ip", "time", signal, DOUBLE_DATA, 1, shape));
}

static void check_slice_read(int pulse_ctx) {
    int op_ctx = -1;
    CHECK_OK(al_begin_slice_action(pulse_ctx, "magnetics", READ_OP, 1.4, CLOSEST_INTERP,
                                   &op_ctx));

    void *buffer = NULL;
    int shape[MAXDIM] = {0};
    CHECK_OK(al_read_data(op_ctx, "ip", "time", &buffer, DOUBLE_DATA, 1, shape));
    CHECK(buffer != NULL);
    CHECK(shape[0] == 1);
    CHECK(((double *)buffer)[0] == 10.0);
    free(buffer);
    CHECK_OK(al_end_action(op_ctx));
}

static void check_timerange_read(int pulse_ctx) {
    int op_ctx = -1;
    double dtime = 0.0;
    int dtime_shape = 0;
    CHECK_OK(al_begin_timerange_action(pulse_ctx, "magnetics", READ_OP, 1.0, 2.0, &dtime,
                                       &dtime_shape, UNDEFINED_INTERP, &op_ctx));

    void *buffer = NULL;
    int shape[MAXDIM] = {0};
    CHECK_OK(al_read_data(op_ctx, "ip", "time", &buffer, DOUBLE_DATA, 1, shape));
    CHECK(buffer != NULL);
    CHECK(shape[0] == 2);
    CHECK(((double *)buffer)[0] == 10.0);
    CHECK(((double *)buffer)[1] == 20.0);
    free(buffer);
    CHECK_OK(al_end_action(op_ctx));
}

static void check_slice_refuses_malformed_stamp(int pulse_ctx) {
    int op_ctx = -1;
    al_status_t status =
        al_begin_slice_action(pulse_ctx, "magnetics", READ_OP, 1.4, CLOSEST_INTERP, &op_ctx);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(strstr(status.message, "malformed DD-version stamp") != NULL);
}

static void check_timerange_refuses_malformed_stamp(int pulse_ctx) {
    int op_ctx = -1;
    double dtime = 0.0;
    int dtime_shape = 0;
    al_status_t status = al_begin_timerange_action(pulse_ctx, "magnetics", READ_OP, 1.0, 2.0,
                                                    &dtime, &dtime_shape, UNDEFINED_INTERP,
                                                    &op_ctx);
    CHECK(status.code == IMAS_MVDD_CONVERSION_ERROR);
    CHECK(strstr(status.message, "malformed DD-version stamp") != NULL);
}

static void check_arraystruct_read(int pulse_ctx) {
    int op_ctx = -1;
    CHECK_OK(al_begin_global_action(pulse_ctx, "magnetics", "", READ_OP, &op_ctx));

    int size = 0;
    int aos_ctx = -1;
    CHECK_OK(al_begin_arraystruct_action(op_ctx, "elements", "", &size, &aos_ctx));
    CHECK(size == 2);
    CHECK(read_int_scalar(aos_ctx, "value") == 101);
    CHECK_OK(al_iterate_over_arraystruct(aos_ctx, 1));
    CHECK(read_int_scalar(aos_ctx, "value") == 202);
    CHECK_OK(al_end_action(aos_ctx));
    CHECK_OK(al_end_action(op_ctx));
}

/* The utility/version calls have no lifecycle of their own, except that the
 * backend-id accessor needs an open context. Exercise each through a real
 * HDF5 pulse so the issue-#8 seam is covered against Core as well as the
 * recording stub. */
static void check_utility_and_version_accessors(int pulse_ctx) {
    int backend_id = -1;
    CHECK_OK(al_get_backendID(pulse_ctx, &backend_id));
    CHECK(backend_id == HDF5_BACKEND);

    char *legacy_uri = NULL;
    CHECK_OK(al_build_uri_from_legacy_parameters(HDF5_BACKEND, 44, 5, "mvdd-user",
                                                 "mvdd-tokamak", "4.1.1", "", &legacy_uri));
    CHECK(legacy_uri != NULL && legacy_uri[0] != '\0');
    /* IMAS-Core's ownership contract for this output is undocumented (see
     * the inventory), so do not guess at a deallocator in this probe. */

    CHECK(strcmp(const2str(HDF5_BACKEND), "HDF5_BACKEND") == 0);
    CHECK(strcmp(err2str(BACKEND_ERR), "BACKEND_ERR") == 0);
    CHECK(getALVersion() != NULL && getALVersion()[0] != '\0');
    CHECK(strcmp(getDDVersion(), "!!DEPRECATED!!") == 0);
}

/* The reentry calls bypass plugin dispatch and talk to the backend directly. */
static void check_plugin_reentry(int pulse_ctx) {
    int op_ctx = -1;
    int value = 314;
    CHECK_OK(al_plugin_begin_global_action(pulse_ctx, "plugin_probe", "", WRITE_OP, &op_ctx));
    CHECK_OK(al_plugin_write_data(op_ctx, "leaf", "", &value, INTEGER_DATA, 0, NULL));
    CHECK_OK(al_plugin_end_action(op_ctx));

    CHECK_OK(al_plugin_begin_global_action(pulse_ctx, "plugin_probe", "", READ_OP, &op_ctx));
    int read_value = -1;
    int read_shape[MAXDIM] = {0};
    void *read_buffer = &read_value;
    CHECK_OK(al_plugin_read_data(op_ctx, "leaf", "", &read_buffer, INTEGER_DATA, 0,
                                 read_shape));
    CHECK(read_value == 314);
    CHECK_OK(al_plugin_end_action(op_ctx));

    /* Starting and ending the slice proves that reentry lifecycle independently
     * of the global-action read/write path above. */
    CHECK_OK(al_plugin_begin_slice_action(pulse_ctx, "magnetics", READ_OP, 1.4,
                                          CLOSEST_INTERP, &op_ctx));
    CHECK_OK(al_plugin_end_action(op_ctx));

    int size = 1;
    int aos_ctx = -1;
    CHECK_OK(al_plugin_begin_global_action(pulse_ctx, "plugin_aos", "", WRITE_OP, &op_ctx));
    CHECK_OK(al_plugin_begin_arraystruct_action(op_ctx, "elements", "", &size, &aos_ctx));
    value = 271;
    CHECK_OK(al_plugin_write_data(aos_ctx, "value", "", &value, INTEGER_DATA, 0, NULL));
    CHECK_OK(al_plugin_end_action(aos_ctx));
    CHECK_OK(al_plugin_end_action(op_ctx));
}

/* Drive all eleven plugin-management/configuration exports across the shim's
 * real-Core boundary. The loadable fixture makes registration legal and logs
 * the setter arguments so this checks forwarding, not only symbol presence. */
static void check_plugin_management(int pulse_ctx, const char *log_path) {
    CHECK(setenv("IMAS_AL_ENABLE_PLUGINS", "TRUE", 1) == 0);
    CHECK(setenv("IMAS_AL_PLUGINS", REAL_CORE_TEST_PLUGIN_DIR, 1) == 0);
    CHECK(setenv("IMAS_MVDD_TEST_PLUGIN_LOG", log_path, 1) == 0);

    bool registered = true;
    CHECK_OK(al_is_plugin_registered(REAL_CORE_TEST_PLUGIN_NAME, &registered));
    CHECK(!registered);
    CHECK_OK(al_register_plugin(REAL_CORE_TEST_PLUGIN_NAME));
    CHECK_OK(al_is_plugin_registered(REAL_CORE_TEST_PLUGIN_NAME, &registered));
    CHECK(registered);

    int generic_value = 7;
    CHECK_OK(al_setvalue_parameter_plugin("generic", INTEGER_DATA, 0, NULL,
                                          &generic_value, REAL_CORE_TEST_PLUGIN_NAME));
    CHECK_OK(al_setvalue_int_scalar_parameter_plugin("integer", 42,
                                                      REAL_CORE_TEST_PLUGIN_NAME));
    CHECK_OK(al_setvalue_double_scalar_parameter_plugin("double", 1.5,
                                                         REAL_CORE_TEST_PLUGIN_NAME));
    check_logged_parameter(log_path, "generic", INTEGER_DATA, "7");
    check_logged_parameter(log_path, "integer", INTEGER_DATA, "42");
    check_logged_parameter(log_path, "double", DOUBLE_DATA, "1.5");

    const char *bound_path = "plugin_metadata:0/leaf";
    CHECK_OK(al_bind_plugin(bound_path, REAL_CORE_TEST_PLUGIN_NAME));

    int op_ctx = -1;
    CHECK_OK(al_begin_global_action(pulse_ctx, "plugin_metadata", "", WRITE_OP, &op_ctx));
    CHECK_OK(al_write_plugins_metadata(op_ctx));
    CHECK_OK(al_end_action(op_ctx));

    CHECK_OK(al_begin_global_action(pulse_ctx, "plugin_metadata", "", READ_OP, &op_ctx));
    CHECK_OK(al_bind_readback_plugins(op_ctx));
    CHECK_OK(al_unbind_readback_plugins(op_ctx));
    CHECK_OK(al_end_action(op_ctx));

    CHECK_OK(al_unbind_plugin(bound_path, REAL_CORE_TEST_PLUGIN_NAME));
    /* A second bind fails if unbind was accidentally forwarded elsewhere. */
    CHECK_OK(al_bind_plugin(bound_path, REAL_CORE_TEST_PLUGIN_NAME));
    CHECK_OK(al_unregister_plugin(REAL_CORE_TEST_PLUGIN_NAME));
    CHECK_OK(al_is_plugin_registered(REAL_CORE_TEST_PLUGIN_NAME, &registered));
    CHECK(!registered);

    CHECK(unsetenv("IMAS_MVDD_TEST_PLUGIN_LOG") == 0);
    CHECK(unsetenv("IMAS_AL_PLUGINS") == 0);
    CHECK(unsetenv("IMAS_AL_ENABLE_PLUGINS") == 0);
}

int main(void) {
    CHECK(getenv("IMAS_CORE_LIBRARY") == NULL);
    CHECK_OK(imas_mvdd_set_hli_dd_version("4.1.1"));

    char temp_dir[] = "/tmp/imas-mvdd-real-core-XXXXXX";
    CHECK(mkdtemp(temp_dir) != NULL);

    char uri[1024];
    char pulse_dir[1024];
    int pulse_dir_length = snprintf(pulse_dir, sizeof pulse_dir, "%s/pulse.h5", temp_dir);
    CHECK(pulse_dir_length > 0 && (size_t)pulse_dir_length < sizeof pulse_dir);
    int uri_length = snprintf(uri, sizeof uri, "imas:hdf5?path=%s", pulse_dir);
    CHECK(uri_length > 0 && (size_t)uri_length < sizeof uri);

    int pulse_ctx = -1;
    CHECK_OK(al_begin_dataentry_action(uri, FORCE_CREATE_PULSE, &pulse_ctx));
    check_utility_and_version_accessors(pulse_ctx);

    /* Seed a dynamic signal and a two-element AOS. */
    int op_ctx = -1;
    CHECK_OK(al_begin_global_action(pulse_ctx, "magnetics", "", WRITE_OP, &op_ctx));
    seed_dynamic_signal(op_ctx);
    int aos_size = 2;
    int aos_ctx = -1;
    CHECK_OK(al_begin_arraystruct_action(op_ctx, "elements", "", &aos_size, &aos_ctx));
    write_int_scalar(aos_ctx, "value", 101);
    CHECK_OK(al_iterate_over_arraystruct(aos_ctx, 1));
    write_int_scalar(aos_ctx, "value", 202);
    CHECK_OK(al_end_action(aos_ctx));
    CHECK_OK(al_end_action(op_ctx));

    /* Create occurrence 2 so occurrence enumeration has a real oracle. */
    CHECK_OK(al_begin_global_action(pulse_ctx, "magnetics_2", "", WRITE_OP, &op_ctx));
    write_int_scalar(op_ctx, "leaf", 2);
    CHECK_OK(al_end_action(op_ctx));

    int *occurrences = NULL;
    int occurrence_count = -1;
    CHECK_OK(al_get_occurrences(pulse_ctx, "magnetics", &occurrences, &occurrence_count));
    CHECK(occurrence_count == 2);
    CHECK(contains_int(occurrences, occurrence_count, 0));
    CHECK(contains_int(occurrences, occurrence_count, 2));
    free(occurrences);

    char **paths = NULL;
    int path_count = -1;
    CHECK_OK(al_list_filled_paths(pulse_ctx, "magnetics", &paths, &path_count));
    CHECK(path_count >= 3);
    CHECK(contains_path(paths, path_count, "ids_properties/homogeneous_time"));
    CHECK(contains_path(paths, path_count, "time"));
    CHECK(contains_path(paths, path_count, "ip"));
    for (int i = 0; i < path_count; ++i) {
        free(paths[i]);
    }
    free(paths);

    char magnetics_file[1024];
    int magnetics_file_length =
        snprintf(magnetics_file, sizeof magnetics_file, "%s/magnetics.h5", pulse_dir);
    CHECK(magnetics_file_length > 0 && (size_t)magnetics_file_length < sizeof magnetics_file);
    set_dd_version_stamp(magnetics_file, "4.1.1");

    check_slice_read(pulse_ctx);
    check_timerange_read(pulse_ctx);
    check_arraystruct_read(pulse_ctx);
    /* Plugin reentry now performs the same version-stamp discovery as its
     * ordinary twins, so exercise its successful real-Core lifecycle while
     * magnetics still has its valid matching stamp. */
    check_plugin_reentry(pulse_ctx);

    set_dd_version_stamp(magnetics_file, "not-a-version");

    /* A present but invalid stamp must turn the next successful real-Core
     * open into a shim refusal. Both operation seams end the just-opened
     * context internally; the HLI therefore must not end either `op_ctx`. */
    check_slice_refuses_malformed_stamp(pulse_ctx);
    check_timerange_refuses_malformed_stamp(pulse_ctx);

    char plugin_log[1024];
    int plugin_log_length = snprintf(plugin_log, sizeof plugin_log, "%s/plugin.log", temp_dir);
    CHECK(plugin_log_length > 0 && (size_t)plugin_log_length < sizeof plugin_log);
    check_plugin_management(pulse_ctx, plugin_log);

    /* Isolate the documented HDF5 delete behavior from the data above. */
    CHECK_OK(al_begin_global_action(pulse_ctx, "delete_probe", "", WRITE_OP, &op_ctx));
    write_int_scalar(op_ctx, "leaf", 1);
    CHECK_OK(al_delete_data(op_ctx, "leaf"));
    CHECK_OK(al_end_action(op_ctx));

    CHECK_OK(al_close_pulse(pulse_ctx, CLOSE_PULSE));
    remove_temp_file(pulse_dir, "magnetics.h5");
    remove_temp_file(pulse_dir, "magnetics_2.h5");
    remove_temp_file(pulse_dir, "delete_probe.h5");
    remove_temp_file(pulse_dir, "plugin_probe.h5");
    remove_temp_file(pulse_dir, "plugin_aos.h5");
    remove_temp_file(pulse_dir, "plugin_metadata.h5");
    remove_temp_file(pulse_dir, "master.h5");
    CHECK(unlink(plugin_log) == 0);
    CHECK(rmdir(pulse_dir) == 0);
    CHECK(rmdir(temp_dir) == 0);

    printf("real_core_forwarding_test: issue-6, issue-7, and issue-8 symbols crossed real IMAS-Core\n");
    return EXIT_SUCCESS;
}
