/* The tracer bullet from issue #3: drives the shim's exported
 * al_context_info against both a recording stub and a real IMAS-Core.
 * Proves the shim exports al_context_info *and* calls al_context_info, with
 * its own definition never capturing the outbound call.
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
#ifndef SHIM_LIBRARY_PATH
#error "SHIM_LIBRARY_PATH must be defined by the build"
#endif

typedef int (*int_accessor_fn)(void);
typedef const char *(*indexed_str_accessor_fn)(int index);
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

#define CHECK_PLUGIN_CALL(count, symbol)                                        \
    do {                                                                        \
        CHECK(call_count() == (count));                                         \
        CHECK(strcmp(last_symbol(), (symbol)) == 0);                            \
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

static void check_context_info(int ctx, const char *expected_info) {
    char *info = NULL;
    al_status_t status = al_context_info(ctx, &info);

    CHECK(status.code == 0);
    CHECK(info != NULL);
    CHECK(strcmp(info, expected_info) == 0);
    free(info);
}

static void scenario_success(void) {
    void *stub = open_stub_for_introspection();
    int_accessor_fn call_count = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_call_count");
    int_accessor_fn last_ctx = (int_accessor_fn)dlsym_or_die(stub, "recording_stub_last_ctx");
    int_accessor_fn version_call_count =
        (int_accessor_fn)dlsym_or_die(stub, "recording_stub_version_call_count");
    CHECK(call_count() == 0);

    check_context_info(42, "recording-stub: context info");
    CHECK(call_count() == 1);
    CHECK(last_ctx() == 42);

    /* A second call reaches the stub again without re-resolving. */
    check_context_info(7, "recording-stub: context info");
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
    int_accessor_fn utility_call_count =
        (int_accessor_fn)dlsym_or_die(stub, "recording_stub_utility_call_count");

    char *info = NULL;
    al_status_t status = al_context_info(1, &info);

    CHECK(status.code != 0);
    CHECK(strstr(status.message, SUPPORTED_CORE_VERSION) != NULL);
    CHECK(strstr(status.message, INCOMPATIBLE_CORE_VERSION) != NULL);
    CHECK(strstr(status.message, "IMAS_CORE_LIBRARY") != NULL);
    /* The mismatch must fail resolution before ever forwarding the call. */
    CHECK(call_count() == 0);
    CHECK(utility_call_count() == 0);

    /* These diagnostics have no status channel. The ADR requires the shim
     * to serve them without resolving any further mismatched-Core symbol. */
    CHECK(strcmp(getALVersion(), INCOMPATIBLE_CORE_VERSION) == 0);
    CHECK(strcmp(const2str(13), "HDF5_BACKEND") == 0);
    CHECK(strcmp(err2str(-3), "BACKEND_ERR") == 0);
    CHECK(strcmp(getDDVersion(), "!!DEPRECATED!!") == 0);
    CHECK(utility_call_count() == 0);

    printf("runtime_binding_test version-mismatch: operations failed and diagnostics stayed local\n");
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

static void scenario_real_core(void) {
    /* Unlike the other scenarios, this one runs against a real, acquired
     * IMAS-Core (see CMakeLists.txt's IMAS-Core acquisition section), not
     * the recording stub, so there is no introspection handle to open:
     * real IMAS-Core exports no such thing. ctxID 0 is real IMAS-Core's
     * "NULL context" case (al_lowlevel.cpp), the one value it answers
     * deterministically with no context ever having been opened. */
    check_context_info(0, "NULL context");

    /* The same public forwarding path remains stable across repeated calls.
     * Stub-only introspection in scenario_success verifies memoization. */
    check_context_info(0, "NULL context");

    printf("runtime_binding_test real-core: the shim reached real IMAS-Core, not a stub\n");
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
        for (int i = 0; i < size; ++i) {
            free(path_list[i]);
        }
        free(path_list);
    }

    printf("runtime_binding_test verbatim-forwarding: every seam and lifecycle call reached the "
           "stub unmodified\n");
}

/* Issue #7's public ABI seam. This is intentionally written against the
 * generated shim header: missing declarations are a build failure, just as
 * they would be for an HLI. The recording-stub assertions are added with the
 * implementation below. */
static void scenario_plugin_forwarding(void) {
    void *stub = open_stub_for_introspection();
    int_accessor_fn call_count =
        (int_accessor_fn)dlsym_or_die(stub, "recording_stub_plugin_call_count");
    str_accessor_fn last_symbol =
        (str_accessor_fn)dlsym_or_die(stub, "recording_stub_plugin_last_symbol");
    str_accessor_fn first_string =
        (str_accessor_fn)dlsym_or_die(stub, "recording_stub_plugin_first_string");
    str_accessor_fn second_string =
        (str_accessor_fn)dlsym_or_die(stub, "recording_stub_plugin_second_string");
    int_accessor_fn last_ctx =
        (int_accessor_fn)dlsym_or_die(stub, "recording_stub_plugin_last_ctx");
    int_accessor_fn first_int =
        (int_accessor_fn)dlsym_or_die(stub, "recording_stub_plugin_first_int");
    int_accessor_fn second_int =
        (int_accessor_fn)dlsym_or_die(stub, "recording_stub_plugin_second_int");
    double_accessor_fn double_value =
        (double_accessor_fn)dlsym_or_die(stub, "recording_stub_plugin_double");
    ptr_accessor_fn pointer_value =
        (ptr_accessor_fn)dlsym_or_die(stub, "recording_stub_plugin_pointer");
    ptr_accessor_fn size_pointer =
        (ptr_accessor_fn)dlsym_or_die(stub, "recording_stub_plugin_size_pointer");
    bool is_registered = false;
    int size = 2;
    int values[2] = {17, 29};
    int opctx = -1;
    int aosctx = -1;
    void *read_data = NULL;
    double write_data = 3.5;

    CHECK(call_count() == 0);
    CHECK(al_register_plugin("recording-plugin").code == 0);
    CHECK_PLUGIN_CALL(1, "al_register_plugin");
    CHECK(strcmp(first_string(), "recording-plugin") == 0);
    CHECK(al_unregister_plugin("recording-plugin").code == 0);
    CHECK_PLUGIN_CALL(2, "al_unregister_plugin");
    CHECK(strcmp(first_string(), "recording-plugin") == 0);
    CHECK(al_bind_plugin("equilibrium/time", "recording-plugin").code == 0);
    CHECK_PLUGIN_CALL(3, "al_bind_plugin");
    CHECK(strcmp(first_string(), "equilibrium/time") == 0);
    CHECK(strcmp(second_string(), "recording-plugin") == 0);
    CHECK(al_unbind_plugin("equilibrium/time", "recording-plugin").code == 0);
    CHECK_PLUGIN_CALL(4, "al_unbind_plugin");
    CHECK(strcmp(first_string(), "equilibrium/time") == 0);
    CHECK(strcmp(second_string(), "recording-plugin") == 0);
    CHECK(al_bind_readback_plugins(701).code == 0);
    CHECK_PLUGIN_CALL(5, "al_bind_readback_plugins");
    CHECK(last_ctx() == 701);
    CHECK(al_unbind_readback_plugins(702).code == 0);
    CHECK_PLUGIN_CALL(6, "al_unbind_readback_plugins");
    CHECK(last_ctx() == 702);
    CHECK(al_is_plugin_registered("recording-plugin", &is_registered).code == 0);
    CHECK_PLUGIN_CALL(7, "al_is_plugin_registered");
    CHECK(is_registered);
    CHECK(al_write_plugins_metadata(703).code == 0);
    CHECK_PLUGIN_CALL(8, "al_write_plugins_metadata");
    CHECK(last_ctx() == 703);
    CHECK(al_setvalue_parameter_plugin("coefficients", 2, 1, &size, values,
                                       "recording-plugin").code == 0);
    CHECK_PLUGIN_CALL(9, "al_setvalue_parameter_plugin");
    CHECK(strcmp(first_string(), "coefficients") == 0);
    CHECK(strcmp(second_string(), "recording-plugin") == 0);
    CHECK(first_int() == 2 && second_int() == 1 && pointer_value() == values);
    CHECK(size_pointer() == &size);
    CHECK(al_setvalue_int_scalar_parameter_plugin("iterations", 37,
                                                   "recording-plugin").code == 0);
    CHECK_PLUGIN_CALL(10, "al_setvalue_int_scalar_parameter_plugin");
    CHECK(first_int() == 37 && strcmp(first_string(), "iterations") == 0);
    CHECK(strcmp(second_string(), "recording-plugin") == 0);
    CHECK(al_setvalue_double_scalar_parameter_plugin("tolerance", 1.25,
                                                      "recording-plugin").code == 0);
    CHECK_PLUGIN_CALL(11, "al_setvalue_double_scalar_parameter_plugin");
    CHECK(double_value() == 1.25 && strcmp(first_string(), "tolerance") == 0);
    CHECK(strcmp(second_string(), "recording-plugin") == 0);
    CHECK(al_plugin_begin_global_action(704, "equilibrium", "profiles", 2, &opctx).code == 0);
    CHECK_PLUGIN_CALL(12, "al_plugin_begin_global_action");
    CHECK(last_ctx() == 704 && strcmp(first_string(), "equilibrium") == 0);
    CHECK(strcmp(second_string(), "profiles") == 0 && first_int() == 2);
    CHECK(opctx == 5001);
    CHECK(al_plugin_begin_slice_action(705, "core_profiles", 1, 2.5, 3, &opctx).code == 0);
    CHECK_PLUGIN_CALL(13, "al_plugin_begin_slice_action");
    CHECK(last_ctx() == 705 && first_int() == 1 && second_int() == 3 && double_value() == 2.5);
    CHECK(strcmp(first_string(), "core_profiles") == 0 && second_string() == NULL);
    CHECK(opctx == 5002);
    CHECK(al_plugin_begin_arraystruct_action(706, "profiles_1d", "time", &size, &aosctx)
              .code == 0);
    CHECK_PLUGIN_CALL(14, "al_plugin_begin_arraystruct_action");
    CHECK(last_ctx() == 706 && pointer_value() == &size);
    CHECK(strcmp(first_string(), "profiles_1d") == 0 && strcmp(second_string(), "time") == 0);
    CHECK(size == 5003);
    CHECK(aosctx == 5004);
    CHECK(al_plugin_end_action(707).code == 0);
    CHECK_PLUGIN_CALL(15, "al_plugin_end_action");
    CHECK(last_ctx() == 707);
    CHECK(al_plugin_read_data(708, "temperature", "time", &read_data, 3, 1, &size).code == 0);
    CHECK_PLUGIN_CALL(16, "al_plugin_read_data");
    CHECK(last_ctx() == 708 && first_int() == 3 && second_int() == 1);
    CHECK(strcmp(first_string(), "temperature") == 0 && strcmp(second_string(), "time") == 0);
    CHECK(size_pointer() == &size);
    CHECK(read_data != NULL);
    CHECK(size == 5005);
    CHECK(al_plugin_write_data(709, "temperature", "time", &write_data, 3, 0, NULL).code == 0);
    CHECK_PLUGIN_CALL(17, "al_plugin_write_data");
    CHECK(last_ctx() == 709 && pointer_value() == &write_data);
    CHECK(first_int() == 3 && second_int() == 0);
    CHECK(strcmp(first_string(), "temperature") == 0 && strcmp(second_string(), "time") == 0);
    CHECK(size_pointer() == NULL);
    CHECK(call_count() == 17);

    printf("runtime_binding_test plugin-forwarding: every exported plugin symbol reached the stub\n");
}

static void scenario_plugin_timerange_omitted(void) {
    void *shim = dlopen(SHIM_LIBRARY_PATH, RTLD_NOW | RTLD_LOCAL);
    if (shim == NULL) {
        fprintf(stderr, "failed to dlopen the shim: %s\n", dlerror());
        abort();
    }
    dlerror();
    CHECK(dlsym(shim, "al_plugin_begin_timerange_action") == NULL);
    CHECK(dlerror() != NULL);
    dlclose(shim);

    printf("runtime_binding_test plugin-timerange-omitted: matches IMAS-Core's unlinkable ABI\n");
}

/* Issue #8's utility and version ABI seam. The stub records every argument
 * and returns distinct values, so this verifies the generated header and the
 * runtime forwarding boundary without coupling to resolver internals. */
static void scenario_utility_forwarding(void) {
    void *stub = open_stub_for_introspection();
    int_accessor_fn call_count =
        (int_accessor_fn)dlsym_or_die(stub, "recording_stub_utility_call_count");
    str_accessor_fn last_symbol =
        (str_accessor_fn)dlsym_or_die(stub, "recording_stub_utility_last_symbol");
    int_accessor_fn last_int =
        (int_accessor_fn)dlsym_or_die(stub, "recording_stub_utility_last_int");
    int_accessor_fn backend_ctx =
        (int_accessor_fn)dlsym_or_die(stub, "recording_stub_utility_backend_ctx");
    ptr_accessor_fn backend_output =
        (ptr_accessor_fn)dlsym_or_die(stub, "recording_stub_utility_backend_output");
    int_accessor_fn builder_backend =
        (int_accessor_fn)dlsym_or_die(stub, "recording_stub_utility_builder_backend");
    int_accessor_fn builder_pulse =
        (int_accessor_fn)dlsym_or_die(stub, "recording_stub_utility_builder_pulse");
    int_accessor_fn builder_run =
        (int_accessor_fn)dlsym_or_die(stub, "recording_stub_utility_builder_run");
    indexed_str_accessor_fn builder_string = (indexed_str_accessor_fn)dlsym_or_die(
        stub, "recording_stub_utility_builder_string");
    ptr_accessor_fn builder_output =
        (ptr_accessor_fn)dlsym_or_die(stub, "recording_stub_utility_builder_output");
    int_accessor_fn version_call_count =
        (int_accessor_fn)dlsym_or_die(stub, "recording_stub_version_call_count");

    CHECK(call_count() == 0);
    int backend_id = -1;
    CHECK(al_get_backendID(801, &backend_id).code == 0);
    CHECK(call_count() == 1);
    CHECK(strcmp(last_symbol(), "al_get_backendID") == 0);
    CHECK(backend_ctx() == 801 && backend_output() == &backend_id);
    CHECK(backend_id == 9001);

    char *uri = NULL;
    CHECK(al_build_uri_from_legacy_parameters(3, 44, 5, "user", "tokamak", "4.1.1",
                                              "options=record", &uri)
              .code == 0);
    CHECK(call_count() == 2);
    CHECK(strcmp(last_symbol(), "al_build_uri_from_legacy_parameters") == 0);
    CHECK(builder_backend() == 3 && builder_pulse() == 44 && builder_run() == 5);
    CHECK(strcmp(builder_string(0), "user") == 0);
    CHECK(strcmp(builder_string(1), "tokamak") == 0);
    CHECK(strcmp(builder_string(2), "4.1.1") == 0);
    CHECK(strcmp(builder_string(3), "options=record") == 0);
    CHECK(builder_output() == &uri);
    CHECK(strcmp(uri, "imas:recording?utility=legacy") == 0);
    free(uri);

    CHECK(strcmp(const2str(12345), "recording-constant") == 0);
    CHECK(call_count() == 3);
    CHECK(strcmp(last_symbol(), "const2str") == 0 && last_int() == 12345);
    CHECK(strcmp(err2str(-44), "recording-error") == 0);
    CHECK(call_count() == 4);
    CHECK(strcmp(last_symbol(), "err2str") == 0 && last_int() == -44);
    CHECK(strcmp(getDDVersion(), "!!DEPRECATED!!") == 0);
    CHECK(call_count() == 5);
    CHECK(strcmp(last_symbol(), "getDDVersion") == 0);

    /* One getALVersion call bootstraps resolution; this is the forwarded one. */
    CHECK(strcmp(getALVersion(), SUPPORTED_CORE_VERSION) == 0);
    CHECK(version_call_count() == 2);

    printf("runtime_binding_test utility-forwarding: every utility and version symbol reached "
           "the stub unmodified\n");
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr,
                "usage: %s "
                "<success|version-drift|version-mismatch|null-version|missing-library|"
                "bare-soname|verbatim-forwarding|plugin-forwarding|plugin-timerange-omitted|"
                "utility-forwarding|real-core>\n",
                argv[0]);
        return 2;
    }

    const char *scenario = argv[1];
    if (strcmp(scenario, "success") == 0) {
        scenario_success();
    } else if (strcmp(scenario, "real-core") == 0) {
        scenario_real_core();
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
    } else if (strcmp(scenario, "plugin-forwarding") == 0) {
        scenario_plugin_forwarding();
    } else if (strcmp(scenario, "plugin-timerange-omitted") == 0) {
        scenario_plugin_timerange_omitted();
    } else if (strcmp(scenario, "utility-forwarding") == 0) {
        scenario_utility_forwarding();
    } else {
        fprintf(stderr, "unknown scenario: %s\n", scenario);
        return 2;
    }

    return 0;
}
