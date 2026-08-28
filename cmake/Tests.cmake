# Test registration is split into included modules so the root build stays the
# single configuration entry point. include() preserves the current directory
# scope, therefore these modules retain the exact target, variable, and path
# behaviour of the former root-level block.

include_guard(GLOBAL)

include("${CMAKE_CURRENT_LIST_DIR}/tests/Common.cmake")

# `real-core` denotes profile membership: every test registered inside this
# bracket exists only when IMAS_MVDD_REAL_CORE_TESTS is on. Labelling the
# difference in the TESTS directory property, rather than naming each test,
# means no registration inside the bracket can be missed -- a bare add_test is
# labelled exactly like one going through add_real_core_test, which is what the
# four previously-unlabelled tests had fallen through.
#
# What this does not do is force a real-Core-only test *into* a bracket. A
# registration gated by its own if(IMAS_MVDD_REAL_CORE_TESTS) somewhere outside
# one is still unlabelled, and CMake cannot see the difference. The invariant is
# checked by comparing `ctest -N` between the two configure profiles against
# `ctest -N -L real-core`; keep the gated regions few enough to eyeball, which
# today means all of RealCore.cmake and one block in Abi.cmake.
function(imas_mvdd_begin_real_core_tests)
    get_property(_tests_before DIRECTORY PROPERTY TESTS)
    set_property(DIRECTORY PROPERTY IMAS_MVDD_REAL_CORE_TESTS_BEFORE "${_tests_before}")
endfunction()

function(imas_mvdd_end_real_core_tests)
    get_property(_tests_before DIRECTORY PROPERTY IMAS_MVDD_REAL_CORE_TESTS_BEFORE)
    get_property(_tests_after DIRECTORY PROPERTY TESTS)
    set(_real_core_tests "${_tests_after}")
    foreach(test IN LISTS _tests_before)
        list(REMOVE_ITEM _real_core_tests "${test}")
    endforeach()
    foreach(test IN LISTS _real_core_tests)
        set_property(TEST "${test}" APPEND PROPERTY LABELS real-core)
    endforeach()
endfunction()

include("${CMAKE_CURRENT_LIST_DIR}/tests/Abi.cmake")
include("${CMAKE_CURRENT_LIST_DIR}/tests/Shim.cmake")

if(IMAS_MVDD_REAL_CORE_TESTS)
    include("${CMAKE_CURRENT_LIST_DIR}/tests/RealCore.cmake")
endif()

get_property(_imas_mvdd_tests DIRECTORY PROPERTY TESTS)
list(LENGTH _imas_mvdd_tests _imas_mvdd_test_count)
set(_imas_mvdd_real_core_test_count 0)
foreach(test IN LISTS _imas_mvdd_tests)
    get_property(_labels TEST "${test}" PROPERTY LABELS)
    if("real-core" IN_LIST _labels)
        math(EXPR _imas_mvdd_real_core_test_count "${_imas_mvdd_real_core_test_count} + 1")
    endif()
endforeach()
message(STATUS "IMAS-MVDD: ${_imas_mvdd_test_count} tests registered, ${_imas_mvdd_real_core_test_count} of them real-core-gated and labelled")
