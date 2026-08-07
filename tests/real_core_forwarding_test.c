/* Optional integration test for issue #6: exercise every newly mirrored
 * symbol through the shim against a real IMAS-Core libal. Unlike the
 * recording-stub test, this test uses only the public C ABI and legal context
 * lifecycles. CMake registers it only when matching Core headers and library
 * are supplied explicitly. */

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
#include <imas_mvdd_loader.h>

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

static void write_int_scalar(int ctx, const char *field, int value) {
    CHECK_OK(al_write_data(ctx, field, "", &value, INTEGER_DATA, 0, NULL));
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

/* The reentry calls bypass plugin dispatch and talk to the backend directly.
 * They are therefore safe to exercise without shipping a test plugin. The
 * other eleven issue-#7 symbols are resolved by the shim's all-or-nothing
 * binding before the first call in this process, so this real-Core tracer
 * also fails if any of their real symbols are absent. */
static void check_plugin_reentry(int pulse_ctx) {
    int op_ctx = -1;
    int value = 314;
    CHECK_OK(al_plugin_begin_global_action(pulse_ctx, "plugin_probe", "", WRITE_OP, &op_ctx));
    CHECK_OK(al_plugin_write_data(op_ctx, "leaf", "", &value, INTEGER_DATA, 0, NULL));
    CHECK_OK(al_plugin_end_action(op_ctx));

    /* Starting and ending the slice proves the reentry's lifecycle path;
     * the recording stub exercises its data-read sibling with controlled
     * buffers. The real-Core test's initial binding resolves all seventeen
     * issue-#7 symbols before any of these calls. */
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

int main(void) {
    CHECK(getenv("IMAS_CORE_LIBRARY") != NULL);

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

    check_slice_read(pulse_ctx);
    check_timerange_read(pulse_ctx);
    check_arraystruct_read(pulse_ctx);
    check_plugin_reentry(pulse_ctx);

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
    remove_temp_file(pulse_dir, "master.h5");
    CHECK(rmdir(pulse_dir) == 0);
    CHECK(rmdir(temp_dir) == 0);

    printf("real_core_forwarding_test: issue-6 and plugin-reentry symbols completed a real HDF5 lifecycle\n");
    return EXIT_SUCCESS;
}
