# Public ABI smoke and contract checks.

include_guard(GLOBAL)

add_executable(abi_smoke "${CMAKE_CURRENT_SOURCE_DIR}/tests/abi/abi_smoke.c")
target_link_libraries(abi_smoke PRIVATE imas_mvdd_loader)
add_dependencies(abi_smoke imas_mvdd_capi)
set_target_properties(abi_smoke PROPERTIES
    BUILD_RPATH "${IMAS_MVDD_STAGE_DIR}/lib")

if(IMAS_MVDD_REAL_CORE_TESTS)
    # This shared-name smoke test exists in both profiles, so it is deliberately
    # outside the real-Core bracket and must not carry the real-core label.
    add_test(NAME abi-smoke
        COMMAND "${CMAKE_COMMAND}" -E env
            --unset=IMAS_CORE_LIBRARY -- $<TARGET_FILE:abi_smoke>)
else()
    add_stub_test(abi-smoke abi_smoke)
endif()

if(IMAS_MVDD_REAL_CORE_TESTS)
imas_mvdd_begin_real_core_tests()
# Keep the runtime-bound surface honest mechanically: after filtering to
# IMAS-Core's public C ABI, the shim and Core must export the same names.
# This deliberately catches a missing new symbol or any leftover shim-only
# scaffolding without maintaining a second handwritten symbol list.
add_test(NAME real-core-export-list
    COMMAND "${CMAKE_COMMAND}"
        "-DCORE_LIBRARY=$<TARGET_FILE:${IMAS_CORE_AL_TARGET}>"
        "-DSHIM_LIBRARY=$<TARGET_FILE:imas_mvdd_loader>"
        "-DNM_EXECUTABLE=${CMAKE_NM}"
        -P "${CMAKE_CURRENT_SOURCE_DIR}/tests/cmake/check_exports.cmake")

# Runtime dlsym cannot type-check the hand-written signatures. Compile each
# public header in its own C translation unit against one shared contract;
# keeping the anonymous status structs separate lets C compare their
# layouts and every mirrored function type without declaration collisions.
add_executable(real_core_abi_check
    "${CMAKE_CURRENT_SOURCE_DIR}/tests/real_core/abi_contract/real_core_abi_core_check.c"
    "${CMAKE_CURRENT_SOURCE_DIR}/tests/real_core/abi_contract/real_core_abi_shim_check.c")
target_include_directories(real_core_abi_check PRIVATE
    "${IMAS_MVDD_STAGE_DIR}/include")
target_link_libraries(real_core_abi_check PRIVATE ${IMAS_CORE_AL_TARGET})
target_compile_features(real_core_abi_check PRIVATE c_std_11)
add_dependencies(real_core_abi_check imas_mvdd_capi)
add_test(NAME real-core-abi COMMAND real_core_abi_check)

# Compile the checker against a copy of the generated header with a
# deliberately wrong shared constant. This guards the guard: a successful
# nested build is a test failure, while the expected compiler rejection
# demonstrates that the ordinary ABI comparison catches drift loudly.
add_test(NAME real-core-abi-rejects-mismatch
    COMMAND "${CMAKE_COMMAND}"
        "-DTEST_SOURCE_DIR=${CMAKE_CURRENT_SOURCE_DIR}/tests/real_core/abi_contract"
        "-DTEST_BINARY_DIR=${CMAKE_CURRENT_BINARY_DIR}"
        "-DSHIM_INCLUDE_DIR=${IMAS_MVDD_STAGED_INCLUDE_DIR}"
        "-DCORE_INCLUDE_DIRS=${_imas_core_include_dirs}"
        -P "${CMAKE_CURRENT_SOURCE_DIR}/tests/cmake/verify_abi_mismatch.cmake")
imas_mvdd_end_real_core_tests()
endif()
