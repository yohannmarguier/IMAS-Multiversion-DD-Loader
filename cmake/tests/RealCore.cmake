# Tests that exercise an acquired IMAS-Core implementation.

include_guard(GLOBAL)

imas_mvdd_begin_real_core_tests()

# --- Real IMAS-Core test: the tracer against the genuine article (issue #4) ---
#
# The same shim call as scenario_success, but against the IMAS-Core
# acquired above instead of the recording stub: al_context_info must
# reach a real implementation, not only a substitute for one. ctxID 0
# is the one value real IMAS-Core answers deterministically without any
# context ever having been opened (al_lowlevel.cpp's "NULL context"
# branch), which is what makes this runnable with no fixture beyond the
# acquired library itself. Explicitly remove the override so this test
# proves the shim reaches the acquired Core by bare soname.
add_real_core_test(runtime-binding-real-core $<TARGET_FILE:runtime_binding_test> real-core)
if(IMAS_CORE_BUILT_FROM_SOURCE)
    # Built from source, EXCLUDE_FROM_ALL: nothing else pulls it into the
    # default build target, so the test binary that needs it must.
    add_dependencies(runtime_binding_test ${IMAS_CORE_AL_TARGET})
endif()

# A real-Core run cannot use the recording stub's private accessors or
# fabricated context IDs. The acquired Core target already provides the
# matching headers and library in every acquisition mode, so drive all
# data, plugin, utility, and version seams through a legal HDF5 lifecycle
# unconditionally rather than making this coverage opt-in.
add_library(real_core_test_plugin MODULE
    "${CMAKE_CURRENT_SOURCE_DIR}/tests/real_core/real_core_test_plugin.cpp")
target_include_directories(real_core_test_plugin PRIVATE ${_imas_core_include_dirs})
target_compile_features(real_core_test_plugin PRIVATE cxx_std_17)
set_target_properties(real_core_test_plugin PROPERTIES
    PREFIX ""
    OUTPUT_NAME "mvddtest_plugin"
    SUFFIX ".so"
    LIBRARY_OUTPUT_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/plugins")

add_executable(real_core_forwarding_test
    "${CMAKE_CURRENT_SOURCE_DIR}/tests/real_core/real_core_forwarding_test.c")
find_package(HDF5 COMPONENTS C REQUIRED)
target_include_directories(real_core_forwarding_test PRIVATE
    ${_imas_core_include_dirs}
    ${HDF5_C_INCLUDE_DIRS})
target_compile_definitions(real_core_forwarding_test PRIVATE
    "REAL_CORE_TEST_PLUGIN_DIR=\"$<TARGET_FILE_DIR:real_core_test_plugin>\""
    "REAL_CORE_TEST_PLUGIN_NAME=\"mvddtest\""
    "EQUILIBRIUM_FIXTURE_DIR=\"${CMAKE_CURRENT_SOURCE_DIR}/imas-python-fixtures/fixtures\"")
# The test uses HDF5 only to seed malformed on-disk DD-version metadata;
# the slice/time-range operations themselves use the public shim/Core ABI.
target_link_libraries(real_core_forwarding_test PRIVATE
    imas_mvdd_loader
    ${HDF5_C_LIBRARIES})
add_dependencies(real_core_forwarding_test
    imas_mvdd_capi
    real_core_test_plugin)
if(IMAS_CORE_BUILT_FROM_SOURCE)
    add_dependencies(real_core_test_plugin ${IMAS_CORE_AL_TARGET})
endif()
set_target_properties(real_core_forwarding_test PROPERTIES
    BUILD_RPATH "${IMAS_MVDD_STAGE_DIR}/lib")

add_real_core_test(runtime-binding-real-core-forwarding
    $<TARGET_FILE:real_core_forwarding_test>)

# --- Issue #54: the first bidirectional translated read, against the
# checked-in equilibrium HDF5 fixture pair rather than a throwaway pulse.
add_executable(equilibrium_read_test
    "${CMAKE_CURRENT_SOURCE_DIR}/tests/real_core/equilibrium_read_test.c")
target_include_directories(equilibrium_read_test PRIVATE
    ${_imas_core_include_dirs}
    ${HDF5_C_INCLUDE_DIRS})
target_compile_definitions(equilibrium_read_test PRIVATE
    "EQUILIBRIUM_FIXTURE_DIR=\"${CMAKE_CURRENT_SOURCE_DIR}/imas-python-fixtures/fixtures\"")
target_link_libraries(equilibrium_read_test PRIVATE
    imas_mvdd_loader
    ${HDF5_C_LIBRARIES})
add_dependencies(equilibrium_read_test imas_mvdd_capi)
set_target_properties(equilibrium_read_test PROPERTIES
    BUILD_RPATH "${IMAS_MVDD_STAGE_DIR}/lib")

# The HLI DD version latch is process-wide, so each scenario is its own
# ctest process (mirrors version_discovery_test.c). Scenarios opening the
# same fixture directory share a resource lock: HDF5's own file locking
# makes two concurrent opens of the same pulse unreliable, and ctest may
# otherwise run tests in parallel.
add_real_core_test(equilibrium-read-reverse-reads-renamed-value
    $<TARGET_FILE:equilibrium_read_test> reverse-reads-renamed-value-through-own-spelling
    RESOURCE_LOCK equilibrium-fixture-dd-4.1.1)

add_real_core_test(equilibrium-read-forward-reads-renamed-value
    $<TARGET_FILE:equilibrium_read_test> forward-reads-renamed-value-through-own-spelling
    RESOURCE_LOCK equilibrium-fixture-dd-3.39.0)

# Issue #129 keeps the safe leaf-delete proof at the spelling-observable
# recording-stub boundary. This real-Core probe retains stamp protection,
# while a real context lifecycle leaves conversion working through whatever is
# open.
add_real_core_test(equilibrium-read-forward-delete-refuses-stamp-removal
    $<TARGET_FILE:equilibrium_read_test> forward-delete-refuses-stamp-removal
    RESOURCE_LOCK equilibrium-fixture-dd-3.39.0)

add_real_core_test(equilibrium-read-forward-context-lifecycle-keeps-conversion-live
    $<TARGET_FILE:equilibrium_read_test> forward-context-lifecycle-keeps-conversion-live
    RESOURCE_LOCK equilibrium-fixture-dd-3.39.0)

add_real_core_test(equilibrium-read-forward-merged-read-falls-through-to-stored-alias
    $<TARGET_FILE:equilibrium_read_test> forward-merged-read-falls-through-to-stored-alias
    RESOURCE_LOCK equilibrium-fixture-dd-3.39.0)

add_real_core_test(equilibrium-read-reverse-merged-read-resolves-single-stored-destination
    $<TARGET_FILE:equilibrium_read_test> reverse-merged-read-resolves-single-stored-destination
    RESOURCE_LOCK equilibrium-fixture-dd-4.1.1)

# Issue #69: the refusal half of the matrix. Both scenarios open a real
# pulse whose contents are never reached — the assertion is that the shim
# stops before IMAS-Core, in whichever direction the artifact says it must.
add_real_core_test(equilibrium-read-reverse-refuses-unservable-paths
    $<TARGET_FILE:equilibrium_read_test> reverse-refuses-unservable-paths
    RESOURCE_LOCK equilibrium-fixture-dd-4.1.1)

add_real_core_test(equilibrium-read-forward-refuses-unservable-paths
    $<TARGET_FILE:equilibrium_read_test> forward-refuses-unservable-paths
    RESOURCE_LOCK equilibrium-fixture-dd-3.39.0)

add_real_core_test(equilibrium-read-reverse-split-read-uses-first-destination-and-flips-value
    $<TARGET_FILE:equilibrium_read_test> reverse-split-read-uses-first-destination-and-flips-value
    RESOURCE_LOCK equilibrium-fixture-dd-4.1.1)

add_real_core_test(equilibrium-read-forward-split-read-uses-single-source-and-flips-value
    $<TARGET_FILE:equilibrium_read_test> forward-split-read-uses-single-source-and-flips-value
    RESOURCE_LOCK equilibrium-fixture-dd-3.39.0)

add_real_core_test(equilibrium-read-reverse-reads-renamed-nested-container-field
    $<TARGET_FILE:equilibrium_read_test> reverse-reads-renamed-nested-container-field
    RESOURCE_LOCK equilibrium-fixture-dd-4.1.1)

add_real_core_test(equilibrium-read-forward-reads-renamed-nested-container-field
    $<TARGET_FILE:equilibrium_read_test> forward-reads-renamed-nested-container-field
    RESOURCE_LOCK equilibrium-fixture-dd-3.39.0)

add_real_core_test(equilibrium-read-reverse-sign-flip-applies-through-nested-container
    $<TARGET_FILE:equilibrium_read_test> reverse-sign-flip-applies-through-nested-container
    RESOURCE_LOCK equilibrium-fixture-dd-4.1.1)

add_real_core_test(equilibrium-read-forward-sign-flip-applies-through-nested-container
    $<TARGET_FILE:equilibrium_read_test> forward-sign-flip-applies-through-nested-container
    RESOURCE_LOCK equilibrium-fixture-dd-3.39.0)

add_real_core_test(equilibrium-read-same-version-is-unaffected
    $<TARGET_FILE:equilibrium_read_test> same-version-read-is-unaffected
    RESOURCE_LOCK equilibrium-fixture-dd-4.1.1)

add_real_core_test(equilibrium-read-conversion-disabled-is-unaffected
    $<TARGET_FILE:equilibrium_read_test> conversion-disabled-read-is-unaffected
    RESOURCE_LOCK equilibrium-fixture-dd-4.1.1)

# This scenario opens only a unique copied fixture directory, so it cannot
# race the scenarios above that open a checked-in pulse and need HDF5 locks.
add_real_core_test(equilibrium-read-copied-fixture-harness-reproves-renamed-read
    $<TARGET_FILE:equilibrium_read_test> copied-fixture-harness-reproves-renamed-read)

# Issue #228 keeps the graph-selected source isolated from the production
# library while exercising its 3.42.0 coexistence map through real Core and
# the established copied-fixture/HDF5 oracle.
add_executable(graph_coexistence_oracle_test
    "${CMAKE_CURRENT_SOURCE_DIR}/tests/real_core/graph_coexistence_oracle_test.c")
target_include_directories(graph_coexistence_oracle_test PRIVATE
    ${_imas_core_include_dirs}
    ${HDF5_C_INCLUDE_DIRS})
target_compile_definitions(graph_coexistence_oracle_test PRIVATE
    "REAL_CORE_LIBRARY_PATH=\"$<TARGET_FILE:${IMAS_CORE_AL_TARGET}>\""
    "EQUILIBRIUM_FIXTURE_DIR=\"${CMAKE_CURRENT_SOURCE_DIR}/imas-python-fixtures/fixtures\"")
target_link_libraries(graph_coexistence_oracle_test PRIVATE
    imas_mvdd_loader_graph_test
    ${CMAKE_DL_LIBS}
    ${HDF5_C_LIBRARIES})
add_dependencies(graph_coexistence_oracle_test imas_mvdd_graph_capi)
if(IMAS_CORE_BUILT_FROM_SOURCE)
    add_dependencies(graph_coexistence_oracle_test ${IMAS_CORE_AL_TARGET})
endif()
set_target_properties(graph_coexistence_oracle_test PROPERTIES
    BUILD_RPATH "${IMAS_MVDD_GRAPH_STAGE_DIR}/lib")

if(IMAS_MVDD_GRAPH_TEST_SOURCE STREQUAL "controlled")
add_real_core_test(read-coexistence-forward-selects-primary-then-falls-back
    $<TARGET_FILE:graph_coexistence_oracle_test>
    read-coexistence-forward-selects-primary-then-falls-back)
add_real_core_test(read-coexistence-forward-arraystruct-falls-back-between-j-candidates
    $<TARGET_FILE:graph_coexistence_oracle_test>
    read-coexistence-forward-arraystruct-falls-back-between-j-candidates)
add_real_core_test(read-coexistence-reverse-selects-the-4.1-successor
    $<TARGET_FILE:graph_coexistence_oracle_test>
    read-coexistence-reverse-selects-the-4.1-successor)
add_real_core_test(write-delete-coexistence-forward-is-primary-only-and-fans-out
    $<TARGET_FILE:graph_coexistence_oracle_test>
    write-delete-coexistence-forward-is-primary-only-and-fans-out)
add_real_core_test(write-coexistence-reverse-non-primary-refuses
    $<TARGET_FILE:graph_coexistence_oracle_test>
    write-coexistence-reverse-non-primary-refuses)
set_tests_properties(
    read-coexistence-forward-selects-primary-then-falls-back
    read-coexistence-reverse-selects-the-4.1-successor
    write-delete-coexistence-forward-is-primary-only-and-fans-out
    write-coexistence-reverse-non-primary-refuses
    PROPERTIES
    ENVIRONMENT "IMAS_MVDD_GRAPH_TEST_SCOPE=coexistence")
set_tests_properties(read-coexistence-forward-arraystruct-falls-back-between-j-candidates
    PROPERTIES
    ENVIRONMENT "IMAS_MVDD_GRAPH_TEST_SCOPE=coexistence-arraystruct")

else()
    foreach(direction IN ITEMS forward reverse)
        add_real_core_test(live-graph-core-coexistence-${direction}
            $<TARGET_FILE:graph_coexistence_oracle_test> live-j-${direction})
    endforeach()
    add_real_core_test(live-graph-core-coexistence-nested
        $<TARGET_FILE:graph_coexistence_oracle_test>
        read-coexistence-forward-arraystruct-falls-back-between-j-candidates)
endif()

# --- Issue #133: on-disk oracle proof for the write and delete seams. Each
# scenario mutates its own private copy of the fixture pair and reads the
# result back with raw HDF5, never through the shim. No RESOURCE_LOCK is
# needed: each scenario opens only its own unique temp-directory copy, the
# same reasoning as the copied-fixture-harness scenario above.
#
# Issue #136 (ADR 0020) is what made these reachable: before it, a WRITE_OP
# open never registered a conversion record against real Core's HDF5 backend,
# so only claim 5 could be proven and the rest were pinned as a known gap.
# Four of #133's five claims are proven here. Issue #138 additionally proves
# the delete half reaches IMAS-Core rather than becoming a successful no-op.
add_executable(write_delete_oracle_test
    "${CMAKE_CURRENT_SOURCE_DIR}/tests/real_core/write_delete_oracle_test.c")
target_include_directories(write_delete_oracle_test PRIVATE
    ${_imas_core_include_dirs}
    ${HDF5_C_INCLUDE_DIRS})
target_compile_definitions(write_delete_oracle_test PRIVATE
    "EQUILIBRIUM_FIXTURE_DIR=\"${CMAKE_CURRENT_SOURCE_DIR}/imas-python-fixtures/fixtures\"")
target_link_libraries(write_delete_oracle_test PRIVATE
    imas_mvdd_loader
    ${HDF5_C_LIBRARIES})
add_dependencies(write_delete_oracle_test imas_mvdd_capi)
set_target_properties(write_delete_oracle_test PROPERTIES
    BUILD_RPATH "${IMAS_MVDD_STAGE_DIR}/lib")

# The prefix names the seam each scenario drives -- `write-oracle-*` for
# al_write_data, `delete-oracle-*` for al_delete_data -- so `ctest -R
# "^delete-oracle-"` selects the delete half, which is one scenario because
# this backend's deleteData ignores its path (issue #139). The ctest name is
# the scenario argument verbatim; every scenario is registered identically, so
# it is registered once here rather than twelve times.
#
foreach(scenario IN ITEMS
        write-oracle-forward-refusal-leaves-the-stamp-untouched
        write-oracle-reverse-refusal-leaves-the-stamp-untouched
        write-oracle-forward-lands-on-the-stored-spelling
        write-oracle-reverse-lands-on-the-stored-spelling
        write-oracle-forward-flips-the-sign-on-disk
        write-oracle-reverse-flips-the-sign-on-disk
        write-oracle-reverse-leaves-the-precedence-two-candidate-alone
        write-oracle-forward-leaves-the-precedence-two-candidate-alone
        write-oracle-forward-non-primary-source-refuses
        delete-oracle-reverse-fan-out-reaches-disk
        write-oracle-forward-no-stored-slot-refuses
        write-oracle-fresh-occurrence-is-untranslated)
    if(NOT "${scenario}" MATCHES "^(write-oracle-|delete-oracle-|write-delete-oracle-)")
        message(FATAL_ERROR
            "oracle scenario '${scenario}' must lead with write-oracle-, "
            "delete-oracle- or write-delete-oracle- so its ctest name names "
            "the seam it drives")
    endif()
    add_real_core_test("${scenario}" $<TARGET_FILE:write_delete_oracle_test> "${scenario}")
endforeach()

# Issue #229: the second IDS uses the graph-selected test source but the same
# public C ABI and raw-HDF5 stored-effect oracle as the equilibrium scenarios.
# Each case owns a fresh pulse because the HLI DD-version latch is process-wide
# and writes/deletes are intentionally observable on disk.
add_executable(graph_runtime_map_oracle_test
    "${CMAKE_CURRENT_SOURCE_DIR}/tests/real_core/graph_runtime_map_oracle_test.c")
target_include_directories(graph_runtime_map_oracle_test PRIVATE
    ${_imas_core_include_dirs}
    ${HDF5_C_INCLUDE_DIRS})
target_link_libraries(graph_runtime_map_oracle_test PRIVATE
    imas_mvdd_loader_graph_test
    ${HDF5_C_LIBRARIES})
add_dependencies(graph_runtime_map_oracle_test imas_mvdd_graph_capi)
set_target_properties(graph_runtime_map_oracle_test PROPERTIES
    BUILD_RPATH "${IMAS_MVDD_GRAPH_STAGE_DIR}/lib")

if(IMAS_MVDD_GRAPH_TEST_SOURCE STREQUAL "live")
    target_compile_definitions(graph_runtime_map_oracle_test PRIVATE IMAS_MVDD_LIVE_GRAPH=1)
    foreach(family IN ITEMS equilibrium_read write_delete_oracle)
        add_executable(live_${family}_test "${CMAKE_CURRENT_SOURCE_DIR}/tests/real_core/${family}_test.c")
        target_include_directories(live_${family}_test PRIVATE ${_imas_core_include_dirs} ${HDF5_C_INCLUDE_DIRS})
        target_compile_definitions(live_${family}_test PRIVATE
            "EQUILIBRIUM_FIXTURE_DIR=\"${CMAKE_CURRENT_SOURCE_DIR}/imas-python-fixtures/fixtures\"")
        target_compile_definitions(live_${family}_test PRIVATE IMAS_MVDD_LIVE_GRAPH=1)
        target_link_libraries(live_${family}_test PRIVATE imas_mvdd_loader_graph_test ${HDF5_C_LIBRARIES})
        add_dependencies(live_${family}_test imas_mvdd_graph_capi)
        set_target_properties(live_${family}_test PROPERTIES BUILD_RPATH "${IMAS_MVDD_GRAPH_STAGE_DIR}/lib")
    endforeach()
    foreach(direction IN ITEMS forward reverse)
        add_real_core_test(live-graph-core-${direction}-rename-read
            $<TARGET_FILE:live_equilibrium_read_test> ${direction}-reads-renamed-value-through-own-spelling
            RESOURCE_LOCK equilibrium-fixture-dd-${direction})
        foreach(scenario IN ITEMS lands-on-the-stored-spelling flips-the-sign-on-disk refusal-leaves-the-stamp-untouched)
            add_real_core_test(live-graph-core-${direction}-${scenario}
                $<TARGET_FILE:live_write_delete_oracle_test> write-oracle-${direction}-${scenario})
        endforeach()
    endforeach()
endif()

add_real_core_test(read-graph-pulse-schedule-forward
    $<TARGET_FILE:graph_runtime_map_oracle_test> graph-pulse-schedule-forward-read)
add_real_core_test(read-graph-pulse-schedule-reverse
    $<TARGET_FILE:graph_runtime_map_oracle_test> graph-pulse-schedule-reverse-read)
add_real_core_test(write-graph-pulse-schedule-forward
    $<TARGET_FILE:graph_runtime_map_oracle_test> graph-pulse-schedule-forward-write)
add_real_core_test(write-graph-pulse-schedule-reverse
    $<TARGET_FILE:graph_runtime_map_oracle_test> graph-pulse-schedule-reverse-write)
add_real_core_test(delete-graph-pulse-schedule-forward
    $<TARGET_FILE:graph_runtime_map_oracle_test> graph-pulse-schedule-forward-delete)
add_real_core_test(delete-graph-pulse-schedule-reverse
    $<TARGET_FILE:graph_runtime_map_oracle_test> graph-pulse-schedule-reverse-delete)
add_real_core_test(delete-graph-pulse-schedule-forward-structure
    $<TARGET_FILE:graph_runtime_map_oracle_test> graph-pulse-schedule-forward-structure-delete)
add_real_core_test(delete-graph-pulse-schedule-reverse-structure
    $<TARGET_FILE:graph_runtime_map_oracle_test> graph-pulse-schedule-reverse-structure-delete)

imas_mvdd_end_real_core_tests()
