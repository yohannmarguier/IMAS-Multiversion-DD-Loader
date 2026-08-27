# Shared CTest infrastructure and configuration-script checks.

include_guard(GLOBAL)

add_test(NAME ci-workflow
    COMMAND "${CMAKE_COMMAND}"
        "-DWORKFLOW_FILE=${CMAKE_CURRENT_SOURCE_DIR}/.github/workflows/ci.yml"
        "-DTOOLCHAIN_ACTION_FILE=${CMAKE_CURRENT_SOURCE_DIR}/.github/actions/setup-toolchain/action.yml"
        -P "${CMAKE_CURRENT_SOURCE_DIR}/tests/cmake/check_ci_workflow.cmake")
add_test(NAME ci-workflow-guard-rejects-misplaced-commands
    COMMAND "${CMAKE_COMMAND}"
        "-DWORKFLOW_FILE=${CMAKE_CURRENT_SOURCE_DIR}/.github/workflows/ci.yml"
        "-DTOOLCHAIN_ACTION_FILE=${CMAKE_CURRENT_SOURCE_DIR}/.github/actions/setup-toolchain/action.yml"
        "-DCHECK_SCRIPT=${CMAKE_CURRENT_SOURCE_DIR}/tests/cmake/check_ci_workflow.cmake"
        "-DTEST_BINARY_DIR=${CMAKE_CURRENT_BINARY_DIR}"
        -P "${CMAKE_CURRENT_SOURCE_DIR}/tests/cmake/verify_ci_workflow_guard.cmake")

# Scripts run with `cmake -P` inherit no policies, so each one must pin its
# own version. CMake 4.x defaults those policies to NEW and CMake 3.x does
# not, which once let an unpinned `IN_LIST` pass locally and fail only on
# CI. Checking the pin here turns that skew into a local test failure.
add_test(NAME script-policy-versions
    COMMAND "${CMAKE_COMMAND}"
        "-DSCRIPT_DIR=${CMAKE_CURRENT_SOURCE_DIR}/tests"
        -P "${CMAKE_CURRENT_SOURCE_DIR}/tests/cmake/check_script_policies.cmake")
add_test(NAME script-policy-guard-rejects-unpinned-scripts
    COMMAND "${CMAKE_COMMAND}"
        "-DCHECK_SCRIPT=${CMAKE_CURRENT_SOURCE_DIR}/tests/cmake/check_script_policies.cmake"
        "-DTEST_BINARY_DIR=${CMAKE_CURRENT_BINARY_DIR}"
        -P "${CMAKE_CURRENT_SOURCE_DIR}/tests/cmake/verify_script_policy_guard.cmake")

# The recording stub is the fast profile's runtime dependency and remains
# part of the full profile so the two complementary seam suites stay honest.
add_library(recording_stub SHARED
    "${CMAKE_CURRENT_SOURCE_DIR}/tests/stub/recording_stub.c")
# Matches the shim's bare-soname fallback (src/core_binding.rs) so the
# bare-soname scenario below can exercise the loader's normal search
# path instead of the explicit override.
# Keep the bare-soname fixture out of the test working directory. On
# macOS, dlopen checks that directory before LC_RPATH; leaving libal there
# would make the real-Core tests silently bind the stub.
set_target_properties(recording_stub PROPERTIES
    OUTPUT_NAME al
    LIBRARY_OUTPUT_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/stub")
target_compile_definitions(recording_stub PRIVATE
    "RECORDING_STUB_DEFAULT_VERSION=\"${IMAS_CORE_VERSION}\"")

# Registers one scenario of a recording-stub suite as its own ctest test.
#
# Every such scenario needs IMAS_CORE_LIBRARY pointing at the stub, and
# nearly all of them also pin the HLI DD version to latch and the stamp
# version the stub reports back. One scenario per process is not a style
# choice: both the version latch (ADR 0005) and the context registry are
# process-wide, so varying either one means a fresh process.
#
# That trio was previously spelled out at 100-odd call sites — the same
# literal 35 times — with four near-identical wrapper functions over parts
# of it. Here it is expressed once, so a new scenario states only what is
# peculiar to it.
#
#   add_stub_test(<ctest-name> <executable> [<scenario-argument>...]
#                 [HLI_DD_VERSION <version>] [STAMP_VERSION <version>]
#                 [ENV <NAME=value>...])
#
# ENV entries are appended to the environment verbatim, in the order given,
# and are how a scenario reaches the recording stub's own fixture knobs.
function(add_stub_test name executable)
    cmake_parse_arguments(PARSE_ARGV 2 ARG "" "HLI_DD_VERSION;STAMP_VERSION" "ENV")

    set(environment "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")
    if(DEFINED ARG_HLI_DD_VERSION)
        list(APPEND environment "IMAS_MVDD_HLI_DD_VERSION=${ARG_HLI_DD_VERSION}")
    endif()
    if(DEFINED ARG_STAMP_VERSION)
        list(APPEND environment "RECORDING_STUB_STAMP_VERSION=${ARG_STAMP_VERSION}")
    endif()
    foreach(entry IN LISTS ARG_ENV)
        list(APPEND environment "${entry}")
    endforeach()

    add_test(NAME "${name}" COMMAND ${executable} ${ARG_UNPARSED_ARGUMENTS})
    set_tests_properties("${name}" PROPERTIES ENVIRONMENT "${environment}")
    if(NOT DEFINED ARG_HLI_DD_VERSION)
        set_property(TEST "${name}" APPEND PROPERTY ENVIRONMENT_MODIFICATION
            "IMAS_MVDD_HLI_DD_VERSION=unset:")
    endif()
endfunction()

# Registers one real-IMAS-Core scenario. The shim must resolve IMAS-Core through
# its build RPATH, rather than the recording-stub override used by stub suites.
function(add_real_core_test name executable)
    cmake_parse_arguments(PARSE_ARGV 2 ARG "" "RESOURCE_LOCK" "")

    add_test(NAME "${name}"
        COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_CORE_LIBRARY --
            ${executable} ${ARG_UNPARSED_ARGUMENTS})
    if(DEFINED ARG_RESOURCE_LOCK)
        set_tests_properties("${name}" PROPERTIES RESOURCE_LOCK "${ARG_RESOURCE_LOCK}")
    endif()
endfunction()

if(IMAS_MVDD_REAL_CORE_TESTS)
    get_target_property(_imas_core_include_dirs ${IMAS_CORE_AL_TARGET}
        INTERFACE_INCLUDE_DIRECTORIES)
    if(NOT _imas_core_include_dirs)
        message(FATAL_ERROR "${IMAS_CORE_AL_TARGET} does not publish IMAS-Core headers")
    endif()
endif()

add_test(NAME rust-unit
    COMMAND "${CARGO_EXECUTABLE}" test ${CARGO_COMMON_ARGS})

# The artifact's autoconvert-equivalence floor is an external contract:
# run the validation command itself, including its deliberately reduced
# fixture, rather than testing an internal helper (issue #51).
#
# The two supported-coverage floors are the only numbers this gate cannot
# derive from a checked-in file, so they are pinned here where a reviewer
# reads them next to the test that enforces them. They are floors, not
# equalities: coverage rising is the point of every new rule. Measured
# 342/428 forward and 335/370 reverse on the approved artifact -- raise a
# floor deliberately when new rules earn it, never to make a red gate green.
add_test(NAME equilibrium-artifact-coverage-floor
    COMMAND "${CMAKE_COMMAND}"
        "-DCARGO_EXECUTABLE=${CARGO_EXECUTABLE}"
        "-DCARGO_MANIFEST_PATH=${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml"
        "-DAPPROVED_ARTIFACT=${CMAKE_CURRENT_SOURCE_DIR}/docs/3.39.0--4.1.1.xml"
        "-DREDUCED_ARTIFACT=${CMAKE_CURRENT_SOURCE_DIR}/tests/fixtures/equilibrium-3.39.0--4.1.1-reduced.xml"
        "-DLEFT_INVENTORY=${CMAKE_CURRENT_SOURCE_DIR}/docs/inventory/equilibrium-3.39.0.txt"
        "-DRIGHT_INVENTORY=${CMAKE_CURRENT_SOURCE_DIR}/docs/inventory/equilibrium-4.1.1.txt"
        "-DBASELINE_TSV=${CMAKE_CURRENT_SOURCE_DIR}/docs/inventory/equilibrium-3.39.0--4.1.1-imas-python-renames.tsv"
        "-DNEAR_BOUNDARY_RULE_ID=rename-beta-normal"
        "-DCOMPLETENESS_RULE_ID=drop-b-flux-pol-norm"
        "-DFORWARD_SUPPORTED_FLOOR=342"
        "-DREVERSE_SUPPORTED_FLOOR=335"
        "-DWORK_DIR=${CMAKE_CURRENT_BINARY_DIR}"
        -P "${CMAKE_CURRENT_SOURCE_DIR}/tests/cmake/verify_artifact_coverage_floor.cmake")
