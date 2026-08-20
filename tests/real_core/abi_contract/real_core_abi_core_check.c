/* Compile the expected ABI contract against IMAS-Core's public C header. */

#include <al_lowlevel.h>

/* al_lowlevel.h leaves al_const.h's include commented out, so it must be
 * pulled in explicitly here to see the backend/op/pulse/data-type/error
 * constants and the const2str/err2str/getALVersion/getDDVersion
 * declarations used by the fallback check below. */
#include <al_const.h>
#include <string.h>

#include "include/real_core_abi_contract.h"

CHECK_ABI_STATUS_LAYOUT();
#define IMAS_ABI_SYMBOL(name, function_type)                                  \
    CHECK_ABI_FUNCTION(name, function_type);
#include "../../abi/abi_symbols.def"
#undef IMAS_ABI_SYMBOL

/* src/core/core_binding.rs hand-transcribes a subset of al_const.h's id-to-string
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
#include "../../abi/abi_fallback_constants.def"
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
#include "../../abi/abi_fallback_constants.def"

#undef IMAS_ABI_FALLBACK_CONST
#undef IMAS_ABI_FALLBACK_ERR
    if (strcmp(const2str((int)UNDEFINED_TIME), "UNDEFINED_TIME") != 0) {
        fprintf(stderr, "fallback const2str(UNDEFINED_TIME) mismatch: got \"%s\"\n",
                const2str((int)UNDEFINED_TIME));
        failures++;
    }

    /* The other half of the fallback contract: what happens to an id the map
     * has no entry for. src/core/core_binding.rs's fallback tables answer "" there, and
     * that is only faithful because al_const.cpp looks the id up and returns
     * "" on a miss rather than NULL, a placeholder, or a thrown exception. The
     * entries above pin every id that *is* mapped; nothing pinned the miss,
     * which is the arm every unrecognised id in the field takes.
     *
     * TIMERANGE_OP and FLEXBUFFERS_BACKEND are the documented cases: real,
     * defined IMAS-Core constants that alconst::constmap deliberately omits.
     * FLEXBUFFERS_BACKEND is a member of a C++ enum with no C-visible
     * spelling, so it appears here by value. */
    static const struct {
        const char *name;
        int id;
    } unmapped_ids[] = {
        {"TIMERANGE_OP", TIMERANGE_OP},
        {"FLEXBUFFERS_BACKEND", BACKEND_ID_0 + 6},
        {"an id no IMAS-Core vocabulary allocates", 987654},
    };
    for (size_t i = 0; i < sizeof unmapped_ids / sizeof unmapped_ids[0]; i++) {
        const char *mapped_name = const2str(unmapped_ids[i].id);
        if (mapped_name == NULL || strcmp(mapped_name, "") != 0) {
            fprintf(stderr,
                    "const2str(%s) must be \"\" for an unmapped id: got %s\n",
                    unmapped_ids[i].name,
                    mapped_name == NULL ? "NULL" : mapped_name);
            failures++;
        }
        const char *mapped_error = err2str(unmapped_ids[i].id);
        if (mapped_error == NULL || strcmp(mapped_error, "") != 0) {
            fprintf(stderr,
                    "err2str(%s) must be \"\" for an unmapped id: got %s\n",
                    unmapped_ids[i].name,
                    mapped_error == NULL ? "NULL" : mapped_error);
            failures++;
        }
    }
    return failures;
}
