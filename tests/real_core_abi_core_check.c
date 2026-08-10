/* Compile the expected ABI contract against IMAS-Core's public C header. */

#include <al_lowlevel.h>

/* al_lowlevel.h leaves al_const.h's include commented out, so it must be
 * pulled in explicitly here to see the backend/op/pulse/data-type/error
 * constants and the const2str/err2str/getALVersion/getDDVersion
 * declarations used by the fallback check below. */
#include <al_const.h>
#include <string.h>

#include "real_core_abi_contract.h"

CHECK_ABI_STATUS_LAYOUT();
#define IMAS_ABI_SYMBOL(name, function_type)                                  \
    CHECK_ABI_FUNCTION(name, function_type);
#include "abi_symbols.def"
#undef IMAS_ABI_SYMBOL

/* src/resolve.rs hand-transcribes a subset of al_const.h's id-to-string
 * tables to answer const2str/err2str/getALVersion/getDDVersion when the
 * loaded IMAS-Core's major version cannot be trusted (see the ADR's
 * consequences section). That transcription is a second hand-written
 * surface, distinct from the signature contract above, so it gets its own
 * check: every fallback id must equal its real upstream macro value
 * (checked at compile time), and every fallback string must equal what the
 * real, linked const2str/err2str actually returns for that id (checked at
 * run time, since the id-to-string tables live in upstream's C++-only
 * al_const.h and are not reachable from a C header comparison). */
#define IMAS_ABI_FALLBACK_CONST(macro_name, expected_value, expected_string) \
    _Static_assert((macro_name) == (expected_value),                        \
                    "ABI fallback constant mismatch: " #macro_name);
#define IMAS_ABI_FALLBACK_ERR(macro_name, expected_value, expected_string)   \
    IMAS_ABI_FALLBACK_CONST(macro_name, expected_value, expected_string)
#include "abi_fallback_constants.def"
#undef IMAS_ABI_FALLBACK_CONST
#undef IMAS_ABI_FALLBACK_ERR

_Static_assert((int)UNDEFINED_TIME == -999,
               "ABI fallback constant mismatch: UNDEFINED_TIME");

int check_fallback_strings(void) {
    int failures = 0;
#define IMAS_ABI_FALLBACK_CONST(macro_name, expected_value, expected_string) \
    if (strcmp(const2str(macro_name), expected_string) != 0) {              \
        fprintf(stderr, "fallback const2str(%s) mismatch: got \"%s\"\n",    \
                #macro_name, const2str(macro_name));                        \
        failures++;                                                         \
    }
#define IMAS_ABI_FALLBACK_ERR(macro_name, expected_value, expected_string)  \
    if (strcmp(err2str(macro_name), expected_string) != 0) {                \
        fprintf(stderr, "fallback err2str(%s) mismatch: got \"%s\"\n",      \
                #macro_name, err2str(macro_name));                          \
        failures++;                                                        \
    }
#include "abi_fallback_constants.def"
#undef IMAS_ABI_FALLBACK_CONST
#undef IMAS_ABI_FALLBACK_ERR
    if (strcmp(const2str((int)UNDEFINED_TIME), "UNDEFINED_TIME") != 0) {
        fprintf(stderr, "fallback const2str(UNDEFINED_TIME) mismatch: got \"%s\"\n",
                const2str((int)UNDEFINED_TIME));
        failures++;
    }
    return failures;
}
