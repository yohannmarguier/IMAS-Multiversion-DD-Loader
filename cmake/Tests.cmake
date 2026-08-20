# Test registration is split into included modules so the root build stays the
# single configuration entry point. include() preserves the current directory
# scope, therefore these modules retain the exact target, variable, and path
# behaviour of the former root-level block.

include_guard(GLOBAL)

include("${CMAKE_CURRENT_LIST_DIR}/tests/Common.cmake")
include("${CMAKE_CURRENT_LIST_DIR}/tests/Abi.cmake")
include("${CMAKE_CURRENT_LIST_DIR}/tests/Shim.cmake")

if(IMAS_MVDD_REAL_CORE_TESTS)
    include("${CMAKE_CURRENT_LIST_DIR}/tests/RealCore.cmake")
endif()
