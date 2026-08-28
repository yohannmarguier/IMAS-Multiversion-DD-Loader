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
#
# The brackets do not nest, and an unmatched one would mislabel silently rather
# than fail, so each end consumes the snapshot its begin left and both refuse
# the mismatched case outright.
function(imas_mvdd_begin_real_core_tests)
    get_property(_open DIRECTORY PROPERTY IMAS_MVDD_REAL_CORE_TESTS_BEFORE SET)
    if(_open)
        message(FATAL_ERROR
            "imas_mvdd_begin_real_core_tests() called while a bracket is already "
            "open; these do not nest")
    endif()
    get_property(_tests_before DIRECTORY PROPERTY TESTS)
    set_property(DIRECTORY PROPERTY IMAS_MVDD_REAL_CORE_TESTS_BEFORE "${_tests_before}")
endfunction()

function(imas_mvdd_end_real_core_tests)
    get_property(_open DIRECTORY PROPERTY IMAS_MVDD_REAL_CORE_TESTS_BEFORE SET)
    if(NOT _open)
        message(FATAL_ERROR
            "imas_mvdd_end_real_core_tests() called with no bracket open; every "
            "test registered so far would be labelled real-core")
    endif()
    get_property(_tests_before DIRECTORY PROPERTY IMAS_MVDD_REAL_CORE_TESTS_BEFORE)
    set_property(DIRECTORY PROPERTY IMAS_MVDD_REAL_CORE_TESTS_BEFORE)

    get_property(_real_core_tests DIRECTORY PROPERTY TESTS)
    if(_tests_before)
        list(REMOVE_ITEM _real_core_tests ${_tests_before})
    endif()
    foreach(test IN LISTS _real_core_tests)
        set_property(TEST "${test}" APPEND PROPERTY LABELS real-core)
    endforeach()
endfunction()

include("${CMAKE_CURRENT_LIST_DIR}/tests/Abi.cmake")
include("${CMAKE_CURRENT_LIST_DIR}/tests/Shim.cmake")

if(IMAS_MVDD_REAL_CORE_TESTS)
    include("${CMAKE_CURRENT_LIST_DIR}/tests/RealCore.cmake")
endif()

# Report the totals that used to be written down in tests/README.md and
# CLAUDE.md, where they went stale silently. In a function so the tallying
# locals do not outlive the count.
function(imas_mvdd_report_test_counts)
    get_property(tests DIRECTORY PROPERTY TESTS)
    list(LENGTH tests total)
    set(labelled 0)
    foreach(test IN LISTS tests)
        get_property(labels TEST "${test}" PROPERTY LABELS)
        if("real-core" IN_LIST labels)
            math(EXPR labelled "${labelled} + 1")
        endif()
    endforeach()
    message(STATUS
        "IMAS-MVDD: ${total} tests registered, ${labelled} of them "
        "real-core-gated and labelled")
endfunction()

imas_mvdd_report_test_counts()
