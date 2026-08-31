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
