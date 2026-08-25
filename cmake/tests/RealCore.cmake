# Tests that exercise an acquired IMAS-Core implementation.

include_guard(GLOBAL)

if(IMAS_MVDD_REAL_CORE_TESTS)
    # --- Real IMAS-Core test: the tracer against the genuine article (issue #4) ---
#
# The same shim call as scenario_success, but against the IMAS-Core
# acquired above instead of the recording stub: al_context_info must
# reach a real implementation, not only a substitute for one. ctxID 0
# is the one value real IMAS-Core answers deterministically without any
# context ever having been opened (al_lowlevel.cpp's "NULL context"
# branch), which is what makes this runnable with no fixture beyond the
# acquired library itself. Explicitly remove the override so this test
# proves the shim's build-only RPATH finds Core by bare soname.
add_test(NAME runtime-binding-real-core
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_CORE_LIBRARY --
        $<TARGET_FILE:runtime_binding_test> real-core)
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

add_test(NAME runtime-binding-real-core-forwarding
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_CORE_LIBRARY --
        $<TARGET_FILE:real_core_forwarding_test>)
set_tests_properties(runtime-binding-real-core-forwarding PROPERTIES
    LABELS real-core)

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
add_test(NAME equilibrium-read-reverse-reads-renamed-value
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_CORE_LIBRARY --
        $<TARGET_FILE:equilibrium_read_test> reverse-reads-renamed-value-through-own-spelling)
set_tests_properties(equilibrium-read-reverse-reads-renamed-value PROPERTIES
    LABELS real-core
    RESOURCE_LOCK equilibrium-fixture-dd-4.1.1)

add_test(NAME equilibrium-read-forward-reads-renamed-value
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_CORE_LIBRARY --
        $<TARGET_FILE:equilibrium_read_test> forward-reads-renamed-value-through-own-spelling)
set_tests_properties(equilibrium-read-forward-reads-renamed-value PROPERTIES
    LABELS real-core
    RESOURCE_LOCK equilibrium-fixture-dd-3.39.0)

# Issue #129 keeps the safe leaf-delete proof at the spelling-observable
# recording-stub boundary. This real-Core probe retains stamp protection,
# while a real context lifecycle leaves conversion working through whatever is
# open.
add_test(NAME equilibrium-read-forward-delete-refuses-stamp-removal
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_CORE_LIBRARY --
        $<TARGET_FILE:equilibrium_read_test> forward-delete-refuses-stamp-removal)
set_tests_properties(equilibrium-read-forward-delete-refuses-stamp-removal PROPERTIES
    LABELS real-core RESOURCE_LOCK equilibrium-fixture-dd-3.39.0)

add_test(NAME equilibrium-read-forward-context-lifecycle-keeps-conversion-live
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_CORE_LIBRARY --
        $<TARGET_FILE:equilibrium_read_test> forward-context-lifecycle-keeps-conversion-live)
set_tests_properties(equilibrium-read-forward-context-lifecycle-keeps-conversion-live PROPERTIES
    LABELS real-core RESOURCE_LOCK equilibrium-fixture-dd-3.39.0)

add_test(NAME equilibrium-read-forward-merged-read-falls-through-to-stored-alias
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_CORE_LIBRARY --
        $<TARGET_FILE:equilibrium_read_test> forward-merged-read-falls-through-to-stored-alias)
set_tests_properties(equilibrium-read-forward-merged-read-falls-through-to-stored-alias PROPERTIES
    LABELS real-core RESOURCE_LOCK equilibrium-fixture-dd-3.39.0)

add_test(NAME equilibrium-read-reverse-merged-read-resolves-single-stored-destination
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_CORE_LIBRARY --
        $<TARGET_FILE:equilibrium_read_test>
        reverse-merged-read-resolves-single-stored-destination)
set_tests_properties(equilibrium-read-reverse-merged-read-resolves-single-stored-destination
    PROPERTIES LABELS real-core RESOURCE_LOCK equilibrium-fixture-dd-4.1.1)

# Issue #69: the refusal half of the matrix. Both scenarios open a real
# pulse whose contents are never reached — the assertion is that the shim
# stops before IMAS-Core, in whichever direction the artifact says it must.
add_test(NAME equilibrium-read-reverse-refuses-unservable-paths
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_CORE_LIBRARY --
        $<TARGET_FILE:equilibrium_read_test> reverse-refuses-unservable-paths)
set_tests_properties(equilibrium-read-reverse-refuses-unservable-paths PROPERTIES
    LABELS real-core RESOURCE_LOCK equilibrium-fixture-dd-4.1.1)

add_test(NAME equilibrium-read-forward-refuses-unservable-paths
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_CORE_LIBRARY --
        $<TARGET_FILE:equilibrium_read_test> forward-refuses-unservable-paths)
set_tests_properties(equilibrium-read-forward-refuses-unservable-paths PROPERTIES
    LABELS real-core RESOURCE_LOCK equilibrium-fixture-dd-3.39.0)

add_test(NAME equilibrium-read-reverse-split-read-uses-first-destination-and-flips-value
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_CORE_LIBRARY --
        $<TARGET_FILE:equilibrium_read_test> reverse-split-read-uses-first-destination-and-flips-value)
set_tests_properties(equilibrium-read-reverse-split-read-uses-first-destination-and-flips-value PROPERTIES
    LABELS real-core RESOURCE_LOCK equilibrium-fixture-dd-4.1.1)

add_test(NAME equilibrium-read-forward-split-read-uses-single-source-and-flips-value
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_CORE_LIBRARY --
        $<TARGET_FILE:equilibrium_read_test> forward-split-read-uses-single-source-and-flips-value)
set_tests_properties(equilibrium-read-forward-split-read-uses-single-source-and-flips-value PROPERTIES
    LABELS real-core RESOURCE_LOCK equilibrium-fixture-dd-3.39.0)

add_test(NAME equilibrium-read-reverse-reads-renamed-nested-container-field
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_CORE_LIBRARY --
        $<TARGET_FILE:equilibrium_read_test> reverse-reads-renamed-nested-container-field)
set_tests_properties(equilibrium-read-reverse-reads-renamed-nested-container-field PROPERTIES
    LABELS real-core RESOURCE_LOCK equilibrium-fixture-dd-4.1.1)

add_test(NAME equilibrium-read-forward-reads-renamed-nested-container-field
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_CORE_LIBRARY --
        $<TARGET_FILE:equilibrium_read_test> forward-reads-renamed-nested-container-field)
set_tests_properties(equilibrium-read-forward-reads-renamed-nested-container-field PROPERTIES
    LABELS real-core RESOURCE_LOCK equilibrium-fixture-dd-3.39.0)

add_test(NAME equilibrium-read-reverse-sign-flip-applies-through-nested-container
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_CORE_LIBRARY --
        $<TARGET_FILE:equilibrium_read_test> reverse-sign-flip-applies-through-nested-container)
set_tests_properties(equilibrium-read-reverse-sign-flip-applies-through-nested-container PROPERTIES
    LABELS real-core RESOURCE_LOCK equilibrium-fixture-dd-4.1.1)

add_test(NAME equilibrium-read-forward-sign-flip-applies-through-nested-container
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_CORE_LIBRARY --
        $<TARGET_FILE:equilibrium_read_test> forward-sign-flip-applies-through-nested-container)
set_tests_properties(equilibrium-read-forward-sign-flip-applies-through-nested-container PROPERTIES
    LABELS real-core RESOURCE_LOCK equilibrium-fixture-dd-3.39.0)

add_test(NAME equilibrium-read-same-version-is-unaffected
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_CORE_LIBRARY --
        $<TARGET_FILE:equilibrium_read_test> same-version-read-is-unaffected)
set_tests_properties(equilibrium-read-same-version-is-unaffected PROPERTIES
    LABELS real-core
    RESOURCE_LOCK equilibrium-fixture-dd-4.1.1)

add_test(NAME equilibrium-read-conversion-disabled-is-unaffected
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_CORE_LIBRARY --
        $<TARGET_FILE:equilibrium_read_test> conversion-disabled-read-is-unaffected)
set_tests_properties(equilibrium-read-conversion-disabled-is-unaffected PROPERTIES
    LABELS real-core
    RESOURCE_LOCK equilibrium-fixture-dd-4.1.1)

# This scenario opens only a unique copied fixture directory, so it cannot
# race the scenarios above that open a checked-in pulse and need HDF5 locks.
add_test(NAME equilibrium-read-copied-fixture-harness-reproves-renamed-read
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_CORE_LIBRARY --
        $<TARGET_FILE:equilibrium_read_test> copied-fixture-harness-reproves-renamed-read)
set_tests_properties(equilibrium-read-copied-fixture-harness-reproves-renamed-read PROPERTIES
    LABELS real-core)

endif()
