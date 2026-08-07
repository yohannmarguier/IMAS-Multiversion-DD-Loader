/* The tracer bullet from issue #3: drives the shim's exported
 * al_context_info and asserts on what a recording stub -- standing in for
 * IMAS-Core -- actually received. Proves the shim exports al_context_info
 * *and* calls al_context_info, with its own definition never capturing the
 * outbound call.
 *
 * The stub is deliberately never linked into this executable: linking it
 * would give the linker two candidate definitions of al_context_info (the
 * shim's and the stub's) to choose between, exactly the ambiguity runtime
 * binding exists to avoid (see docs/adr/0001-runtime-binding-not-linking.md).
 * Instead this test opens the stub with its own dlopen call, purely to read
 * back its recorded state through introspection accessors that are not
 * part of the mirrored ABI. The dynamic loader maps a given shared object
 * once per process no matter how many times it is dlopen'd, so the shim's
 * handle and this test's handle observe the same recorded state.
 *
 * Each scenario is registered as its own ctest process (see CMakeLists.txt)
 * because resolution is cached for the process's lifetime: a scenario that
 * needs a fresh resolution needs a fresh process, not a fresh setenv().
 *
 * The supported and deliberately incompatible versions are supplied from
 * CMake's reading of IMAS_CORE_VERSION, the same pin consumed by build.rs. */

#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <imas_mvdd_loader.h>

#ifndef RECORDING_STUB_PATH
#error "RECORDING_STUB_PATH must be defined by the build (see CMakeLists.txt)"
#endif
#ifndef SUPPORTED_CORE_VERSION
#error "SUPPORTED_CORE_VERSION must come from IMAS_CORE_VERSION"
#endif
#ifndef INCOMPATIBLE_CORE_VERSION
#error "INCOMPATIBLE_CORE_VERSION must be defined by the build"
#endif

typedef int (*int_accessor_fn)(void);
typedef double (*double_accessor_fn)(void);
typedef const char *(*str_accessor_fn)(void);
typedef const void *(*ptr_accessor_fn)(void);

#define CHECK(condition)                                                        \
    do {                                                                        \
        if (!(condition)) {                                                     \
            fprintf(stderr, "check failed at %s:%d: %s\n", __FILE__, __LINE__, \
                    #condition);                                                \
            exit(EXIT_FAILURE);                                                 \
        }                                                                       \
    } while (0)

static void *dlsym_or_die(void *handle, const char *name) {
    void *symbol = dlsym(handle, name);
    if (symbol == NULL) {
        fprintf(stderr, "recording stub has no symbol '%s': %s\n", name, dlerror());
        abort();
    }
    return symbol;
}

static void *open_stub_for_introspection(void) {
    void *handle = dlopen(RECORDING_STUB_PATH, RTLD_NOW | RTLD_LOCAL);
    if (handle == NULL) {
        fprintf(stderr, "failed to dlopen the recording stub for introspection: %s\n", dlerror());
        abort();
    }
    return handle;
}

static void scenario_success(void) {
    void *stub = open_stub_for_introspection();
    int_accessor_fn call_count = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_call_count");
    int_accessor_fn last_ctx = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_last_ctx");
    int_accessor_fn version_call_count =
        (int_accessor_fn)dlsym_or_die(stub, "recording_stub_version_call_count");
    CHECK(call_count() == 0);

    char *info = NULL;
    al_status_t status = al_context_info(42, &info);

    CHECK(status.code == 0);
    CHECK(info != NULL);
    free(info);
    CHECK(call_count() == 1);
    CHECK(last_ctx() == 42);

    /* A second call reaches the stub again without re-resolving. */
    char *info2 = NULL;
    al_status_t status2 = al_context_info(7, &info2);
    CHECK(status2.code == 0);
    free(info2);
    CHECK(call_count() == 2);
    CHECK(last_ctx() == 7);
    CHECK(version_call_count() == 1);

    printf("runtime_binding_test success: the shim reached the stub, not itself\n");
}

static void scenario_version_drift(void) {
    void *stub = open_stub_for_introspection();
    int_accessor_fn version_call_count =
        (int_accessor_fn)dlsym_or_die(stub, "recording_stub_version_call_count");

    char *info = NULL;
    al_status_t status = al_context_info(42, &info);
    CHECK(status.code == 0);
    free(info);
    CHECK(version_call_count() == 1);

    char *info2 = NULL;
    al_status_t status2 = al_context_info(7, &info2);
    CHECK(status2.code == 0);
    free(info2);
    CHECK(version_call_count() == 1);

    printf("runtime_binding_test version-drift: recorded and tolerated once\n");
}

static void scenario_version_mismatch(void) {
    void *stub = open_stub_for_introspection();
    int_accessor_fn call_count = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_call_count");

    char *info = NULL;
    al_status_t status = al_context_info(1, &info);

    CHECK(status.code != 0);
    CHECK(strstr(status.message, SUPPORTED_CORE_VERSION) != NULL);
    CHECK(strstr(status.message, INCOMPATIBLE_CORE_VERSION) != NULL);
    CHECK(strstr(status.message, "IMAS_CORE_LIBRARY") != NULL);
    /* The mismatch must fail resolution before ever forwarding the call. */
    CHECK(call_count() == 0);

    printf("runtime_binding_test version-mismatch: resolution failed before forwarding\n");
}

static void scenario_null_version(void) {
    void *stub = open_stub_for_introspection();
    int_accessor_fn call_count = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_call_count");

    char *info = NULL;
    al_status_t status = al_context_info(1, &info);

    CHECK(status.code != 0);
    CHECK(strstr(status.message, "getALVersion") != NULL);
    CHECK(strstr(status.message, "null") != NULL);
    CHECK(strstr(status.message, "IMAS_CORE_LIBRARY") != NULL);
    CHECK(call_count() == 0);

    printf("runtime_binding_test null-version: resolution failed safely\n");
}

static void scenario_missing_library(void) {
    char *info = NULL;
    al_status_t status = al_context_info(1, &info);

    CHECK(status.code != 0);
    CHECK(status.message[0] != '\0');
    CHECK(strstr(status.message, "IMAS_CORE_LIBRARY") != NULL);

    printf("runtime_binding_test missing-library: status=%d message=%s\n", status.code,
           status.message);
}

static void scenario_bare_soname(void) {
    /* No IMAS_CORE_LIBRARY override here (see CMakeLists.txt): the shim
     * must locate IMAS-Core by its bare soname through the loader's normal
     * search path, which ctest points at the stub via LD_LIBRARY_PATH /
     * DYLD_LIBRARY_PATH. */
    char *info = NULL;
    al_status_t status = al_context_info(11, &info);
    CHECK(status.code == 0);
    free(info);

    void *stub = open_stub_for_introspection();
    int_accessor_fn last_ctx = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_last_ctx");
    CHECK(last_ctx() == 11);

    printf("runtime_binding_test bare-soname: resolved IMAS-Core through the loader's search path\n");
}

/* Issue #6: the data-entry, action-lifecycle and data-operation seams.
 * Drives each of the thirteen forwarded symbols with distinct sentinel
 * arguments and asserts, against the recording stub, that every argument
 * arrived unmodified and every output the stub produced comes back through
 * the shim unmodified too. */
static void scenario_verbatim_forwarding(void) {
    void *stub = open_stub_for_introspection();

    /* al_begin_dataentry_action */
    {
        int_accessor_fn call_count =
            (int_accessor_fn)dlsym_or_die(stub, "recording_stub_dataentry_call_count");
        str_accessor_fn uri = (str_accessor_fn)dlsym_or_die(stub, "recording_stub_dataentry_uri");
        int_accessor_fn mode = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_dataentry_mode");

        int dectxID = -1;
        al_status_t status = al_begin_dataentry_action("imas:hdf5?path=/tmp/pulse", 7, &dectxID);
        CHECK(status.code == 0);
        CHECK(call_count() == 1);
        CHECK(strcmp(uri(), "imas:hdf5?path=/tmp/pulse") == 0);
        CHECK(mode() == 7);
        CHECK(dectxID == 1001);
    }

    /* al_close_pulse */
    {
        int_accessor_fn call_count =
            (int_accessor_fn)dlsym_or_die(stub, "recording_stub_close_pulse_call_count");
        int_accessor_fn ctx = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_close_pulse_ctx");
        int_accessor_fn mode =
            (int_accessor_fn)dlsym_or_die(stub, "recording_stub_close_pulse_mode");

        al_status_t status = al_close_pulse(1001, 1 /* CLOSE_PULSE */);
        CHECK(status.code == 0);
        CHECK(call_count() == 1);
        CHECK(ctx() == 1001);
        CHECK(mode() == 1);
    }

    /* al_begin_global_action */
    {
        int_accessor_fn call_count =
            (int_accessor_fn)dlsym_or_die(stub, "recording_stub_global_call_count");
        int_accessor_fn pctx_id = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_global_pctx_id");
        str_accessor_fn dataobjectname =
            (str_accessor_fn)dlsym_or_die(stub, "recording_stub_global_dataobjectname");
        str_accessor_fn datapath =
            (str_accessor_fn)dlsym_or_die(stub, "recording_stub_global_datapath");
        int_accessor_fn rwmode = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_global_rwmode");

        int octxID = -1;
        al_status_t status =
            al_begin_global_action(1001, "core_profiles", "core_profiles/profiles_1d", 2, &octxID);
        CHECK(status.code == 0);
        CHECK(call_count() == 1);
        CHECK(pctx_id() == 1001);
        CHECK(strcmp(dataobjectname(), "core_profiles") == 0);
        CHECK(strcmp(datapath(), "core_profiles/profiles_1d") == 0);
        CHECK(rwmode() == 2);
        CHECK(octxID == 2001);
    }

    /* al_begin_slice_action */
    {
        int_accessor_fn call_count =
            (int_accessor_fn)dlsym_or_die(stub, "recording_stub_slice_call_count");
        int_accessor_fn pctx_id = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_slice_pctx_id");
        str_accessor_fn dataobjectname =
            (str_accessor_fn)dlsym_or_die(stub, "recording_stub_slice_dataobjectname");
        int_accessor_fn rwmode = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_slice_rwmode");
        double_accessor_fn time =
            (double_accessor_fn)dlsym_or_die(stub, "recording_stub_slice_time");
        int_accessor_fn interpmode =
            (int_accessor_fn)dlsym_or_die(stub, "recording_stub_slice_interpmode");

        int octxID = -1;
        al_status_t status = al_begin_slice_action(1001, "equilibrium", 1, 12.5, 3, &octxID);
        CHECK(status.code == 0);
        CHECK(call_count() == 1);
        CHECK(pctx_id() == 1001);
        CHECK(strcmp(dataobjectname(), "equilibrium") == 0);
        CHECK(rwmode() == 1);
        CHECK(time() == 12.5);
        CHECK(interpmode() == 3);
        CHECK(octxID == 2002);
    }

    /* al_begin_timerange_action */
    {
        int_accessor_fn call_count =
            (int_accessor_fn)dlsym_or_die(stub, "recording_stub_timerange_call_count");
        int_accessor_fn pctx_id =
            (int_accessor_fn)dlsym_or_die(stub, "recording_stub_timerange_pctx_id");
        str_accessor_fn dataobjectname =
            (str_accessor_fn)dlsym_or_die(stub, "recording_stub_timerange_dataobjectname");
        int_accessor_fn rwmode =
            (int_accessor_fn)dlsym_or_die(stub, "recording_stub_timerange_rwmode");
        double_accessor_fn tmin =
            (double_accessor_fn)dlsym_or_die(stub, "recording_stub_timerange_tmin");
        double_accessor_fn tmax =
            (double_accessor_fn)dlsym_or_die(stub, "recording_stub_timerange_tmax");
        ptr_accessor_fn dtime_buffer =
            (ptr_accessor_fn)dlsym_or_die(stub, "recording_stub_timerange_dtime_buffer");
        ptr_accessor_fn dtime_shape =
            (ptr_accessor_fn)dlsym_or_die(stub, "recording_stub_timerange_dtime_shape");
        int_accessor_fn interpmode =
            (int_accessor_fn)dlsym_or_die(stub, "recording_stub_timerange_interpmode");

        double dtime_values[3] = {0.0, 0.5, 1.0};
        int dtime_shape_value = 3;
        int octxID = -1;
        al_status_t status = al_begin_timerange_action(1001, "equilibrium", 2, 0.0, 1.0,
                                                         dtime_values, &dtime_shape_value, 3,
                                                         &octxID);
        CHECK(status.code == 0);
        CHECK(call_count() == 1);
        CHECK(pctx_id() == 1001);
        CHECK(strcmp(dataobjectname(), "equilibrium") == 0);
        CHECK(rwmode() == 2);
        CHECK(tmin() == 0.0);
        CHECK(tmax() == 1.0);
        CHECK(dtime_buffer() == dtime_values);
        CHECK(dtime_shape() == &dtime_shape_value);
        CHECK(interpmode() == 3);
        CHECK(octxID == 2003);
    }

    /* al_begin_arraystruct_action */
    {
        int_accessor_fn call_count =
            (int_accessor_fn)dlsym_or_die(stub, "recording_stub_arraystruct_call_count");
        int_accessor_fn ctx_id =
            (int_accessor_fn)dlsym_or_die(stub, "recording_stub_arraystruct_ctx_id");
        str_accessor_fn path = (str_accessor_fn)dlsym_or_die(stub, "recording_stub_arraystruct_path");
        str_accessor_fn timebase =
            (str_accessor_fn)dlsym_or_die(stub, "recording_stub_arraystruct_timebase");
        int_accessor_fn size_in =
            (int_accessor_fn)dlsym_or_die(stub, "recording_stub_arraystruct_size_in");

        int size = 42;
        int actxID = -1;
        al_status_t status =
            al_begin_arraystruct_action(2001, "profiles_1d", "time", &size, &actxID);
        CHECK(status.code == 0);
        CHECK(call_count() == 1);
        CHECK(ctx_id() == 2001);
        CHECK(strcmp(path(), "profiles_1d") == 0);
        CHECK(strcmp(timebase(), "time") == 0);
        CHECK(size_in() == 42);
        CHECK(size == 3003);
        CHECK(actxID == 3004);
    }

    /* al_end_action */
    {
        int_accessor_fn call_count =
            (int_accessor_fn)dlsym_or_die(stub, "recording_stub_end_action_call_count");
        int_accessor_fn ctx_id =
            (int_accessor_fn)dlsym_or_die(stub, "recording_stub_end_action_ctx_id");

        al_status_t status = al_end_action(3004);
        CHECK(status.code == 0);
        CHECK(call_count() == 1);
        CHECK(ctx_id() == 3004);
    }

    /* al_read_data */
    {
        int_accessor_fn call_count =
            (int_accessor_fn)dlsym_or_die(stub, "recording_stub_read_call_count");
        int_accessor_fn ctx_id = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_read_ctx_id");
        str_accessor_fn field = (str_accessor_fn)dlsym_or_die(stub, "recording_stub_read_field");
        str_accessor_fn timebase =
            (str_accessor_fn)dlsym_or_die(stub, "recording_stub_read_timebase");
        int_accessor_fn datatype =
            (int_accessor_fn)dlsym_or_die(stub, "recording_stub_read_datatype");
        int_accessor_fn dim = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_read_dim");
        ptr_accessor_fn buffer_address =
            (ptr_accessor_fn)dlsym_or_die(stub, "recording_stub_read_buffer_address");

        void *data = NULL;
        int size[1] = {0};
        al_status_t status = al_read_data(2001, "temperature", "time", &data, 3 /* DOUBLE_DATA */,
                                           1, size);
        CHECK(status.code == 0);
        CHECK(call_count() == 1);
        CHECK(ctx_id() == 2001);
        CHECK(strcmp(field(), "temperature") == 0);
        CHECK(strcmp(timebase(), "time") == 0);
        CHECK(datatype() == 3);
        CHECK(dim() == 1);
        CHECK(data == buffer_address());
        CHECK(size[0] == 4004);

        /* The "two conflicting meanings of zero" (CLAUDE.md): a backend's
         * inner not-found convention (0) must not be reinterpreted as a
         * status-level failure by the shim. code == 0 here is the ABI's
         * success meaning, and it must stay 0 whether or not the field
         * was actually found underneath. */
        setenv("RECORDING_STUB_READ_NOT_FOUND", "1", 1);
        void *not_found_data = &data; /* poisoned: must become NULL */
        int not_found_size[1] = {99};
        al_status_t not_found_status = al_read_data(2001, "missing_field", "time", &not_found_data,
                                                      3, 1, not_found_size);
        unsetenv("RECORDING_STUB_READ_NOT_FOUND");
        CHECK(not_found_status.code == 0);
        CHECK(not_found_data == NULL);
        CHECK(not_found_size[0] == 0);
        CHECK(call_count() == 2);
    }

    /* al_write_data */
    {
        int_accessor_fn call_count =
            (int_accessor_fn)dlsym_or_die(stub, "recording_stub_write_call_count");
        int_accessor_fn ctx_id = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_write_ctx_id");
        str_accessor_fn field = (str_accessor_fn)dlsym_or_die(stub, "recording_stub_write_field");
        str_accessor_fn timebase =
            (str_accessor_fn)dlsym_or_die(stub, "recording_stub_write_timebase");
        ptr_accessor_fn data_ptr = (ptr_accessor_fn)dlsym_or_die(stub, "recording_stub_write_data");
        int_accessor_fn datatype =
            (int_accessor_fn)dlsym_or_die(stub, "recording_stub_write_datatype");
        int_accessor_fn dim = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_write_dim");
        int_accessor_fn size_first =
            (int_accessor_fn)dlsym_or_die(stub, "recording_stub_write_size_first");

        double payload[2] = {3.14, 2.71};
        int size[1] = {2};
        al_status_t status =
            al_write_data(2001, "temperature", "time", payload, 3 /* DOUBLE_DATA */, 1, size);
        CHECK(status.code == 0);
        CHECK(call_count() == 1);
        CHECK(ctx_id() == 2001);
        CHECK(strcmp(field(), "temperature") == 0);
        CHECK(strcmp(timebase(), "time") == 0);
        CHECK(data_ptr() == payload);
        CHECK(datatype() == 3);
        CHECK(dim() == 1);
        CHECK(size_first() == 2);
    }

    /* al_delete_data */
    {
        int_accessor_fn call_count =
            (int_accessor_fn)dlsym_or_die(stub, "recording_stub_delete_call_count");
        int_accessor_fn ctx = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_delete_ctx");
        str_accessor_fn path = (str_accessor_fn)dlsym_or_die(stub, "recording_stub_delete_path");

        al_status_t status = al_delete_data(2001, "temperature");
        CHECK(status.code == 0);
        CHECK(call_count() == 1);
        CHECK(ctx() == 2001);
        CHECK(strcmp(path(), "temperature") == 0);
    }

    /* al_iterate_over_arraystruct */
    {
        int_accessor_fn call_count =
            (int_accessor_fn)dlsym_or_die(stub, "recording_stub_iterate_call_count");
        int_accessor_fn aosctx = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_iterate_aosctx");
        int_accessor_fn step = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_iterate_step");

        al_status_t status = al_iterate_over_arraystruct(3004, 1);
        CHECK(status.code == 0);
        CHECK(call_count() == 1);
        CHECK(aosctx() == 3004);
        CHECK(step() == 1);
    }

    /* al_get_occurrences */
    {
        int_accessor_fn call_count =
            (int_accessor_fn)dlsym_or_die(stub, "recording_stub_occurrences_call_count");
        int_accessor_fn pctx_id =
            (int_accessor_fn)dlsym_or_die(stub, "recording_stub_occurrences_pctx_id");
        str_accessor_fn ids_name =
            (str_accessor_fn)dlsym_or_die(stub, "recording_stub_occurrences_ids_name");

        int *occurrences_list = NULL;
        int size = -1;
        al_status_t status = al_get_occurrences(1001, "core_profiles", &occurrences_list, &size);
        CHECK(status.code == 0);
        CHECK(call_count() == 1);
        CHECK(pctx_id() == 1001);
        CHECK(strcmp(ids_name(), "core_profiles") == 0);
        CHECK(size == 3);
        CHECK(occurrences_list != NULL);
        CHECK(occurrences_list[0] == 11);
        CHECK(occurrences_list[1] == 22);
        CHECK(occurrences_list[2] == 33);
    }

    /* al_list_filled_paths */
    {
        int_accessor_fn call_count =
            (int_accessor_fn)dlsym_or_die(stub, "recording_stub_filled_paths_call_count");
        int_accessor_fn pctx_id =
            (int_accessor_fn)dlsym_or_die(stub, "recording_stub_filled_paths_pctx_id");
        str_accessor_fn dataobjectname =
            (str_accessor_fn)dlsym_or_die(stub, "recording_stub_filled_paths_dataobjectname");

        char **path_list = NULL;
        int size = -1;
        al_status_t status = al_list_filled_paths(1001, "core_profiles", &path_list, &size);
        CHECK(status.code == 0);
        CHECK(call_count() == 1);
        CHECK(pctx_id() == 1001);
        CHECK(strcmp(dataobjectname(), "core_profiles") == 0);
        CHECK(size == 2);
        CHECK(path_list != NULL);
        CHECK(strcmp(path_list[0], "ids/path/one") == 0);
        CHECK(strcmp(path_list[1], "ids/path/two") == 0);
    }

    printf("runtime_binding_test verbatim-forwarding: every seam and lifecycle call reached the "
           "stub unmodified\n");
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr,
                "usage: %s "
                "<success|version-drift|version-mismatch|null-version|missing-library|"
                "bare-soname|verbatim-forwarding>\n",
                argv[0]);
        return 2;
    }

    const char *scenario = argv[1];
    if (strcmp(scenario, "success") == 0) {
        scenario_success();
    } else if (strcmp(scenario, "version-drift") == 0) {
        scenario_version_drift();
    } else if (strcmp(scenario, "version-mismatch") == 0) {
        scenario_version_mismatch();
    } else if (strcmp(scenario, "null-version") == 0) {
        scenario_null_version();
    } else if (strcmp(scenario, "missing-library") == 0) {
        scenario_missing_library();
    } else if (strcmp(scenario, "bare-soname") == 0) {
        scenario_bare_soname();
    } else if (strcmp(scenario, "verbatim-forwarding") == 0) {
        scenario_verbatim_forwarding();
    } else {
        fprintf(stderr, "unknown scenario: %s\n", scenario);
        return 2;
    }

    return 0;
}
