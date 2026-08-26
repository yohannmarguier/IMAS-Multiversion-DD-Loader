/* Issue #123: every public seam IMAS-Core can re-enter enters one shared
 * depth guard. Each scenario calls a harmless outer seam, then makes the
 * recording stub call a write seam back through a supplied shim function
 * pointer against a live mismatched context. Without the guard that inner
 * write refuses before it reaches Core; with it, it is forwarded verbatim. */

#include <string.h>

#ifndef RECORDING_STUB_PATH
#error "RECORDING_STUB_PATH must be defined by CMakeLists.txt"
#endif

#include "../support/shim_test_support.h"

typedef al_status_t (*data_fn)(int, const char *, const char *, void *, int, int, int *);
typedef void (*set_reentrant_data_fn)(data_fn, int, const char *, const char *);

static const char *const REENTRANT_FIELD = "time_slice/global_quantities/beta_tor_norm";
static const char *const REENTRANT_TIMEBASE = "time";

static void arm_reentrant_data(const char *setter_name, data_fn callback, int ctx_id) {
    set_reentrant_data_fn arm = (set_reentrant_data_fn)stub_symbol_or_die(setter_name);
    arm(callback, ctx_id, REENTRANT_FIELD, REENTRANT_TIMEBASE);
}

static void check_reentrant_write_forwarded(int mismatched_ctx) {
    CHECK(int_from_stub("recording_stub_reentrant_data_call_count") == 1);
    CHECK(int_from_stub("recording_stub_reentrant_data_status_code") == 0);
    CHECK(int_from_stub("recording_stub_reentrant_data_seen_ctx") == mismatched_ctx);
    CHECK(strcmp(string_from_stub("recording_stub_reentrant_data_seen_field"), REENTRANT_FIELD) ==
          0);
    CHECK(strcmp(string_from_stub("recording_stub_reentrant_data_seen_timebase"),
                 REENTRANT_TIMEBASE) == 0);
    CHECK(pointer_from_stub("recording_stub_reentrant_data_seen_data") ==
          pointer_from_stub("recording_stub_reentrant_data_expected_data"));
    CHECK(int_from_stub("recording_stub_reentrant_data_seen_datatype") == 52 /* DOUBLE_DATA */);
    CHECK(int_from_stub("recording_stub_reentrant_data_seen_dim") == 1);
    CHECK(pointer_from_stub("recording_stub_reentrant_data_seen_size") ==
          pointer_from_stub("recording_stub_reentrant_data_expected_size"));
    CHECK(int_from_stub("recording_stub_reentrant_data_seen_size_first") == 1);
    CHECK(loss_count(mismatched_ctx) == 0);
}

static void scenario_write_data_reentry_forwards_across_the_plugin_family(void) {
    int mismatched_ctx = open_mismatched_equilibrium();
    arm_reentrant_data("recording_stub_set_reentrant_write_data", al_plugin_write_data,
                       mismatched_ctx);
    double payload = 1.0;
    int size[1] = {1};
    CHECK(al_write_data(701, "outer/write", "", &payload, 52 /* DOUBLE_DATA */, 1, size).code ==
          0);
    check_reentrant_write_forwarded(mismatched_ctx);
    printf("reentry_guard_test write-data-reentry-forwards-across-the-plugin-family: "
           "a plugin write beneath an ordinary write bypassed mismatch refusal\n");
}

static void scenario_plugin_write_data_reentry_forwards_across_the_ordinary_family(void) {
    int mismatched_ctx = open_mismatched_equilibrium();
    arm_reentrant_data("recording_stub_set_reentrant_plugin_write_data", al_write_data,
                       mismatched_ctx);
    double payload = 1.0;
    int size[1] = {1};
    CHECK(al_plugin_write_data(702, "outer/plugin_write", "", &payload, 52 /* DOUBLE_DATA */,
                               1, size)
              .code == 0);
    check_reentrant_write_forwarded(mismatched_ctx);
    printf("reentry_guard_test plugin-write-data-reentry-forwards-across-the-ordinary-family: "
           "an ordinary write beneath a plugin write bypassed mismatch refusal\n");
}

static void scenario_delete_data_reentry_forwards_unchanged(void) {
    int mismatched_ctx = open_mismatched_equilibrium();
    arm_reentrant_data("recording_stub_set_reentrant_delete_data", al_plugin_write_data,
                       mismatched_ctx);
    CHECK(al_delete_data(703, "outer/delete").code == 0);
    check_reentrant_write_forwarded(mismatched_ctx);
    printf("reentry_guard_test delete-data-reentry-forwards-unchanged: a write beneath delete "
           "bypassed mismatch refusal\n");
}

static void scenario_write_plugins_metadata_reentry_forwards_unchanged(void) {
    int mismatched_ctx = open_mismatched_equilibrium();
    arm_reentrant_data("recording_stub_set_reentrant_write_plugins_metadata", al_plugin_write_data,
                       mismatched_ctx);
    CHECK(al_write_plugins_metadata(704).code == 0);
    check_reentrant_write_forwarded(mismatched_ctx);
    printf("reentry_guard_test write-plugins-metadata-reentry-forwards-unchanged: a write "
           "beneath metadata forwarding bypassed mismatch refusal\n");
}

static void scenario_bind_readback_plugins_reentry_forwards_unchanged(void) {
    int mismatched_ctx = open_mismatched_equilibrium();
    arm_reentrant_data("recording_stub_set_reentrant_bind_readback_plugins", al_plugin_write_data,
                       mismatched_ctx);
    CHECK(al_bind_readback_plugins(705).code == 0);
    check_reentrant_write_forwarded(mismatched_ctx);
    printf("reentry_guard_test bind-readback-plugins-reentry-forwards-unchanged: a write beneath "
           "readback binding bypassed mismatch refusal\n");
}

static void scenario_unbind_readback_plugins_reentry_forwards_unchanged(void) {
    int mismatched_ctx = open_mismatched_equilibrium();
    arm_reentrant_data("recording_stub_set_reentrant_unbind_readback_plugins", al_plugin_write_data,
                       mismatched_ctx);
    CHECK(al_unbind_readback_plugins(706).code == 0);
    check_reentrant_write_forwarded(mismatched_ctx);
    printf("reentry_guard_test unbind-readback-plugins-reentry-forwards-unchanged: a write beneath "
           "readback unbinding bypassed mismatch refusal\n");
}

int main(int argc, char **argv) {
    static const shim_test_scenario scenarios[] = {
        {"write-data-reentry-forwards-across-the-plugin-family",
         scenario_write_data_reentry_forwards_across_the_plugin_family},
        {"plugin-write-data-reentry-forwards-across-the-ordinary-family",
         scenario_plugin_write_data_reentry_forwards_across_the_ordinary_family},
        {"delete-data-reentry-forwards-unchanged", scenario_delete_data_reentry_forwards_unchanged},
        {"write-plugins-metadata-reentry-forwards-unchanged",
         scenario_write_plugins_metadata_reentry_forwards_unchanged},
        {"bind-readback-plugins-reentry-forwards-unchanged",
         scenario_bind_readback_plugins_reentry_forwards_unchanged},
        {"unbind-readback-plugins-reentry-forwards-unchanged",
         scenario_unbind_readback_plugins_reentry_forwards_unchanged},
    };
    return RUN_NAMED_SCENARIO(argc, argv, scenarios);
}
