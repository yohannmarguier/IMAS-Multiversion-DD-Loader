/* Shared harness for the C suites that drive the shim's public ABI.
 *
 * Every one of those suites needs the same four things: an assertion macro
 * that reports file and line, IMAS-Core's data-type codes, a way to read the
 * recording stub's call recorders, and a mismatched occurrence to read
 * through. Each suite grew its own copy — `CHECK` was defined twelve times,
 * `string_from_stub`/`int_from_stub` eight, the same dlopen-the-stub body
 * under three different names, and `open_mismatched_equilibrium` five times
 * with contracts that had begun to diverge.
 *
 * That is not merely repetition: a copy of `CHECK` carrying a literal
 * backslash-n instead of a newline reached four suites, so their failure
 * messages printed `\n` as text at exactly the moment someone was reading
 * them. One definition per thing is what stops the next such defect from
 * being copied rather than fixed. `tests/real_core_abi_contract.h` is the
 * precedent for a shared test header here.
 *
 * Include this with `#include "shim_test_support.h"` — the suites sit in this
 * directory, so no include path is needed. The stub-accessor half compiles
 * only where RECORDING_STUB_PATH is defined, which is what lets the real-Core
 * suites include this header for CHECK and the data-type codes alone. */

#ifndef IMAS_MVDD_SHIM_TEST_SUPPORT_H
#define IMAS_MVDD_SHIM_TEST_SUPPORT_H

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <dirent.h>
#include <sys/stat.h>

#include <imas_mvdd_loader.h>

#define CHECK(condition)                                                       \
    do {                                                                       \
        if (!(condition)) {                                                    \
            fprintf(stderr, "check failed at %s:%d: %s\n", __FILE__, __LINE__, \
                    #condition);                                               \
            exit(EXIT_FAILURE);                                                \
        }                                                                      \
    } while (0)

/* Asserts a call the scenario needs to have succeeded, reporting the code and
 * message an al_status_t failure carries — the two things a bare CHECK on
 * `.code == 0` throws away. */
static inline void shim_test_check_ok(al_status_t status, const char *expression, const char *file,
                                      int line) {
    if (status.code != 0) {
        fprintf(stderr, "call failed at %s:%d: %s: code=%d message=%s\n", file, line, expression,
                status.code, status.message);
        exit(EXIT_FAILURE);
    }
}

#define CHECK_OK(expression) shim_test_check_ok((expression), #expression, __FILE__, __LINE__)

/* Issue #58 AC3: "Every refusal message begins with IMAS-MVDD: and orders
 * reason, DD path, HLI DD version, then stored DD version." Asserted here as
 * one exact string rather than as four independent substring searches, so a
 * seam that emitted the fields out of order, or dropped one, cannot pass.
 *
 * Shared because the contract is one contract: it used to hold at the read
 * seam only, while the arraystruct opens and the write/delete refusals emitted
 * the reason alone, and each suite asserting its own seam's half of it is how
 * that divergence went unnoticed. */
static inline void shim_test_check_refusal_message(al_status_t status, const char *reason,
                                                   const char *dd_path, const char *hli_version,
                                                   const char *stored_version, const char *file,
                                                   int line) {
    char expected[MAX_ERR_MSG_LEN];
    snprintf(expected, sizeof expected,
             "IMAS-MVDD: %s; DD path: %s; HLI DD version: %s; stored DD version: %s", reason,
             dd_path, hli_version, stored_version);
    if (strcmp(status.message, expected) != 0) {
        fprintf(stderr, "refusal message mismatch at %s:%d:\n  expected: %s\n  actual:   %s\n", file,
                line, expected, status.message);
        exit(EXIT_FAILURE);
    }
}

#define CHECK_REFUSAL_MESSAGE(status, reason, dd_path, hli_version, stored_version)               \
    shim_test_check_refusal_message((status), (reason), (dd_path), (hli_version),                 \
                                    (stored_version), __FILE__, __LINE__)

/* One scenario a suite can be asked to run, under the name ctest registers it
 * as. Every suite here is one process per scenario — both the HLI DD version
 * latch and the context registry are process-wide — so `argv[1]` selects which
 * one runs.
 *
 * Each suite used to hand-roll that selection as a chain of strcmp calls plus a
 * hand-maintained usage string, which meant a scenario's name was written out
 * up to four times: in the usage text, in the strcmp, in the function name, and
 * in the CMake registration. Three of those four are now one table entry, and
 * the fourth (CMake) is the only spelling a rename still has to chase. The
 * usage text can no longer disagree with the dispatch, because it is generated
 * from the same table. */
typedef struct {
    const char *name;
    void (*run)(void);
} shim_test_scenario;

/* Runs the scenario `argv[1]` names and returns the process's exit status: 0 if
 * it ran (a scenario that fails an assertion exits from inside CHECK), or 2 for
 * a missing or unknown name, listing every scenario the suite has. */
static inline int run_named_scenario(int argc, char **argv, const shim_test_scenario *scenarios,
                                    size_t count) {
    if (argc == 2) {
        for (size_t i = 0; i < count; ++i) {
            if (strcmp(argv[1], scenarios[i].name) == 0) {
                scenarios[i].run();
                return 0;
            }
        }
        fprintf(stderr, "unknown scenario: %s\n", argv[1]);
    }

    fprintf(stderr, "usage: %s <scenario>\nscenarios:\n", argv[0]);
    for (size_t i = 0; i < count; ++i) {
        fprintf(stderr, "  %s\n", scenarios[i].name);
    }
    return 2;
}

#define RUN_NAMED_SCENARIO(argc, argv, scenarios)                                                  \
    run_named_scenario((argc), (argv), (scenarios), sizeof(scenarios) / sizeof(*(scenarios)))

/* IMAS-Core's data-type codes, spelled out because the recording-stub profile
 * deliberately acquires no IMAS-Core and so has no al_const.h to include.
 * The values are `DATA_TYPE_0` (50) plus an offset, per IMAS-Core's
 * al_defs.h.in — not small ordinals, which is an easy and silent mistake to
 * make in a test that passes a bare literal, and one issue #69 had to sweep
 * out of seven call sites. The real-Core suites include al_const.h and use
 * its own CHAR_DATA/INTEGER_DATA/DOUBLE_DATA/COMPLEX_DATA instead. */
#define IMAS_CHAR_DATA 50
#define IMAS_INTEGER_DATA 51
#define IMAS_DOUBLE_DATA 52
#define IMAS_COMPLEX_DATA 53

/* The recording stub's own data-event kinds, drained through
 * `recording_stub_data_event_kind_at`. The stub declares them in a
 * file-private enum (`tests/stub/recording_stub.c`, RECORDING_STUB_DATA_EVENT_*)
 * and `tests/stub/` ships no header, so a suite reaching them across the
 * dlopen boundary has no way to link against the definition. Naming them once
 * here is the closest thing to that link, and it keeps a bare `1` from
 * standing in a comparison under a comment claiming to be a constant. */
#define IMAS_MVDD_STUB_DATA_EVENT_READ 1
#define IMAS_MVDD_STUB_DATA_EVENT_DELETE 2

/* The loss-file scenarios see only the published on-disk report. They clean
 * their scenario-specific directory before opening an occurrence, then locate
 * the one report the process produced without depending on its clock or PID. */
static inline const char *loss_log_directory(void) {
    const char *directory = getenv("IMAS_MVDD_LOSS_LOG_DIR");
    CHECK(directory != NULL);
    return directory;
}

static inline int is_loss_log_name(const char *name) {
    size_t length = strlen(name);
    return strncmp(name, "imas-mvdd-loss-", 15) == 0 && length > 19
           && strcmp(name + length - 4, ".txt") == 0;
}

static inline void clear_loss_log_directory(void) {
    const char *directory = loss_log_directory();
    (void)mkdir(directory, 0700);
    DIR *dir = opendir(directory);
    CHECK(dir != NULL);
    struct dirent *entry;
    while ((entry = readdir(dir)) != NULL) {
        if (is_loss_log_name(entry->d_name)) {
            char path[1024];
            CHECK(snprintf(path, sizeof path, "%s/%s", directory, entry->d_name) < (int)sizeof path);
            CHECK(remove(path) == 0);
        }
    }
    CHECK(closedir(dir) == 0);
}

static inline char *single_loss_log_path_or_null(void) {
    const char *directory = loss_log_directory();
    DIR *dir = opendir(directory);
    CHECK(dir != NULL);
    char *result = NULL;
    struct dirent *entry;
    while ((entry = readdir(dir)) != NULL) {
        if (!is_loss_log_name(entry->d_name)) {
            continue;
        }
        CHECK(result == NULL);
        size_t length = strlen(directory) + 1 + strlen(entry->d_name) + 1;
        result = malloc(length);
        CHECK(result != NULL);
        snprintf(result, length, "%s/%s", directory, entry->d_name);
    }
    CHECK(closedir(dir) == 0);
    return result;
}

static inline char *read_loss_log(void) {
    char *path = single_loss_log_path_or_null();
    CHECK(path != NULL);
    FILE *file = fopen(path, "rb");
    CHECK(file != NULL);
    CHECK(fseek(file, 0, SEEK_END) == 0);
    long length = ftell(file);
    CHECK(length >= 0);
    CHECK(fseek(file, 0, SEEK_SET) == 0);
    char *contents = malloc((size_t)length + 1);
    CHECK(contents != NULL);
    CHECK(fread(contents, 1, (size_t)length, file) == (size_t)length);
    contents[length] = '\0';
    CHECK(fclose(file) == 0);
    free(path);
    return contents;
}

/* The loss log a caller drains through the shim's four owned exports
 * (ADR 0012). These four helpers were copied into five suites — `loss_count`
 * five times, `check_loss_at` four, and the newest copy of `check_loss_at`
 * with a fifth parameter the other three lacked, which is precisely the
 * divergence this header exists to stop. The five-parameter form is the one
 * that survives: issue #124 put the operation on every entry, so there is no
 * such thing as an entry whose operation is not worth asserting, and a read
 * site passing IMAS_MVDD_LOSS_OPERATION_READ says so where it used to say
 * nothing. That also retires the separate `check_loss_operation_at` two suites
 * carried: its three call sites each sat directly beneath a `check_loss_at` on
 * the same context and index, so folding the parameter in covers them without
 * losing an assertion. */
static inline int loss_count(int ctx_id) {
    int count = -1;
    CHECK(imas_mvdd_context_loss_count(ctx_id, &count).code == 0);
    return count;
}

static inline void check_no_loss_entry(int ctx_id) { CHECK(loss_count(ctx_id) == 0); }

/* ADR 0016 decision 12: a write produces no certainly-lossy verdict. Drains
 * the whole log rather than one index, because the claim is about every entry
 * a write can put there. The reachability half of it is pinned in Rust, at the
 * site that chooses the fidelity
 * (`interpose::tests::a_declared_lossy_candidate_plan_still_retains_a_potential_loss`);
 * this is the observable half, and if it ever fires the answer is to go add
 * real coverage for the certain bucket, not to relax it (ADR 0011). */
static inline void check_no_write_lossy_verdict(int ctx_id) {
    int count = -1;
    CHECK(imas_mvdd_context_loss_count(ctx_id, &count).code == 0);
    for (int index = 0; index < count; ++index) {
        char path[256] = {0};
        int verdict = -1;
        CHECK(imas_mvdd_context_loss_at(ctx_id, index, path, sizeof(path), &verdict).code == 0);
        if (verdict == IMAS_MVDD_FIDELITY_LOSSY) {
            fprintf(stderr,
                    "a write-side LOSSY verdict needs real coverage before it is allowed: "
                    "entry %d is %s\n",
                    index, path);
            exit(EXIT_FAILURE);
        }
    }
}

static inline void check_loss_at(int ctx_id, int index, const char *expected_path,
                                 int expected_verdict, int expected_operation) {
    char path[256] = {0};
    int verdict = -1;
    int operation = -1;
    CHECK(imas_mvdd_context_loss_at(ctx_id, index, path, sizeof(path), &verdict).code == 0);
    CHECK(strcmp(path, expected_path) == 0);
    CHECK(verdict == expected_verdict);
    CHECK(imas_mvdd_context_loss_operation_at(ctx_id, index, &operation).code == 0);
    CHECK(operation == expected_operation);
}

#ifdef RECORDING_STUB_PATH

#include <dlfcn.h>

typedef const char *(*shim_test_string_accessor_fn)(void);
typedef int (*shim_test_int_accessor_fn)(void);
typedef double (*shim_test_double_accessor_fn)(void);
typedef double (*shim_test_double_at_accessor_fn)(int);
typedef const void *(*shim_test_pointer_accessor_fn)(void);

/* The recording stub is already loaded as the shim's IMAS-Core; opening it
 * again here only yields a handle onto that same instance, so the recorders
 * read below are the ones the shim's own calls wrote. */
static inline void *open_recording_stub(void) {
    void *stub = dlopen(RECORDING_STUB_PATH, RTLD_NOW | RTLD_LOCAL);
    if (stub == NULL) {
        fprintf(stderr, "failed to open recording stub: %s\n", dlerror());
        abort();
    }
    return stub;
}

static inline void *stub_symbol_or_die(const char *symbol_name) {
    void *symbol = dlsym(open_recording_stub(), symbol_name);
    if (symbol == NULL) {
        fprintf(stderr, "recording stub has no symbol '%s': %s\n", symbol_name, dlerror());
        abort();
    }
    return symbol;
}

static inline const char *string_from_stub(const char *symbol_name) {
    return ((shim_test_string_accessor_fn)stub_symbol_or_die(symbol_name))();
}

static inline int int_from_stub(const char *symbol_name) {
    return ((shim_test_int_accessor_fn)stub_symbol_or_die(symbol_name))();
}

static inline double double_from_stub(const char *symbol_name) {
    return ((shim_test_double_accessor_fn)stub_symbol_or_die(symbol_name))();
}

static inline double double_at_from_stub(const char *symbol_name, int index) {
    return ((shim_test_double_at_accessor_fn)stub_symbol_or_die(symbol_name))(index);
}

static inline const void *pointer_from_stub(const char *symbol_name) {
    return ((shim_test_pointer_accessor_fn)stub_symbol_or_die(symbol_name))();
}

/* Opens a pulse and a global action on `dataobjectname` whose supplied stamp
 * makes the stored DD version differ from the latched HLI one, leaving the
 * resulting mismatch record live for the caller's whole scenario. Returns the
 * operation context; `pulse_ctx_out`, when non-NULL, receives the pulse
 * context the seams addressed by it need.
 *
 * This deliberately asserts nothing beyond both calls succeeding. A suite that
 * wants more — that the mismatch record really is converting, say — layers its
 * own named helper on top rather than widening this one, because the stub's
 * call recorders are shared state and an extra call made here would perturb
 * every scenario that counts them. */
static inline int open_mismatched_occurrence(const char *dataobjectname, int *pulse_ctx_out) {
    int pulse_ctx = -1;
    CHECK(al_begin_dataentry_action("imas:hdf5?path=/tmp/pulse", 7, &pulse_ctx).code == 0);

    int operation_ctx = -1;
    CHECK(al_begin_global_action(pulse_ctx, dataobjectname, "", 30, &operation_ctx).code == 0);

    if (pulse_ctx_out != NULL) {
        *pulse_ctx_out = pulse_ctx;
    }
    return operation_ctx;
}

static inline int open_mismatched_equilibrium(void) {
    return open_mismatched_occurrence("equilibrium", NULL);
}

#endif /* RECORDING_STUB_PATH */

#endif /* IMAS_MVDD_SHIM_TEST_SUPPORT_H */
