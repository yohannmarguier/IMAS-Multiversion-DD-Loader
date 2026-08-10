/* Compile the expected ABI contract against the shim's generated C header. */

#include <imas_mvdd_loader.h>

#include "real_core_abi_contract.h"

CHECK_ABI_STATUS_LAYOUT();
#define IMAS_ABI_SYMBOL(name, function_type)                                  \
    CHECK_ABI_FUNCTION(name, function_type);
#include "abi_symbols.def"
#undef IMAS_ABI_SYMBOL

/* Defined in real_core_abi_core_check.c, which alone includes IMAS-Core's
 * real al_const.h and can call its real const2str/err2str. Runs the
 * runtime half of the fallback-table check documented there. */
int check_fallback_strings(void);

int main(void) { return check_fallback_strings(); }
