#ifndef IMAS_MVDD_REAL_CORE_ABI_CONTRACT_H
#define IMAS_MVDD_REAL_CORE_ABI_CONTRACT_H

#include <stdbool.h>
#include <stddef.h>

enum {
    ABI_EXPECTED_MAXDIM = 7,
    ABI_EXPECTED_MAX_ERR_MSG_LEN = 256,
};

struct abi_expected_status_layout {
    int code;
    char message[ABI_EXPECTED_MAX_ERR_MSG_LEN];
};

typedef al_status_t (*AbiContextInfoFn)(int, char **);
typedef al_status_t (*AbiGetBackendIdFn)(int, int *);
typedef al_status_t (*AbiBuildUriFromLegacyParametersFn)(
    int, int, int, const char *, const char *, const char *, const char *,
    char **);
typedef const char *(*AbiStringLookupFn)(int);
typedef const char *(*AbiVersionAccessorFn)(void);
typedef al_status_t (*AbiBeginDataentryActionFn)(const char *, int, int *);
typedef al_status_t (*AbiClosePulseFn)(int, int);
typedef al_status_t (*AbiBeginGlobalActionFn)(int, const char *, const char *,
                                               int, int *);
typedef al_status_t (*AbiBeginSliceActionFn)(int, const char *, int, double,
                                              int, int *);
typedef al_status_t (*AbiBeginTimerangeActionFn)(
    int, const char *, int, double, double, const double *, const int *, int,
    int *);
typedef al_status_t (*AbiBeginArraystructActionFn)(int, const char *,
                                                    const char *, int *, int *);
typedef al_status_t (*AbiEndActionFn)(int);
typedef al_status_t (*AbiReadDataFn)(int, const char *, const char *, void **,
                                      int, int, int *);
typedef al_status_t (*AbiWriteDataFn)(int, const char *, const char *, void *,
                                       int, int, int *);
typedef al_status_t (*AbiDeleteDataFn)(int, const char *);
typedef al_status_t (*AbiIterateOverArraystructFn)(int, int);
typedef al_status_t (*AbiGetOccurrencesFn)(int, const char *, int **, int *);
typedef al_status_t (*AbiListFilledPathsFn)(int, const char *, char ***, int *);
typedef al_status_t (*AbiPluginNameFn)(const char *);
typedef al_status_t (*AbiBindPluginFn)(const char *, const char *);
typedef al_status_t (*AbiPluginContextFn)(int);
typedef al_status_t (*AbiIsPluginRegisteredFn)(const char *, bool *);
typedef al_status_t (*AbiSetvalueParameterPluginFn)(const char *, int, int,
                                                     int *, void *,
                                                     const char *);
typedef al_status_t (*AbiSetvalueIntScalarParameterPluginFn)(const char *, int,
                                                              const char *);
typedef al_status_t (*AbiSetvalueDoubleScalarParameterPluginFn)(
    const char *, double, const char *);

#define CHECK_ABI_STATUS_LAYOUT()                                              \
    _Static_assert(sizeof(al_status_t) ==                                     \
                       sizeof(struct abi_expected_status_layout),             \
                   "ABI status size mismatch");                              \
    _Static_assert(_Alignof(al_status_t) ==                                   \
                       _Alignof(struct abi_expected_status_layout),           \
                   "ABI status alignment mismatch");                         \
    _Static_assert(offsetof(al_status_t, code) ==                             \
                       offsetof(struct abi_expected_status_layout, code),     \
                   "ABI status code offset mismatch");                       \
    _Static_assert(offsetof(al_status_t, message) ==                          \
                       offsetof(struct abi_expected_status_layout, message),  \
                   "ABI status message offset mismatch");                    \
    _Static_assert(MAXDIM == ABI_EXPECTED_MAXDIM,                             \
                   "ABI constant mismatch: MAXDIM");                         \
    _Static_assert(MAX_ERR_MSG_LEN == ABI_EXPECTED_MAX_ERR_MSG_LEN,           \
                   "ABI constant mismatch: MAX_ERR_MSG_LEN")

#define CHECK_ABI_FUNCTION(name, function_type)                               \
    _Static_assert(                                                          \
        _Generic(&(name), function_type: 1, default: 0),                     \
        "ABI signature mismatch: " #name)

#endif
