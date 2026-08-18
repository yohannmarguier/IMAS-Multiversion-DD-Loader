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

#ifdef RECORDING_STUB_PATH

#include <dlfcn.h>

typedef const char *(*shim_test_string_accessor_fn)(void);
typedef int (*shim_test_int_accessor_fn)(void);
typedef double (*shim_test_double_accessor_fn)(void);
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
