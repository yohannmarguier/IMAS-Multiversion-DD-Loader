#include <pthread.h>
#include "../support/shim_test_support.h"

static void check_file_entry(const char *contents, const char *operation,
                             const char *fidelity, const char *path) {
    char expected[1024];
    snprintf(expected, sizeof expected,
             "imas:hdf5?path=/tmp/pulse\tequilibrium\t3.39.0\t4.1.1\t%s\t%s\t%s\n",
             operation, fidelity, path);
    const char *entry = strstr(contents, expected);
    CHECK(entry != NULL);
    CHECK(strstr(entry + 1, expected) == NULL);
}

static void check_buffer_boundaries(int ctx, const char *path) {
    char buffer[256];
    int lengths[] = {-1, 0, (int)strlen(path)};
    for (size_t i = 0; i < sizeof lengths / sizeof *lengths; ++i) {
        memset(buffer, 'X', sizeof buffer);
        int verdict = -77;
        CHECK(imas_mvdd_context_loss_at(ctx, 0, buffer, lengths[i], &verdict).code ==
              IMAS_MVDD_CONVERSION_ERROR);
        CHECK(verdict == -77);
        for (size_t j = 0; j < sizeof buffer; ++j) CHECK(buffer[j] == 'X');
    }
    int verdict = -77;
    CHECK_OK(imas_mvdd_context_loss_at(ctx, 0, buffer, (int)strlen(path) + 1, &verdict));
    CHECK(strcmp(buffer, path) == 0);
    CHECK(buffer[strlen(path) + 1] == 'X');
    CHECK(verdict == IMAS_MVDD_FIDELITY_LOSSY);
}

static void scenario_operations_survive_close(void) {
    clear_loss_log_directory();
    int root = open_mismatched_equilibrium();
    int child = -1, extent = 0;
    CHECK_OK(al_begin_arraystruct_action(root, "time_slice", "", &extent, &child));
    CHECK(single_loss_log_path_or_null() == NULL);
    const char *absent = "time_slice/boundary/phi";
    void *data = (void *)1;
    int size = 1;
    CHECK_OK(al_read_data(child, "boundary/phi", "", &data, IMAS_DOUBLE_DATA, 1, &size));
    CHECK(data == NULL && size == 0);
    check_loss_at(root, 0, absent, IMAS_MVDD_FIDELITY_LOSSY, IMAS_MVDD_LOSS_OPERATION_READ);
    check_loss_at(child, 0, absent, IMAS_MVDD_FIDELITY_LOSSY, IMAS_MVDD_LOSS_OPERATION_READ);
    check_buffer_boundaries(child, absent);

    const char *candidates[] = {"time_slice/profiles_2d/b_field_phi",
                              "time_slice/profiles_2d/b_field_tor",
                              "time_slice/profiles_2d/b_tor"};
    double value = 42.0;
    size = 1;
    CHECK_OK(al_write_data(root, candidates[0], "", &value, IMAS_DOUBLE_DATA, 1, &size));
    for (int i = 1; i < 3; ++i)
        check_loss_at(child, i, candidates[i], IMAS_MVDD_FIDELITY_POTENTIALLY_LOSSY,
                      IMAS_MVDD_LOSS_OPERATION_WRITE);
    CHECK_OK(al_delete_data(root, candidates[0]));
    for (int i = 0; i < 3; ++i)
        check_loss_at(root, i + 3, candidates[i], IMAS_MVDD_FIDELITY_POTENTIALLY_LOSSY,
                      IMAS_MVDD_LOSS_OPERATION_DELETE);

    CHECK(al_write_data(child, "boundary/phi", "", &value, IMAS_DOUBLE_DATA, 1, &size).code ==
          IMAS_MVDD_CONVERSION_ERROR);
    CHECK(al_delete_data(child, "boundary/phi").code == IMAS_MVDD_CONVERSION_ERROR);
    const char *retyped = "grids_ggd/grid/space/coordinates_type";
    CHECK(al_read_data(root, retyped, "", &data, IMAS_DOUBLE_DATA, 1, &size).code ==
          IMAS_MVDD_CONVERSION_ERROR);
    check_loss_at(root, 6, absent, IMAS_MVDD_FIDELITY_UNMAPPABLE, IMAS_MVDD_LOSS_OPERATION_WRITE);
    check_loss_at(root, 7, absent, IMAS_MVDD_FIDELITY_UNMAPPABLE, IMAS_MVDD_LOSS_OPERATION_DELETE);
    check_loss_at(root, 8, retyped, IMAS_MVDD_FIDELITY_UNMAPPABLE, IMAS_MVDD_LOSS_OPERATION_READ);
    CHECK(loss_count(child) == 9);

    char *before = read_loss_log();
    check_file_entry(before, "read", "LOSSY", absent);
    for (int i = 1; i < 3; ++i)
        check_file_entry(before, "write", "POTENTIALLY_LOSSY", candidates[i]);
    for (int i = 0; i < 3; ++i)
        check_file_entry(before, "delete", "POTENTIALLY_LOSSY", candidates[i]);
    check_file_entry(before, "write", "UNMAPPABLE", absent);
    check_file_entry(before, "delete", "UNMAPPABLE", absent);
    check_file_entry(before, "read", "UNMAPPABLE", retyped);
    CHECK_OK(al_end_action(root));
    CHECK(loss_count(root) == 0 && loss_count(child) == 0 && loss_count(-999) == 0);
    CHECK_OK(al_end_action(child));
    char *after = read_loss_log();
    CHECK(strcmp(before, after) == 0);
    free(before);
    free(after);
}

struct worker_input { int ctx; const char *path; };
static pthread_mutex_t start_mutex = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t start_condition = PTHREAD_COND_INITIALIZER;
static int started = 0;

static void *refuse_writes(void *opaque) {
    struct worker_input *input = opaque;
    CHECK(pthread_mutex_lock(&start_mutex) == 0);
    while (!started) CHECK(pthread_cond_wait(&start_condition, &start_mutex) == 0);
    CHECK(pthread_mutex_unlock(&start_mutex) == 0);
    for (int i = 0; i < 16; ++i) {
        double value = 1.0;
        int size = 1;
        CHECK(al_write_data(input->ctx, input->path, "", &value, IMAS_DOUBLE_DATA, 1, &size).code ==
              IMAS_MVDD_CONVERSION_ERROR);
    }
    return NULL;
}

static void scenario_concurrent_first_loss(void) {
    clear_loss_log_directory();
    int root = open_mismatched_equilibrium();
    const char *paths[] = {"time_slice/boundary/phi", "grids_ggd/grid/space/coordinates_type"};
    pthread_t threads[8];
    struct worker_input inputs[8];
    CHECK(single_loss_log_path_or_null() == NULL);
    int writes_before = int_from_stub("recording_stub_write_call_count");
    for (int i = 0; i < 8; ++i) {
        inputs[i] = (struct worker_input){root, paths[i % 2]};
        CHECK(pthread_create(&threads[i], NULL, refuse_writes, &inputs[i]) == 0);
    }
    CHECK(pthread_mutex_lock(&start_mutex) == 0);
    started = 1;
    CHECK(pthread_cond_broadcast(&start_condition) == 0);
    CHECK(pthread_mutex_unlock(&start_mutex) == 0);
    for (int i = 0; i < 8; ++i) CHECK(pthread_join(threads[i], NULL) == 0);
    /* Refusals never enter the recording Core, whose counters are not a
     * concurrent API. Only the shim's registry and file writer are exercised. */
    CHECK(int_from_stub("recording_stub_write_call_count") == writes_before);
    CHECK(loss_count(root) == 128);
    char *contents = read_loss_log();
    for (int i = 0; i < 2; ++i) check_file_entry(contents, "write", "UNMAPPABLE", paths[i]);
    free(contents);
    CHECK_OK(al_end_action(root));
}

int main(int argc, char **argv) {
    const shim_test_scenario scenarios[] = {
        {"operations-survive-close", scenario_operations_survive_close},
        {"concurrent-first-loss", scenario_concurrent_first_loss},
    };
    return RUN_NAMED_SCENARIO(argc, argv, scenarios);
}
