/* Stands in for IMAS-Core in tests/runtime_binding_test.c: exports the mirrored ABI symbols
 * under their real names and signatures, and records what it received
 * instead of doing anything real.
 *
 * al_status_t is duplicated here rather than pulled from the shim's
 * generated header: a real IMAS-Core defines its own copy independently of
 * this project's header, and this stub should behave the same way. */

#include <stdlib.h>
#include <string.h>

#ifndef RECORDING_STUB_DEFAULT_VERSION
#error "RECORDING_STUB_DEFAULT_VERSION must come from IMAS_CORE_VERSION"
#endif

typedef struct {
    int code;
    char message[256];
} al_status_t;

static al_status_t ok_status(void) {
    al_status_t status;
    status.code = 0;
    memset(status.message, 0, sizeof status.message);
    return status;
}

static char *record_str(const char *value) {
    if (value == NULL) {
        return NULL;
    }
    size_t len = strlen(value) + 1;
    char *copy = malloc(len);
    if (copy != NULL) {
        memcpy(copy, value, len);
    }
    return copy;
}

static int g_call_count = 0;
static int g_last_ctx = 0;
static int g_version_call_count = 0;

al_status_t al_context_info(int ctx, char **info) {
    g_call_count++;
    g_last_ctx = ctx;

    al_status_t status = ok_status();

    if (info != NULL) {
        static const char reply[] = "recording-stub: context info";
        char *copy = malloc(sizeof reply);
        if (copy != NULL) {
            memcpy(copy, reply, sizeof reply);
        }
        *info = copy;
    }

    return status;
}

/* Defaults to the repository's supported IMAS-Core release;
 * RECORDING_STUB_VERSION lets a test simulate a different release. */
const char *getALVersion(void) {
    g_version_call_count++;
    if (getenv("RECORDING_STUB_NULL_VERSION") != NULL) {
        return NULL;
    }
    const char *version_override = getenv("RECORDING_STUB_VERSION");
    return version_override != NULL ? version_override : RECORDING_STUB_DEFAULT_VERSION;
}

/* --- utility and version accessors --------------------------------------- */

static int g_utility_call_count = 0;
static const char *g_utility_last_symbol = NULL;
static int g_utility_last_int = 0;
static int g_utility_backend_ctx = 0;
static int *g_utility_backend_output = NULL;
static int g_utility_builder_backend = 0;
static int g_utility_builder_pulse = 0;
static int g_utility_builder_run = 0;
static char *g_utility_builder_strings[4] = {NULL, NULL, NULL, NULL};
static char **g_utility_builder_output = NULL;

static void record_utility_call(const char *symbol) {
    g_utility_call_count++;
    g_utility_last_symbol = symbol;
}

al_status_t al_get_backendID(int ctx, int *beid) {
    record_utility_call("al_get_backendID");
    g_utility_backend_ctx = ctx;
    g_utility_backend_output = beid;
    if (beid != NULL) {
        *beid = 9001;
    }
    return ok_status();
}

al_status_t al_build_uri_from_legacy_parameters(const int backendID, const int pulse,
                                                 const int run, const char *user,
                                                 const char *tokamak, const char *version,
                                                 const char *options, char **uri) {
    record_utility_call("al_build_uri_from_legacy_parameters");
    g_utility_builder_backend = backendID;
    g_utility_builder_pulse = pulse;
    g_utility_builder_run = run;
    const char *strings[] = {user, tokamak, version, options};
    for (int i = 0; i < 4; ++i) {
        free(g_utility_builder_strings[i]);
        g_utility_builder_strings[i] = record_str(strings[i]);
    }
    g_utility_builder_output = uri;
    if (uri != NULL) {
        static const char result[] = "imas:recording?utility=legacy";
        *uri = malloc(sizeof result);
        if (*uri != NULL) {
            memcpy(*uri, result, sizeof result);
        }
    }
    return ok_status();
}

const char *const2str(int id) {
    record_utility_call("const2str");
    g_utility_last_int = id;
    return "recording-constant";
}

const char *err2str(int id) {
    record_utility_call("err2str");
    g_utility_last_int = id;
    return "recording-error";
}

const char *getDDVersion(void) {
    record_utility_call("getDDVersion");
    return "!!DEPRECATED!!";
}

/* --- al_begin_dataentry_action ------------------------------------------ */

static int g_dataentry_call_count = 0;
static char *g_dataentry_uri = NULL;
static int g_dataentry_mode = 0;

al_status_t al_begin_dataentry_action(const char *uri, int mode, int *dectxID) {
    g_dataentry_call_count++;
    free(g_dataentry_uri);
    g_dataentry_uri = record_str(uri);
    g_dataentry_mode = mode;

    /* RECORDING_STUB_DATAENTRY_FAIL lets a test simulate a failed open: the
     * shim must still forward uri/mode unchanged (verified above via the
     * recording, which runs before this check) and must register no
     * data-entry context for the caller-visible failure. */
    if (getenv("RECORDING_STUB_DATAENTRY_FAIL") != NULL) {
        al_status_t status;
        status.code = -7;
        memset(status.message, 0, sizeof status.message);
        strncpy(status.message, "recording-stub: dataentry open refused",
                sizeof status.message - 1);
        if (dectxID != NULL) {
            *dectxID = 0;
        }
        return status;
    }

    if (dectxID != NULL) {
        *dectxID = 1001;
    }
    return ok_status();
}

/* --- al_close_pulse ------------------------------------------------------ */

static int g_close_pulse_call_count = 0;
static int g_close_pulse_ctx = 0;
static int g_close_pulse_mode = 0;

al_status_t al_close_pulse(int pulseCtx, int mode) {
    g_close_pulse_call_count++;
    g_close_pulse_ctx = pulseCtx;
    g_close_pulse_mode = mode;
    return ok_status();
}

/* --- al_begin_global_action ---------------------------------------------- */

static int g_global_call_count = 0;
static int g_global_pctx_id = 0;
static char *g_global_dataobjectname = NULL;
static char *g_global_datapath = NULL;
static int g_global_rwmode = 0;

al_status_t al_begin_global_action(int pctxID, const char *dataobjectname, const char *datapath,
                                    int rwmode, int *octxID) {
    g_global_call_count++;
    g_global_pctx_id = pctxID;
    free(g_global_dataobjectname);
    g_global_dataobjectname = record_str(dataobjectname);
    free(g_global_datapath);
    g_global_datapath = record_str(datapath);
    g_global_rwmode = rwmode;
    if (octxID != NULL) {
        *octxID = 2001;
    }
    return ok_status();
}

/* --- al_begin_slice_action ------------------------------------------------ */

static int g_slice_call_count = 0;
static int g_slice_pctx_id = 0;
static char *g_slice_dataobjectname = NULL;
static int g_slice_rwmode = 0;
static double g_slice_time = 0.0;
static int g_slice_interpmode = 0;

al_status_t al_begin_slice_action(int pctxID, const char *dataobjectname, int rwmode, double time,
                                   int interpmode, int *octxID) {
    g_slice_call_count++;
    g_slice_pctx_id = pctxID;
    free(g_slice_dataobjectname);
    g_slice_dataobjectname = record_str(dataobjectname);
    g_slice_rwmode = rwmode;
    g_slice_time = time;
    g_slice_interpmode = interpmode;
    if (octxID != NULL) {
        *octxID = 2002;
    }
    return ok_status();
}

/* --- al_begin_timerange_action --------------------------------------------- */

static int g_timerange_call_count = 0;
static int g_timerange_pctx_id = 0;
static char *g_timerange_dataobjectname = NULL;
static int g_timerange_rwmode = 0;
static double g_timerange_tmin = 0.0;
static double g_timerange_tmax = 0.0;
static const double *g_timerange_dtime_buffer = NULL;
static const int *g_timerange_dtime_shape = NULL;
static int g_timerange_interpmode = 0;

al_status_t al_begin_timerange_action(int pctxID, const char *dataobjectname, int rwmode,
                                       double tmin, double tmax, const double *dtime_buffer,
                                       const int *dtime_shape, int interpmode, int *octxID) {
    g_timerange_call_count++;
    g_timerange_pctx_id = pctxID;
    free(g_timerange_dataobjectname);
    g_timerange_dataobjectname = record_str(dataobjectname);
    g_timerange_rwmode = rwmode;
    g_timerange_tmin = tmin;
    g_timerange_tmax = tmax;
    g_timerange_dtime_buffer = dtime_buffer;
    g_timerange_dtime_shape = dtime_shape;
    g_timerange_interpmode = interpmode;
    if (octxID != NULL) {
        *octxID = 2003;
    }
    return ok_status();
}

/* --- al_begin_arraystruct_action ------------------------------------------- */

static int g_arraystruct_call_count = 0;
static int g_arraystruct_ctx_id = 0;
static char *g_arraystruct_path = NULL;
static char *g_arraystruct_timebase = NULL;
static int g_arraystruct_size_in = 0;

al_status_t al_begin_arraystruct_action(int ctxID, const char *path, const char *timebase,
                                         int *size, int *actxID) {
    g_arraystruct_call_count++;
    g_arraystruct_ctx_id = ctxID;
    free(g_arraystruct_path);
    g_arraystruct_path = record_str(path);
    free(g_arraystruct_timebase);
    g_arraystruct_timebase = record_str(timebase);
    if (size != NULL) {
        g_arraystruct_size_in = *size;
        *size = 3003;
    }
    if (actxID != NULL) {
        *actxID = 3004;
    }
    return ok_status();
}

/* --- al_end_action --------------------------------------------------------- */

static int g_end_action_call_count = 0;
static int g_end_action_ctx_id = 0;

al_status_t al_end_action(int ctxID) {
    g_end_action_call_count++;
    g_end_action_ctx_id = ctxID;
    return ok_status();
}

/* --- al_read_data ------------------------------------------------------------ */

static int g_read_call_count = 0;
static int g_read_ctx_id = 0;
static char *g_read_field = NULL;
static char *g_read_timebase = NULL;
static int g_read_datatype = 0;
static int g_read_dim = 0;
static char g_read_buffer[] = "recording-stub: read data payload";

#define VERSION_STAMP_FIELD "ids_properties/version_put/data_dictionary"

/* RECORDING_STUB_STAMP_VERSION lets a test control the DD-version stamp
 * discovery read (issue #53) independently of every other al_read_data call:
 * unset means the stamp is absent (the shim's "unstamped" case), set to a
 * value means the stamp is present and holds exactly that value — a known
 * DD release/development spelling for the "stored version discovered" case,
 * or garbage for the "malformed stamp" case. The returned buffer is
 * malloc'd, sized to exactly the stamp's byte length, and deliberately never
 * NUL-terminated: the shim owns freeing it (this read never reaches an HLI)
 * and must decode it by the reported size, never by scanning for a NUL. */
static al_status_t stamp_read_response(void **data, int *size) {
    const char *stamp = getenv("RECORDING_STUB_STAMP_VERSION");
    al_status_t status = ok_status();
    if (getenv("RECORDING_STUB_STAMP_READ_FAIL") != NULL) {
        /* A failed discovery is distinct from successful not-found. The
         * shim's read-outcome classifier treats both as an unstamped
         * occurrence, but this switch keeps that classifier branch covered
         * through the public global-action ABI (issue #53, ADR 0012). */
        status.code = -8;
        strncpy(status.message, "recording-stub: stamp read refused", sizeof status.message - 1);
        if (data != NULL) {
            *data = NULL;
        }
        if (size != NULL) {
            *size = 0;
        }
        return status;
    }
    if (stamp == NULL) {
        if (data != NULL) {
            *data = NULL;
        }
        if (size != NULL) {
            *size = 0;
        }
        return status;
    }

    size_t len = strlen(stamp);
    char *buffer = malloc(len > 0 ? len : 1);
    if (buffer == NULL) {
        status.code = -1;
        strncpy(status.message, "recording-stub: allocation failed", sizeof status.message - 1);
        return status;
    }
    if (len > 0) {
        memcpy(buffer, stamp, len);
    }
    if (data != NULL) {
        *data = buffer;
    }
    if (size != NULL) {
        *size = (int)len;
    }
    return status;
}

al_status_t al_read_data(int ctxID, const char *field, const char *timebase, void **data,
                          int datatype, int dim, int *size) {
    g_read_call_count++;
    g_read_ctx_id = ctxID;
    free(g_read_field);
    g_read_field = record_str(field);
    free(g_read_timebase);
    g_read_timebase = record_str(timebase);
    g_read_datatype = datatype;
    g_read_dim = dim;

    if (field != NULL && strcmp(field, VERSION_STAMP_FIELD) == 0) {
        return stamp_read_response(data, size);
    }

    if (data != NULL) {
        *data = g_read_buffer;
    }
    if (size != NULL) {
        size[0] = 4004;
    }

    al_status_t status;
    /* RECORDING_STUB_READ_NOT_FOUND lets a test simulate the layer-below's
     * "not found" convention (0) while the status code above it still
     * reports success (also 0, but a distinct meaning) — see CLAUDE.md's
     * "two conflicting meanings of zero." The shim must forward this
     * status.code exactly as received, not reinterpret it. */
    if (getenv("RECORDING_STUB_READ_NOT_FOUND") != NULL) {
        status.code = 0;
        memset(status.message, 0, sizeof status.message);
        strncpy(status.message, "recording-stub: not found", sizeof status.message - 1);
        if (data != NULL) {
            *data = NULL;
        }
        if (size != NULL) {
            size[0] = 0;
        }
    } else {
        status = ok_status();
        strncpy(status.message, "recording-stub: read ok", sizeof status.message - 1);
    }
    return status;
}

/* --- al_write_data ------------------------------------------------------------ */

static int g_write_call_count = 0;
static int g_write_ctx_id = 0;
static char *g_write_field = NULL;
static char *g_write_timebase = NULL;
static const void *g_write_data = NULL;
static int g_write_datatype = 0;
static int g_write_dim = 0;
static int g_write_size_first = 0;

al_status_t al_write_data(int ctxID, const char *field, const char *timebase, void *data,
                           int datatype, int dim, int *size) {
    g_write_call_count++;
    g_write_ctx_id = ctxID;
    free(g_write_field);
    g_write_field = record_str(field);
    free(g_write_timebase);
    g_write_timebase = record_str(timebase);
    g_write_data = data;
    g_write_datatype = datatype;
    g_write_dim = dim;
    if (size != NULL && dim > 0) {
        g_write_size_first = size[0];
    }
    return ok_status();
}

/* --- al_delete_data ----------------------------------------------------------- */

static int g_delete_call_count = 0;
static int g_delete_ctx = 0;
static char *g_delete_path = NULL;

al_status_t al_delete_data(int ctx, const char *path) {
    g_delete_call_count++;
    g_delete_ctx = ctx;
    free(g_delete_path);
    g_delete_path = record_str(path);
    return ok_status();
}

/* --- al_iterate_over_arraystruct ---------------------------------------------- */

static int g_iterate_call_count = 0;
static int g_iterate_aosctx = 0;
static int g_iterate_step = 0;

al_status_t al_iterate_over_arraystruct(int aosctx, int step) {
    g_iterate_call_count++;
    g_iterate_aosctx = aosctx;
    g_iterate_step = step;
    return ok_status();
}

/* --- al_get_occurrences -------------------------------------------------------- */

static int g_occurrences_call_count = 0;
static int g_occurrences_pctx_id = 0;
static char *g_occurrences_ids_name = NULL;
static int g_occurrences_values[] = {11, 22, 33};

al_status_t al_get_occurrences(int pctxID, const char *ids_name, int **occurrences_list,
                                int *size) {
    g_occurrences_call_count++;
    g_occurrences_pctx_id = pctxID;
    free(g_occurrences_ids_name);
    g_occurrences_ids_name = record_str(ids_name);
    if (occurrences_list != NULL) {
        *occurrences_list = g_occurrences_values;
    }
    if (size != NULL) {
        *size = 3;
    }
    return ok_status();
}

/* --- al_list_filled_paths ------------------------------------------------------ */

static int g_filled_paths_call_count = 0;
static int g_filled_paths_pctx_id = 0;
static char *g_filled_paths_dataobjectname = NULL;

al_status_t al_list_filled_paths(int pctxID, const char *dataobjectname, char ***path_list,
                                  int *size) {
    g_filled_paths_call_count++;
    g_filled_paths_pctx_id = pctxID;
    free(g_filled_paths_dataobjectname);
    g_filled_paths_dataobjectname = record_str(dataobjectname);

    if (path_list != NULL) {
        char **list = malloc(2 * sizeof *list);
        if (list == NULL) {
            al_status_t status = ok_status();
            status.code = -1;
            strncpy(status.message, "recording-stub: allocation failed",
                    sizeof status.message - 1);
            return status;
        }
        list[0] = record_str("ids/path/one");
        list[1] = record_str("ids/path/two");
        if (list[0] == NULL || list[1] == NULL) {
            free(list[0]);
            free(list[1]);
            free(list);
            al_status_t status = ok_status();
            status.code = -1;
            strncpy(status.message, "recording-stub: allocation failed",
                    sizeof status.message - 1);
            return status;
        }
        *path_list = list;
    }
    if (size != NULL) {
        *size = 2;
    }
    return ok_status();
}

/* --- Plugin ABI family (issue #7) ----------------------------------------- */

static int g_plugin_call_count = 0;
static const char *g_plugin_last_symbol = NULL;
static const char *g_plugin_first_string = NULL;
static const char *g_plugin_second_string = NULL;
static int g_plugin_last_ctx = 0;
static int g_plugin_first_int = 0;
static int g_plugin_second_int = 0;
static double g_plugin_double = 0.0;
static const void *g_plugin_pointer = NULL;
static const void *g_plugin_size_pointer = NULL;

static void record_plugin_call(const char *symbol, int ctx, const char *first, const char *second) {
    g_plugin_call_count++;
    g_plugin_last_symbol = symbol;
    g_plugin_last_ctx = ctx;
    g_plugin_first_string = first;
    g_plugin_second_string = second;
    g_plugin_first_int = 0;
    g_plugin_second_int = 0;
    g_plugin_double = 0.0;
    g_plugin_pointer = NULL;
    g_plugin_size_pointer = NULL;
}

al_status_t al_register_plugin(const char *plugin_name) {
    record_plugin_call("al_register_plugin", 0, plugin_name, NULL);
    (void)plugin_name;
    return ok_status();
}

al_status_t al_unregister_plugin(const char *plugin_name) {
    record_plugin_call("al_unregister_plugin", 0, plugin_name, NULL);
    (void)plugin_name;
    return ok_status();
}

al_status_t al_bind_plugin(const char *field_path, const char *plugin_name) {
    record_plugin_call("al_bind_plugin", 0, field_path, plugin_name);
    (void)field_path;
    (void)plugin_name;
    return ok_status();
}

al_status_t al_unbind_plugin(const char *field_path, const char *plugin_name) {
    record_plugin_call("al_unbind_plugin", 0, field_path, plugin_name);
    (void)field_path;
    (void)plugin_name;
    return ok_status();
}

al_status_t al_bind_readback_plugins(int ctx_id) {
    record_plugin_call("al_bind_readback_plugins", ctx_id, NULL, NULL);
    (void)ctx_id;
    return ok_status();
}

al_status_t al_unbind_readback_plugins(int ctx_id) {
    record_plugin_call("al_unbind_readback_plugins", ctx_id, NULL, NULL);
    (void)ctx_id;
    return ok_status();
}

al_status_t al_is_plugin_registered(const char *plugin_name, _Bool *is_registered) {
    record_plugin_call("al_is_plugin_registered", 0, plugin_name, NULL);
    (void)plugin_name;
    if (is_registered != NULL) {
        *is_registered = 1;
    }
    return ok_status();
}

al_status_t al_write_plugins_metadata(int ctx_id) {
    record_plugin_call("al_write_plugins_metadata", ctx_id, NULL, NULL);
    (void)ctx_id;
    return ok_status();
}

al_status_t al_setvalue_parameter_plugin(const char *parameter_name, int datatype, int dim,
                                         int *size, void *data, const char *plugin_name) {
    record_plugin_call("al_setvalue_parameter_plugin", 0, parameter_name, plugin_name);
    g_plugin_first_int = datatype;
    g_plugin_second_int = dim;
    g_plugin_pointer = data;
    g_plugin_size_pointer = size;
    (void)parameter_name;
    (void)datatype;
    (void)dim;
    (void)size;
    (void)data;
    (void)plugin_name;
    return ok_status();
}

al_status_t al_setvalue_int_scalar_parameter_plugin(const char *parameter_name,
                                                     int parameter_value,
                                                     const char *plugin_name) {
    record_plugin_call("al_setvalue_int_scalar_parameter_plugin", 0, parameter_name, plugin_name);
    g_plugin_first_int = parameter_value;
    (void)parameter_name;
    (void)parameter_value;
    (void)plugin_name;
    return ok_status();
}

al_status_t al_setvalue_double_scalar_parameter_plugin(const char *parameter_name,
                                                        double parameter_value,
                                                        const char *plugin_name) {
    record_plugin_call("al_setvalue_double_scalar_parameter_plugin", 0, parameter_name, plugin_name);
    g_plugin_double = parameter_value;
    (void)parameter_name;
    (void)parameter_value;
    (void)plugin_name;
    return ok_status();
}

al_status_t al_plugin_begin_global_action(int pctx_id, const char *dataobjectname,
                                          const char *datapath, int rwmode, int *octx_id) {
    record_plugin_call("al_plugin_begin_global_action", pctx_id, dataobjectname, datapath);
    g_plugin_first_int = rwmode;
    (void)pctx_id;
    (void)dataobjectname;
    (void)datapath;
    (void)rwmode;
    if (octx_id != NULL) {
        *octx_id = 5001;
    }
    return ok_status();
}

al_status_t al_plugin_begin_slice_action(int pctx_id, const char *dataobjectname, int rwmode,
                                         double time, int interpmode, int *octx_id) {
    record_plugin_call("al_plugin_begin_slice_action", pctx_id, dataobjectname, NULL);
    g_plugin_first_int = rwmode;
    g_plugin_second_int = interpmode;
    g_plugin_double = time;
    (void)pctx_id;
    (void)dataobjectname;
    (void)rwmode;
    (void)time;
    (void)interpmode;
    if (octx_id != NULL) {
        *octx_id = 5002;
    }
    return ok_status();
}

al_status_t al_plugin_begin_arraystruct_action(int ctx_id, const char *path,
                                               const char *timebase, int *size, int *actx_id) {
    record_plugin_call("al_plugin_begin_arraystruct_action", ctx_id, path, timebase);
    g_plugin_pointer = size;
    g_plugin_size_pointer = size;
    (void)ctx_id;
    (void)path;
    (void)timebase;
    if (size != NULL) {
        *size = 5003;
    }
    if (actx_id != NULL) {
        *actx_id = 5004;
    }
    return ok_status();
}

al_status_t al_plugin_end_action(int ctx_id) {
    record_plugin_call("al_plugin_end_action", ctx_id, NULL, NULL);
    (void)ctx_id;
    return ok_status();
}

al_status_t al_plugin_read_data(int ctx_id, const char *field, const char *timebase, void **data,
                                int datatype, int dim, int *size) {
    record_plugin_call("al_plugin_read_data", ctx_id, field, timebase);
    g_plugin_first_int = datatype;
    g_plugin_second_int = dim;
    g_plugin_size_pointer = size;
    static char plugin_read_buffer[] = "recording-stub: plugin read data payload";
    (void)ctx_id;
    (void)field;
    (void)timebase;
    (void)datatype;
    (void)dim;
    if (data != NULL) {
        *data = plugin_read_buffer;
    }
    if (size != NULL) {
        size[0] = 5005;
    }
    return ok_status();
}

al_status_t al_plugin_write_data(int ctx_id, const char *field, const char *timebase, void *data,
                                 int datatype, int dim, int *size) {
    record_plugin_call("al_plugin_write_data", ctx_id, field, timebase);
    g_plugin_first_int = datatype;
    g_plugin_second_int = dim;
    g_plugin_pointer = data;
    g_plugin_size_pointer = size;
    (void)ctx_id;
    (void)field;
    (void)timebase;
    (void)data;
    (void)datatype;
    (void)dim;
    (void)size;
    return ok_status();
}

int recording_stub_plugin_call_count(void) {
    return g_plugin_call_count;
}
const char *recording_stub_plugin_last_symbol(void) { return g_plugin_last_symbol; }
const char *recording_stub_plugin_first_string(void) { return g_plugin_first_string; }
const char *recording_stub_plugin_second_string(void) { return g_plugin_second_string; }
int recording_stub_plugin_last_ctx(void) { return g_plugin_last_ctx; }
int recording_stub_plugin_first_int(void) { return g_plugin_first_int; }
int recording_stub_plugin_second_int(void) { return g_plugin_second_int; }
double recording_stub_plugin_double(void) { return g_plugin_double; }
const void *recording_stub_plugin_pointer(void) { return g_plugin_pointer; }
const void *recording_stub_plugin_size_pointer(void) { return g_plugin_size_pointer; }

/* Introspection accessors below: not part of the mirrored IMAS-Core ABI.
 * tests/runtime_binding_test.c dlsym's these directly rather than linking this stub —
 * see CMakeLists.txt for why. */

int recording_stub_call_count(void) {
    return g_call_count;
}

int recording_stub_last_ctx(void) {
    return g_last_ctx;
}

int recording_stub_version_call_count(void) {
    return g_version_call_count;
}

int recording_stub_utility_call_count(void) {
    return g_utility_call_count;
}
const char *recording_stub_utility_last_symbol(void) {
    return g_utility_last_symbol;
}
int recording_stub_utility_last_int(void) {
    return g_utility_last_int;
}
int recording_stub_utility_backend_ctx(void) {
    return g_utility_backend_ctx;
}
const void *recording_stub_utility_backend_output(void) {
    return g_utility_backend_output;
}
int recording_stub_utility_builder_backend(void) {
    return g_utility_builder_backend;
}
int recording_stub_utility_builder_pulse(void) {
    return g_utility_builder_pulse;
}
int recording_stub_utility_builder_run(void) {
    return g_utility_builder_run;
}
const char *recording_stub_utility_builder_string(int index) {
    return index >= 0 && index < 4 ? g_utility_builder_strings[index] : NULL;
}
const void *recording_stub_utility_builder_output(void) {
    return g_utility_builder_output;
}

int recording_stub_dataentry_call_count(void) {
    return g_dataentry_call_count;
}
const char *recording_stub_dataentry_uri(void) {
    return g_dataentry_uri;
}
int recording_stub_dataentry_mode(void) {
    return g_dataentry_mode;
}

int recording_stub_close_pulse_call_count(void) {
    return g_close_pulse_call_count;
}
int recording_stub_close_pulse_ctx(void) {
    return g_close_pulse_ctx;
}
int recording_stub_close_pulse_mode(void) {
    return g_close_pulse_mode;
}

int recording_stub_global_call_count(void) {
    return g_global_call_count;
}
int recording_stub_global_pctx_id(void) {
    return g_global_pctx_id;
}
const char *recording_stub_global_dataobjectname(void) {
    return g_global_dataobjectname;
}
const char *recording_stub_global_datapath(void) {
    return g_global_datapath;
}
int recording_stub_global_rwmode(void) {
    return g_global_rwmode;
}

int recording_stub_slice_call_count(void) {
    return g_slice_call_count;
}
int recording_stub_slice_pctx_id(void) {
    return g_slice_pctx_id;
}
const char *recording_stub_slice_dataobjectname(void) {
    return g_slice_dataobjectname;
}
int recording_stub_slice_rwmode(void) {
    return g_slice_rwmode;
}
double recording_stub_slice_time(void) {
    return g_slice_time;
}
int recording_stub_slice_interpmode(void) {
    return g_slice_interpmode;
}

int recording_stub_timerange_call_count(void) {
    return g_timerange_call_count;
}
int recording_stub_timerange_pctx_id(void) {
    return g_timerange_pctx_id;
}
const char *recording_stub_timerange_dataobjectname(void) {
    return g_timerange_dataobjectname;
}
int recording_stub_timerange_rwmode(void) {
    return g_timerange_rwmode;
}
double recording_stub_timerange_tmin(void) {
    return g_timerange_tmin;
}
double recording_stub_timerange_tmax(void) {
    return g_timerange_tmax;
}
const void *recording_stub_timerange_dtime_buffer(void) {
    return g_timerange_dtime_buffer;
}
const void *recording_stub_timerange_dtime_shape(void) {
    return g_timerange_dtime_shape;
}
int recording_stub_timerange_interpmode(void) {
    return g_timerange_interpmode;
}

int recording_stub_arraystruct_call_count(void) {
    return g_arraystruct_call_count;
}
int recording_stub_arraystruct_ctx_id(void) {
    return g_arraystruct_ctx_id;
}
const char *recording_stub_arraystruct_path(void) {
    return g_arraystruct_path;
}
const char *recording_stub_arraystruct_timebase(void) {
    return g_arraystruct_timebase;
}
int recording_stub_arraystruct_size_in(void) {
    return g_arraystruct_size_in;
}

int recording_stub_end_action_call_count(void) {
    return g_end_action_call_count;
}
int recording_stub_end_action_ctx_id(void) {
    return g_end_action_ctx_id;
}

int recording_stub_read_call_count(void) {
    return g_read_call_count;
}
int recording_stub_read_ctx_id(void) {
    return g_read_ctx_id;
}
const char *recording_stub_read_field(void) {
    return g_read_field;
}
const char *recording_stub_read_timebase(void) {
    return g_read_timebase;
}
int recording_stub_read_datatype(void) {
    return g_read_datatype;
}
int recording_stub_read_dim(void) {
    return g_read_dim;
}
const void *recording_stub_read_buffer_address(void) {
    return g_read_buffer;
}

int recording_stub_write_call_count(void) {
    return g_write_call_count;
}
int recording_stub_write_ctx_id(void) {
    return g_write_ctx_id;
}
const char *recording_stub_write_field(void) {
    return g_write_field;
}
const char *recording_stub_write_timebase(void) {
    return g_write_timebase;
}
const void *recording_stub_write_data(void) {
    return g_write_data;
}
int recording_stub_write_datatype(void) {
    return g_write_datatype;
}
int recording_stub_write_dim(void) {
    return g_write_dim;
}
int recording_stub_write_size_first(void) {
    return g_write_size_first;
}

int recording_stub_delete_call_count(void) {
    return g_delete_call_count;
}
int recording_stub_delete_ctx(void) {
    return g_delete_ctx;
}
const char *recording_stub_delete_path(void) {
    return g_delete_path;
}

int recording_stub_iterate_call_count(void) {
    return g_iterate_call_count;
}
int recording_stub_iterate_aosctx(void) {
    return g_iterate_aosctx;
}
int recording_stub_iterate_step(void) {
    return g_iterate_step;
}

int recording_stub_occurrences_call_count(void) {
    return g_occurrences_call_count;
}
int recording_stub_occurrences_pctx_id(void) {
    return g_occurrences_pctx_id;
}
const char *recording_stub_occurrences_ids_name(void) {
    return g_occurrences_ids_name;
}

int recording_stub_filled_paths_call_count(void) {
    return g_filled_paths_call_count;
}
int recording_stub_filled_paths_pctx_id(void) {
    return g_filled_paths_pctx_id;
}
const char *recording_stub_filled_paths_dataobjectname(void) {
    return g_filled_paths_dataobjectname;
}
