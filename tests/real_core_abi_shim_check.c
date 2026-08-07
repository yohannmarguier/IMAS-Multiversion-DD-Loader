/* Compile the expected ABI contract against the shim's generated C header. */

#include <imas_mvdd_loader.h>

#include "real_core_abi_contract.h"

CHECK_ABI_STATUS_LAYOUT();
#define IMAS_ABI_SYMBOL(name, function_type)                                  \
    CHECK_ABI_FUNCTION(name, function_type);
#include "abi_symbols.def"
#undef IMAS_ABI_SYMBOL

int main(void) { return 0; }
