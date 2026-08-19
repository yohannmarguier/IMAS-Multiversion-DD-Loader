    cmake_minimum_required(VERSION 3.21)

project(imas-mvdd-loader
    VERSION 0.1.0
    DESCRIPTION "Shim between the IMAS HLIs and IMAS-Core"
    # The shipped ABI remains C, but tests compile C++ for a loadable plugin
    # fixture. find_package(al-core CONFIG)'s find_dependency(Boost) also
    # requires a configured C++ compiler.
    LANGUAGES C CXX)

include(GNUInstallDirs)

# CMake does not build the Rust code itself: cargo-c does, and CMake drives it.
# That keeps one build system responsible for the C ABI artefacts (library,
# generated header, pkg-config file) and lets consumers depend on this project
# the way they depend on IMAS-Core.

option(IMAS_MVDD_BUILD_TESTS "Build and register the test suite" ON)
option(IMAS_MVDD_CARGO_OFFLINE "Pass --offline to cargo (build nodes without network)" OFF)
option(IMAS_MVDD_REAL_CORE_TESTS
    "Acquire IMAS-Core and build the drift and real-Core seam tests" ON)

set(IMAS_MVDD_PACKAGE imas-mvdd-loader)   # cargo package name
set(IMAS_MVDD_LIB_NAME imas_mvdd_loader)  # resulting library / header name

# One committed pin feeds the Rust compatibility gate and both sides of the
# recording-stub test. Keeping the value out of those sources prevents a test
# fixture from drifting away from the release the shim actually supports.
file(STRINGS "${CMAKE_CURRENT_SOURCE_DIR}/IMAS_CORE_VERSION"
    IMAS_CORE_VERSION LIMIT_COUNT 1)
if(NOT IMAS_CORE_VERSION MATCHES "^[0-9]+\\.[0-9]+\\.[0-9]+$")
    message(FATAL_ERROR
        "IMAS_CORE_VERSION must contain a numeric major.minor.patch release")
endif()
string(REGEX MATCH "^([0-9]+)\\.([0-9]+)\\." _imas_core_version_match
    "${IMAS_CORE_VERSION}")
set(IMAS_CORE_SUPPORTED_MAJOR "${CMAKE_MATCH_1}")
set(IMAS_CORE_SUPPORTED_MINOR "${CMAKE_MATCH_2}")
math(EXPR IMAS_CORE_INCOMPATIBLE_MAJOR "${IMAS_CORE_SUPPORTED_MAJOR} + 1")
math(EXPR IMAS_CORE_DRIFT_MINOR "${IMAS_CORE_SUPPORTED_MINOR} + 1")
set(IMAS_CORE_INCOMPATIBLE_VERSION "${IMAS_CORE_INCOMPATIBLE_MAJOR}.0.0")
set(IMAS_CORE_DRIFT_VERSION
    "${IMAS_CORE_SUPPORTED_MAJOR}.${IMAS_CORE_DRIFT_MINOR}.0")

# ---------------------------------------------------------------------------
# IMAS-Core acquisition
#
# The shim never links against IMAS-Core; it resolves it at runtime (see
# docs/adr/0001-runtime-binding-not-linking.md). The default profile still
# acquires real IMAS-Core so a missing prerequisite fails at configure time.
# CI's explicit IMAS_MVDD_REAL_CORE_TESTS=OFF profile is the one exception:
# it registers only the fast recording-stub seams, never a silently reduced
# version of the real-Core suite.
#
# Three modes, matching the option vocabulary IMAS-Fortran's AL core
# acquisition module already established (its top-level CMakeLists.txt),
# so a reviewer from that team can read this with knowledge they already
# have:
#   - installed (default): find_package(al-core CONFIG) against a
#     pre-installed IMAS-Core. Deliberately its CMake package — introduced
#     in IMAS-Core 5.7.1 — rather than its pkg-config file: the .pc file's
#     Libs line carries a GNU-ld-only flag that breaks on macOS.
#   - development layout (-D IMAS_CORE_DEVELOPMENT_LAYOUT=ON): build a
#     sibling checkout at ../IMAS-Core.
#   - download and build (-D IMAS_CORE_DOWNLOAD_DEPENDENCIES=ON): fetch and
#     build IMAS_CORE_GIT_TAG.
#
# Both build modes add IMAS-Core with EXCLUDE_FROM_ALL: its own install()
# rules must never run as part of installing this shim, or installing the
# shim would deposit a second copy of IMAS-Core into the prefix.
# ---------------------------------------------------------------------------

option(IMAS_CORE_DOWNLOAD_DEPENDENCIES
    "Download and build IMAS-Core at IMAS_CORE_GIT_TAG instead of finding an installed one" OFF)
option(IMAS_CORE_DEVELOPMENT_LAYOUT
    "Build IMAS-Core from a sibling checkout at ../IMAS-Core instead of finding an installed one" OFF)
if(IMAS_CORE_DEVELOPMENT_LAYOUT AND IMAS_CORE_DOWNLOAD_DEPENDENCIES)
    message(STATUS
        "IMAS_CORE_DEVELOPMENT_LAYOUT=ON: setting IMAS_CORE_DOWNLOAD_DEPENDENCIES=OFF")
    set(IMAS_CORE_DOWNLOAD_DEPENDENCIES OFF CACHE BOOL
        "Download and build IMAS-Core at IMAS_CORE_GIT_TAG instead of finding an installed one"
        FORCE)
endif()

set(IMAS_CORE_GIT_REPOSITORY "https://github.com/iterorganization/IMAS-Core.git"
    CACHE STRING "Git repository of IMAS-Core, for IMAS_CORE_DOWNLOAD_DEPENDENCIES")
# Defaults to the pin above rather than a floating branch: nothing here
# links against IMAS-Core for a linker to validate signatures against, so a
# drifted upstream would corrupt memory silently at runtime instead of
# failing the build, and CI would stop being reproducible across days.
set(IMAS_CORE_GIT_TAG "${IMAS_CORE_VERSION}"
    CACHE STRING "Git tag of IMAS-Core, for IMAS_CORE_DOWNLOAD_DEPENDENCIES")

set(IMAS_CORE_MODULE_HINT "module load IMAS-Core/${IMAS_CORE_VERSION}")

if(NOT IMAS_MVDD_REAL_CORE_TESTS AND
        (IMAS_CORE_DOWNLOAD_DEPENDENCIES OR IMAS_CORE_DEVELOPMENT_LAYOUT))
    message(FATAL_ERROR
        "IMAS_MVDD_REAL_CORE_TESTS=OFF selects the recording-stub-only test "
        "profile and cannot be combined with an IMAS-Core acquisition mode")
endif()
if(NOT IMAS_MVDD_REAL_CORE_TESTS AND NOT IMAS_MVDD_BUILD_TESTS)
    message(FATAL_ERROR
        "IMAS_MVDD_REAL_CORE_TESTS=OFF requires IMAS_MVDD_BUILD_TESTS=ON; "
        "the recording-stub profile must not silently register no tests")
endif()

if(IMAS_MVDD_REAL_CORE_TESTS)
    if(IMAS_CORE_DOWNLOAD_DEPENDENCIES)
    message(STATUS
        "Acquiring IMAS-Core: downloading ${IMAS_CORE_GIT_REPOSITORY} @ ${IMAS_CORE_GIT_TAG}")
    include(FetchContent)
    FetchContent_Declare(al-core
        GIT_REPOSITORY "${IMAS_CORE_GIT_REPOSITORY}"
        GIT_TAG        "${IMAS_CORE_GIT_TAG}")
    FetchContent_GetProperties(al-core)
    if(NOT al-core_POPULATED)
        FetchContent_Populate(al-core)
        add_subdirectory("${al-core_SOURCE_DIR}" "${al-core_BINARY_DIR}" EXCLUDE_FROM_ALL)
    endif()
    set(IMAS_CORE_AL_TARGET al)
    set(IMAS_CORE_BUILT_FROM_SOURCE TRUE)
    elseif(IMAS_CORE_DEVELOPMENT_LAYOUT)
    set(IMAS_CORE_DEV_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../IMAS-Core")
    if(NOT EXISTS "${IMAS_CORE_DEV_DIR}/CMakeLists.txt")
        message(FATAL_ERROR
            "IMAS_CORE_DEVELOPMENT_LAYOUT=ON but '${IMAS_CORE_DEV_DIR}' has no "
            "CMakeLists.txt. This mode expects a sibling checkout of IMAS-Core "
            "next to this repository.")
    endif()
    message(STATUS "Acquiring IMAS-Core: development layout at ${IMAS_CORE_DEV_DIR}")
    add_subdirectory("${IMAS_CORE_DEV_DIR}" "${CMAKE_CURRENT_BINARY_DIR}/_deps/al-core-build"
        EXCLUDE_FROM_ALL)
    set(IMAS_CORE_AL_TARGET al)
    set(IMAS_CORE_BUILT_FROM_SOURCE TRUE)
    else()
    find_package(al-core CONFIG)
    if(NOT al-core_FOUND)
        message(FATAL_ERROR
            "IMAS-Core was not found (find_package(al-core CONFIG)). This project "
            "requires IMAS-Core to configure - pick one of:\n"
            "  1. installed (default): point CMAKE_PREFIX_PATH (or al-core_DIR) at "
            "an install prefix\n     containing lib/cmake/al-core/al-coreConfig.cmake "
            "(IMAS-Core >= 5.7.1).\n     On the ITER cluster: ${IMAS_CORE_MODULE_HINT}\n"
            "  2. development layout: -D IMAS_CORE_DEVELOPMENT_LAYOUT=ON, with a "
            "sibling checkout\n     at ../IMAS-Core\n"
            "  3. download and build: -D IMAS_CORE_DOWNLOAD_DEPENDENCIES=ON, fetches "
            "${IMAS_CORE_GIT_REPOSITORY}\n     at ${IMAS_CORE_GIT_TAG}\n")
    endif()
    message(STATUS "Acquiring IMAS-Core: found installed package (al-core_DIR=${al-core_DIR})")
    set(IMAS_CORE_AL_TARGET al-core::al)
    set(IMAS_CORE_BUILT_FROM_SOURCE FALSE)
    endif()
else()
    message(STATUS
        "IMAS-Core acquisition disabled: recording-stub-only test profile")
endif()

if(CMAKE_CONFIGURATION_TYPES)
    message(FATAL_ERROR
        "Multi-config generators are not supported: cargo has a single "
        "target directory per configuration. Use a single-config generator "
        "(Ninja, Unix Makefiles) and set CMAKE_BUILD_TYPE.")
endif()

if(NOT CMAKE_BUILD_TYPE)
    set(CMAKE_BUILD_TYPE RelWithDebInfo CACHE STRING "Build type" FORCE)
    set_property(CACHE CMAKE_BUILD_TYPE PROPERTY STRINGS
        Debug Release RelWithDebInfo MinSizeRel)
endif()

# ---------------------------------------------------------------------------
# Toolchain discovery
#
# On the ITER cluster both tools come from modules:
#   module load Rust/1.88.0-GCCcore-14.3.0
#   module load cargo-c/0.10.15-GCCcore-14.3.0
# ---------------------------------------------------------------------------

find_program(CARGO_EXECUTABLE cargo
    DOC "Path to the cargo executable")

if(NOT CARGO_EXECUTABLE)
    message(FATAL_ERROR
        "cargo not found. On the ITER cluster: "
        "module load Rust/1.88.0-GCCcore-14.3.0")
endif()

execute_process(
    COMMAND "${CARGO_EXECUTABLE}" --version
    OUTPUT_VARIABLE CARGO_VERSION_OUTPUT
    OUTPUT_STRIP_TRAILING_WHITESPACE
    ERROR_QUIET)

# cargo-c registers itself as a set of cargo subcommands; asking cargo for one
# of them is the only reliable way to detect it, since it may come from a module
# rather than from ~/.cargo/bin. (`cargo capi --version` prints help, not a
# version — `cbuild` is the one that answers properly.)
execute_process(
    COMMAND "${CARGO_EXECUTABLE}" cbuild --version
    RESULT_VARIABLE CARGO_C_RESULT
    OUTPUT_VARIABLE CARGO_C_VERSION_OUTPUT
    OUTPUT_STRIP_TRAILING_WHITESPACE
    ERROR_QUIET)

if(NOT CARGO_C_RESULT EQUAL 0)
    message(FATAL_ERROR
        "cargo-c not found (`cargo cbuild --version` failed). On the ITER "
        "cluster: module load cargo-c/0.10.15-GCCcore-14.3.0")
endif()

message(STATUS "Found cargo:   ${CARGO_EXECUTABLE} (${CARGO_VERSION_OUTPUT})")
message(STATUS "Found cargo-c: ${CARGO_C_VERSION_OUTPUT}")

# ---------------------------------------------------------------------------
# Cargo invocation
# ---------------------------------------------------------------------------

set(CARGO_TARGET_DIR "${CMAKE_CURRENT_BINARY_DIR}/cargo")
set(IMAS_MVDD_STAGE_DIR "${CMAKE_CURRENT_BINARY_DIR}/stage")

# Name the cargo profile explicitly: `cargo cinstall` defaults to release
# whatever CMAKE_BUILD_TYPE says, so leaving it implicit silently ignores a
# Debug configuration.
if(CMAKE_BUILD_TYPE STREQUAL "Debug")
    set(CARGO_PROFILE dev)
else()
    set(CARGO_PROFILE release)
endif()

set(CARGO_COMMON_ARGS
    --manifest-path "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml"
    --target-dir "${CARGO_TARGET_DIR}"
    --package "${IMAS_MVDD_PACKAGE}"
    --profile "${CARGO_PROFILE}")

if(IMAS_MVDD_CARGO_OFFLINE)
    list(APPEND CARGO_COMMON_ARGS --offline)
endif()

# Staging into the build tree gives the artefacts a predictable, install-shaped
# layout that the C smoke test and downstream in-tree consumers can point at.
set(CARGO_STAGE_ARGS
    --prefix "${IMAS_MVDD_STAGE_DIR}"
    --libdir "${IMAS_MVDD_STAGE_DIR}/lib"
    --includedir "${IMAS_MVDD_STAGE_DIR}/include")

# The shim opens IMAS-Core by bare soname. Give only the build-tree cdylib an
# RPATH to the acquired Core so tests and in-tree consumers need no override or
# loader-path environment setup. build.rs tracks this private environment
# variable and the install rule explicitly unsets it, forcing cargo to relink
# the installed cdylib without the build-machine path.
set(IMAS_MVDD_CARGO_STAGE_COMMAND
    "${CMAKE_COMMAND}" -E env --unset=MAKEFLAGS --unset=MFLAGS)
if(IMAS_MVDD_REAL_CORE_TESTS)
    list(APPEND IMAS_MVDD_CARGO_STAGE_COMMAND
        "IMAS_CORE_BUILD_RPATH=$<TARGET_FILE_DIR:${IMAS_CORE_AL_TARGET}>")
else()
    list(APPEND IMAS_MVDD_CARGO_STAGE_COMMAND
        --unset=IMAS_CORE_BUILD_RPATH)
endif()
list(APPEND IMAS_MVDD_CARGO_STAGE_COMMAND
    -- "${CARGO_EXECUTABLE}" cinstall ${CARGO_COMMON_ARGS} ${CARGO_STAGE_ARGS})

set(IMAS_MVDD_STAGED_INCLUDE_DIR "${IMAS_MVDD_STAGE_DIR}/include")
set(IMAS_MVDD_STAGED_HEADER "${IMAS_MVDD_STAGED_INCLUDE_DIR}/${IMAS_MVDD_LIB_NAME}.h")
set(IMAS_MVDD_STAGED_LIBRARY
    "${IMAS_MVDD_STAGE_DIR}/lib/${CMAKE_SHARED_LIBRARY_PREFIX}${IMAS_MVDD_LIB_NAME}${CMAKE_SHARED_LIBRARY_SUFFIX}")

# The imported target below refuses a non-existent include directory at
# generate time, so create it before the build has produced anything.
file(MAKE_DIRECTORY "${IMAS_MVDD_STAGED_INCLUDE_DIR}")

file(GLOB_RECURSE IMAS_MVDD_RUST_SOURCES CONFIGURE_DEPENDS
    "${CMAKE_CURRENT_SOURCE_DIR}/src/*.rs"
    "${CMAKE_CURRENT_SOURCE_DIR}/crates/*.rs")

add_custom_command(
    OUTPUT "${IMAS_MVDD_STAGED_LIBRARY}" "${IMAS_MVDD_STAGED_HEADER}"
    COMMAND ${IMAS_MVDD_CARGO_STAGE_COMMAND}
    DEPENDS
        ${IMAS_MVDD_RUST_SOURCES}
        "${CMAKE_CURRENT_SOURCE_DIR}/build.rs"
        "${CMAKE_CURRENT_SOURCE_DIR}/IMAS_CORE_VERSION"
        "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml"
        "${CMAKE_CURRENT_SOURCE_DIR}/cbindgen.toml"
    WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}"
    COMMENT "Building ${IMAS_MVDD_LIB_NAME} C ABI with cargo-c"
    VERBATIM)

add_custom_target(imas_mvdd_capi ALL
    DEPENDS "${IMAS_MVDD_STAGED_LIBRARY}" "${IMAS_MVDD_STAGED_HEADER}")

# Let in-tree consumers link this the way they would any CMake library.
add_library(imas_mvdd_loader SHARED IMPORTED GLOBAL)
add_library(imas-mvdd-loader::imas-mvdd-loader ALIAS imas_mvdd_loader)
set_target_properties(imas_mvdd_loader PROPERTIES
    IMPORTED_LOCATION "${IMAS_MVDD_STAGED_LIBRARY}"
    INTERFACE_INCLUDE_DIRECTORIES "${IMAS_MVDD_STAGED_INCLUDE_DIR}")

# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

if(IMAS_MVDD_BUILD_TESTS)
    enable_testing()

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

    add_executable(abi_smoke "${CMAKE_CURRENT_SOURCE_DIR}/tests/abi_smoke.c")
    target_link_libraries(abi_smoke PRIVATE imas_mvdd_loader)
    add_dependencies(abi_smoke imas_mvdd_capi)
    set_target_properties(abi_smoke PROPERTIES
        BUILD_RPATH "${IMAS_MVDD_STAGE_DIR}/lib")

    if(IMAS_MVDD_REAL_CORE_TESTS)
        add_test(NAME abi-smoke
            COMMAND "${CMAKE_COMMAND}" -E env
                --unset=IMAS_CORE_LIBRARY -- $<TARGET_FILE:abi_smoke>)
    else()
        add_stub_test(abi-smoke abi_smoke)
    endif()

    if(IMAS_MVDD_REAL_CORE_TESTS)
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
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/real_core/real_core_abi_mismatch/real_core_abi_core_check.c"
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/real_core/real_core_abi_mismatch/real_core_abi_shim_check.c")
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
            "-DTEST_SOURCE_DIR=${CMAKE_CURRENT_SOURCE_DIR}/tests/real_core/real_core_abi_mismatch"
            "-DTEST_BINARY_DIR=${CMAKE_CURRENT_BINARY_DIR}"
            "-DSHIM_INCLUDE_DIR=${IMAS_MVDD_STAGED_INCLUDE_DIR}"
            "-DCORE_INCLUDE_DIRS=${_imas_core_include_dirs}"
            -P "${CMAKE_CURRENT_SOURCE_DIR}/tests/cmake/verify_abi_mismatch.cmake")
    endif()

    # --- Runtime-binding test: proves the architecture (issue #3) ---------
    #
    # A recording stub stands in for IMAS-Core: it exports the complete
    # runtime-bound utility, data, and plugin surface and records what it
    # received, so assertions can be made directly on what crossed the
    # boundary instead of inferring it from a data round trip.
    #
    # The stub is deliberately never linked into runtime_binding_test: doing so would
    # give the linker two candidate definitions of al_context_info (the
    # shim's and the stub's) to choose between — exactly the ambiguity
    # runtime binding exists to avoid. runtime_binding_test only dlopen's the stub
    # itself, purely to read back its recorded state.
    add_executable(runtime_binding_test
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/runtime_binding_test.c")
    target_link_libraries(runtime_binding_test PRIVATE imas_mvdd_loader ${CMAKE_DL_LIBS})
    target_compile_definitions(runtime_binding_test PRIVATE
        "RECORDING_STUB_PATH=\"$<TARGET_FILE:recording_stub>\""
        "SHIM_LIBRARY_PATH=\"$<TARGET_FILE:imas_mvdd_loader>\""
        "SUPPORTED_CORE_VERSION=\"${IMAS_CORE_VERSION}\""
        "INCOMPATIBLE_CORE_VERSION=\"${IMAS_CORE_INCOMPATIBLE_VERSION}\"")
    add_dependencies(runtime_binding_test imas_mvdd_capi recording_stub)
    set_target_properties(runtime_binding_test PROPERTIES
        BUILD_RPATH "${IMAS_MVDD_STAGE_DIR}/lib")

    add_stub_test(runtime-binding-success runtime_binding_test success)

    add_test(NAME runtime-binding-version-drift-tolerated
        COMMAND runtime_binding_test version-drift)
    set_tests_properties(runtime-binding-version-drift-tolerated PROPERTIES
        ENVIRONMENT
            "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>;RECORDING_STUB_VERSION=${IMAS_CORE_DRIFT_VERSION}"
        PASS_REGULAR_EXPRESSION
            "tolerating IMAS-Core version drift.*built against ${IMAS_CORE_VERSION}, found ${IMAS_CORE_DRIFT_VERSION}")

    add_stub_test(runtime-binding-version-mismatch runtime_binding_test version-mismatch
        ENV "RECORDING_STUB_VERSION=${IMAS_CORE_INCOMPATIBLE_VERSION}")

    add_stub_test(runtime-binding-null-version runtime_binding_test null-version
        ENV "RECORDING_STUB_NULL_VERSION=1")

    add_test(NAME runtime-binding-missing-library COMMAND runtime_binding_test missing-library)
    set_tests_properties(runtime-binding-missing-library PROPERTIES ENVIRONMENT
        "IMAS_CORE_LIBRARY=${CMAKE_CURRENT_BINARY_DIR}/does-not-exist.so")

    add_stub_test(runtime-binding-verbatim-forwarding runtime_binding_test verbatim-forwarding)

    add_stub_test(runtime-binding-plugin-forwarding runtime_binding_test plugin-forwarding)

    add_test(NAME runtime-binding-plugin-timerange-omitted
        COMMAND runtime_binding_test plugin-timerange-omitted)

    add_stub_test(runtime-binding-utility-forwarding runtime_binding_test utility-forwarding)

    add_test(NAME runtime-binding-bare-soname COMMAND runtime_binding_test bare-soname)
    if(APPLE)
        set(_runtime_binding_search_path_var DYLD_LIBRARY_PATH)
    else()
        set(_runtime_binding_search_path_var LD_LIBRARY_PATH)
    endif()
    set_tests_properties(runtime-binding-bare-soname PROPERTIES ENVIRONMENT
        "${_runtime_binding_search_path_var}=$<TARGET_FILE_DIR:recording_stub>")

    # --- HLI DD version latch test: the setter and its environment fallback
    # (issue #45, ADR 0005) ---
    #
    # LATCH is a single process-wide OnceLock, so each scenario needs its own
    # fresh process exactly like runtime_binding_test's scenarios do; ctest
    # gives each add_test its own process already.
    find_package(Threads REQUIRED)
    add_executable(hli_dd_version_test
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/hli_dd_version_test.c")
    target_link_libraries(hli_dd_version_test PRIVATE
        imas_mvdd_loader ${CMAKE_DL_LIBS} Threads::Threads)
    target_compile_definitions(hli_dd_version_test PRIVATE
        "RECORDING_STUB_PATH=\"$<TARGET_FILE:recording_stub>\"")
    add_dependencies(hli_dd_version_test imas_mvdd_capi recording_stub)
    set_target_properties(hli_dd_version_test PROPERTIES
        BUILD_RPATH "${IMAS_MVDD_STAGE_DIR}/lib")

    add_test(NAME hli-dd-version-setter-accepts-valid-version
        COMMAND hli_dd_version_test setter-accepts-valid-version)
    add_test(NAME hli-dd-version-setter-accepts-identical-repeat
        COMMAND hli_dd_version_test setter-accepts-identical-repeat)
    add_test(NAME hli-dd-version-setter-rejects-conflicting-repeat
        COMMAND hli_dd_version_test setter-rejects-conflicting-repeat)
    add_test(NAME hli-dd-version-setter-rejects-invalid-version
        COMMAND hli_dd_version_test setter-rejects-invalid-version)
    add_test(NAME hli-dd-version-setter-rejects-null-version
        COMMAND hli_dd_version_test setter-rejects-null-version)
    add_test(NAME hli-dd-version-concurrent-identical-setters-all-succeed
        COMMAND hli_dd_version_test concurrent-identical-setters-all-succeed)

    add_stub_test(hli-dd-version-setter-precedes-environment
        hli_dd_version_test setter-precedes-environment
        HLI_DD_VERSION not-a-version)

    add_stub_test(hli-dd-version-valid-environment-latches-on-first-open
        hli_dd_version_test valid-environment-latches-on-first-open
        HLI_DD_VERSION 4.1.1)

    add_stub_test(hli-dd-version-invalid-environment-fails-first-open
        hli_dd_version_test invalid-environment-fails-first-open
        HLI_DD_VERSION not-a-version)

    add_test(NAME hli-dd-version-unset-first-open-then-setter-refused
        COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_MVDD_HLI_DD_VERSION --
            $<TARGET_FILE:hli_dd_version_test> unset-first-open-then-setter-refused)
    set_tests_properties(hli-dd-version-unset-first-open-then-setter-refused PROPERTIES
        ENVIRONMENT "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")

    # --- DD-version stamp discovery test: the al_begin_global_action seam
    # and al_begin_dataentry_action's registration (issue #53, ADR 0002,
    # ADR 0007, ADR 0009, ADR 0012) ---
    #
    # The HLI DD version latch and the context registry are both process-
    # wide, so each scenario needs its own fresh process, same as
    # hli_dd_version_test's scenarios.
    add_executable(version_discovery_test
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/version_discovery_test.c")
    target_link_libraries(version_discovery_test PRIVATE
        imas_mvdd_loader ${CMAKE_DL_LIBS})
    target_compile_definitions(version_discovery_test PRIVATE
        "RECORDING_STUB_PATH=\"$<TARGET_FILE:recording_stub>\"")
    add_dependencies(version_discovery_test imas_mvdd_capi recording_stub)
    set_target_properties(version_discovery_test PROPERTIES
        BUILD_RPATH "${IMAS_MVDD_STAGE_DIR}/lib")

    add_stub_test(version-discovery-dataentry-success-forwards-uri-and-mode version_discovery_test dataentry-success-forwards-uri-and-mode)

    add_stub_test(version-discovery-dataentry-failure-forwards-status-unchanged
        version_discovery_test dataentry-failure-forwards-status-unchanged
        ENV "RECORDING_STUB_DATAENTRY_FAIL=1")

    add_test(NAME version-discovery-hli-unset-global-action-is-plain-forward
        COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_MVDD_HLI_DD_VERSION --
            $<TARGET_FILE:version_discovery_test> hli-unset-global-action-is-plain-forward)
    set_tests_properties(version-discovery-hli-unset-global-action-is-plain-forward PROPERTIES
        ENVIRONMENT "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")

    add_stub_test(version-discovery-unstamped-occurrence-forwards-datapath-unchanged
        version_discovery_test unstamped-occurrence-forwards-datapath-unchanged
        HLI_DD_VERSION 4.1.1)

    add_stub_test(version-discovery-matching-version-forwards-datapath-unchanged
        version_discovery_test matching-version-forwards-datapath-unchanged
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 4.1.1)

    add_stub_test(version-discovery-mismatch-translates-datapath-on-second-open
        version_discovery_test mismatch-translates-datapath-on-second-open
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_stub_test(version-discovery-unstamped-stamp-clears-an-earlier-mismatch
        version_discovery_test unstamped-stamp-clears-an-earlier-mismatch
        HLI_DD_VERSION 4.1.1)

    add_stub_test(version-discovery-failed-stamp-read-clears-an-earlier-mismatch
        version_discovery_test failed-stamp-read-clears-an-earlier-mismatch
        HLI_DD_VERSION 4.1.1)

    add_stub_test(version-discovery-malformed-stamp-refuses-and-ends-context
        version_discovery_test malformed-stamp-refuses-and-ends-context
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION not-a-version)

    add_stub_test(version-discovery-reentrant-read-beneath-stamp-discovery-forwards-unchanged
        version_discovery_test reentrant-read-beneath-stamp-discovery-forwards-unchanged
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    # --- al_begin_slice_action / al_begin_timerange_action apply the same
    # rule as global action (issue #55) ---

    add_test(NAME version-discovery-slice-action-hli-unset-is-plain-forward
        COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_MVDD_HLI_DD_VERSION --
            $<TARGET_FILE:version_discovery_test> slice-action-hli-unset-is-plain-forward)
    set_tests_properties(version-discovery-slice-action-hli-unset-is-plain-forward PROPERTIES
        ENVIRONMENT "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")

    add_stub_test(version-discovery-slice-action-unstamped-forwards-ids-name-unchanged
        version_discovery_test slice-action-unstamped-forwards-ids-name-unchanged
        HLI_DD_VERSION 4.1.1)

    add_stub_test(version-discovery-slice-action-matching-version-forwards-ids-name-unchanged
        version_discovery_test slice-action-matching-version-forwards-ids-name-unchanged
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 4.1.1)

    add_stub_test(version-discovery-slice-action-mismatch-registers-occurrence-for-global-action
        version_discovery_test slice-action-mismatch-registers-occurrence-for-global-action
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_stub_test(version-discovery-slice-action-malformed-stamp-refuses-and-ends-context
        version_discovery_test slice-action-malformed-stamp-refuses-and-ends-context
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION not-a-version)

    add_stub_test(version-discovery-slice-action-failure-forwards-status-unchanged
        version_discovery_test slice-action-failure-forwards-status-unchanged
        HLI_DD_VERSION 4.1.1
        ENV "RECORDING_STUB_SLICE_FAIL=1")

    add_test(NAME version-discovery-timerange-action-hli-unset-is-plain-forward
        COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_MVDD_HLI_DD_VERSION --
            $<TARGET_FILE:version_discovery_test> timerange-action-hli-unset-is-plain-forward)
    set_tests_properties(version-discovery-timerange-action-hli-unset-is-plain-forward PROPERTIES
        ENVIRONMENT "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")

    add_stub_test(version-discovery-timerange-action-unstamped-forwards-ids-name-unchanged
        version_discovery_test timerange-action-unstamped-forwards-ids-name-unchanged
        HLI_DD_VERSION 4.1.1)

    add_stub_test(version-discovery-timerange-action-matching-version-forwards-ids-name-unchanged
        version_discovery_test timerange-action-matching-version-forwards-ids-name-unchanged
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 4.1.1)

    add_stub_test(version-discovery-timerange-action-mismatch-registers-occurrence-for-global-action
        version_discovery_test timerange-action-mismatch-registers-occurrence-for-global-action
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_stub_test(version-discovery-timerange-action-malformed-stamp-refuses-and-ends-context
        version_discovery_test timerange-action-malformed-stamp-refuses-and-ends-context
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION not-a-version)

    add_stub_test(version-discovery-timerange-action-failure-forwards-status-unchanged
        version_discovery_test timerange-action-failure-forwards-status-unchanged
        HLI_DD_VERSION 4.1.1
        ENV "RECORDING_STUB_TIMERANGE_FAIL=1")

    # --- Issue #56: ordinary read-path outcomes through the public C ABI ---
    add_executable(read_path_test
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/read_path_test.c")
    target_link_libraries(read_path_test PRIVATE imas_mvdd_loader ${CMAKE_DL_LIBS})
    target_compile_definitions(read_path_test PRIVATE
        "RECORDING_STUB_PATH=\"$<TARGET_FILE:recording_stub>\"")
    add_dependencies(read_path_test imas_mvdd_capi recording_stub)
    set_target_properties(read_path_test PROPERTIES
        BUILD_RPATH "${IMAS_MVDD_STAGE_DIR}/lib")

    add_stub_test(read-path-translates-field-and-timebase-independently
        read_path_test translates-field-and-timebase-independently
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_stub_test(read-path-forward-direction-translates-and-reports-no-source
        read_path_test forward-direction-translates-and-reports-no-source
        HLI_DD_VERSION 3.39.0
        STAMP_VERSION 4.1.1)

    add_stub_test(read-path-identity-rule-returns-data read_path_test identity-rule-returns-data
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_stub_test(read-path-merged-read-falls-through-to-next-candidate
        read_path_test merged-read-falls-through-to-next-candidate
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0
        ENV "RECORDING_STUB_READ_NOT_FOUND_FIELD=time_slice/ggd/b_field_phi")
    add_stub_test(read-path-merged-read-stops-at-first-candidate-with-data
        read_path_test merged-read-stops-at-first-candidate-with-data
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)
    add_stub_test(read-path-merged-read-returns-not-found-when-all-candidates-are-absent
        read_path_test merged-read-returns-not-found-when-all-candidates-are-absent
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0
        ENV "RECORDING_STUB_READ_NOT_FOUND=1")
    add_stub_test(read-path-split-plan-reads-and-flips-its-first-stored-destination
        read_path_test split-plan-reads-and-flips-its-first-stored-destination
        HLI_DD_VERSION 3.39.0
        STAMP_VERSION 4.1.1
        ENV "RECORDING_STUB_READ_DOUBLE=1")
    add_stub_test(read-path-reverse-split-read-flips-its-single-stored-source
        read_path_test reverse-split-read-flips-its-single-stored-source
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0
        ENV "RECORDING_STUB_READ_DOUBLE=1")

    add_stub_test(read-path-no-source-returns-null-without-core-call
        read_path_test no-source-returns-null-without-core-call
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    # All conversion-refusal scenarios need the same known mismatched
    # occurrence. Keep that shared seam setup in one place.
    function(add_read_path_refusal_test name scenario)
        add_stub_test("${name}" read_path_test "${scenario}"
            HLI_DD_VERSION 4.1.1
            STAMP_VERSION 3.39.0)
    endfunction()

    add_read_path_refusal_test(read-path-rank-changing-retype-refuses-without-core-call
        rank-changing-retype-refuses-without-core-call)
    add_read_path_refusal_test(read-path-unit-redefinition-refuses-without-core-call
        unit-redefinition-refuses-without-core-call)
    add_read_path_refusal_test(read-path-unsupported-sign-flip-types-refuse-without-core-call
        unsupported-sign-flip-types-refuse-without-core-call)
    add_read_path_refusal_test(read-path-sign-flip-rank-exceeding-maxdim-refuses-without-core-call
        sign-flip-rank-exceeding-maxdim-refuses-without-core-call)

    add_stub_test(read-path-sign-flip-array-negates-values-and-preserves-empty-double
        read_path_test sign-flip-array-negates-values-and-preserves-empty-double
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0
        ENV "RECORDING_STUB_READ_DOUBLE_VALUES=1.5,-9e40,3.2,-4.0")

    # ADR 0014: a read re-entering the shim beneath an in-flight read is
    # forwarded untouched. The stub's reentrant-read knob reproduces on any
    # platform what real IMAS-Core only does on ELF.
    add_stub_test(read-path-reentrant-read-is-forwarded-unchanged
        read_path_test reentrant-read-is-forwarded-unchanged
        HLI_DD_VERSION 3.39.0
        STAMP_VERSION 4.1.1)

    add_stub_test(read-path-reentrant-read-does-not-reapply-a-sign-flip
        read_path_test reentrant-read-does-not-reapply-a-sign-flip
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0
        ENV "RECORDING_STUB_READ_DOUBLE_VALUES=1.5,-9e40,3.2,-4.0")

    add_stub_test(read-path-sign-flip-invalid-shape-refuses-without-modifying-buffer
        read_path_test sign-flip-invalid-shape-refuses-without-modifying-buffer
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0
        ENV "RECORDING_STUB_READ_DOUBLE=1"
        "RECORDING_STUB_READ_SIZE_CSV=2147483647,2147483647,2147483647")

    add_stub_test(read-path-sign-flip-shape-override-respects-read-rank
        read_path_test sign-flip-shape-override-respects-read-rank
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0
        ENV "RECORDING_STUB_READ_DOUBLE=1"
        "RECORDING_STUB_READ_SIZE_CSV=1,42")

    add_stub_test(read-path-sign-flip-not-found-skips-value-transformation
        read_path_test sign-flip-not-found-skips-value-transformation
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0
        ENV "RECORDING_STUB_READ_NOT_FOUND=1")

    add_stub_test(read-path-resolves-relative-field-and-absolute-timebase
        read_path_test resolves-relative-field-and-absolute-timebase
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_stub_test(read-path-matching-context-bypasses-conversion
        read_path_test matching-context-bypasses-conversion
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 4.1.1)

    add_stub_test(read-path-unknown-context-bypasses-conversion
        read_path_test unknown-context-bypasses-conversion
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_stub_test(read-path-unstamped-context-bypasses-conversion
        read_path_test unstamped-context-bypasses-conversion
        HLI_DD_VERSION 4.1.1)

    add_test(NAME read-path-conversion-disabled-bypasses-conversion
        COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_MVDD_HLI_DD_VERSION --
            $<TARGET_FILE:read_path_test> conversion-disabled-bypasses-conversion)
    set_tests_properties(read-path-conversion-disabled-bypasses-conversion PROPERTIES
        ENVIRONMENT "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")

    add_stub_test(read-path-core-failure-propagates-unchanged
        read_path_test core-failure-propagates-unchanged
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0
        ENV "RECORDING_STUB_READ_FAIL=1")

    # --- Issue #65: the root-context fidelity loss log and its query exports ---
    # Both scenarios below need the HLI reading in its own, 3.39.0 spelling
    # (Direction::Forward) to hit a rule this artifact declares lossy in that
    # direction: fold-ggd-bfield (merged) and move-gap (moved).
    add_stub_test(read-path-merged-read-retains-a-lossy-verdict-in-the-loss-log
        read_path_test merged-read-retains-a-lossy-verdict-in-the-loss-log
        HLI_DD_VERSION 3.39.0
        STAMP_VERSION 4.1.1)

    add_stub_test(read-path-moved-read-retains-a-lossy-verdict-in-the-loss-log
        read_path_test moved-read-retains-a-lossy-verdict-in-the-loss-log
        HLI_DD_VERSION 3.39.0
        STAMP_VERSION 4.1.1)

    add_stub_test(read-path-ending-context-destroys-its-loss-log
        read_path_test ending-context-destroys-its-loss-log
        HLI_DD_VERSION 3.39.0
        STAMP_VERSION 4.1.1)

    # The remaining query-export safety scenarios only need any live
    # non-exact loss entry to query against, so they reuse the standard
    # mismatched occurrence fixture like the refusal scenarios above.
    function(add_loss_query_test name scenario)
        add_stub_test("${name}" read_path_test "${scenario}"
            HLI_DD_VERSION 3.39.0
            STAMP_VERSION 4.1.1)
    endfunction()

    add_loss_query_test(read-path-loss-count-null-output-is-refused
        loss-count-null-output-is-refused)
    add_loss_query_test(read-path-loss-at-null-path-buffer-is-refused
        loss-at-null-path-buffer-is-refused)
    add_loss_query_test(read-path-loss-at-null-verdict-is-refused
        loss-at-null-verdict-is-refused)
    add_loss_query_test(read-path-loss-at-negative-index-is-refused
        loss-at-negative-index-is-refused)
    add_loss_query_test(read-path-loss-at-out-of-range-index-is-refused
        loss-at-out-of-range-index-is-refused)
    add_loss_query_test(read-path-loss-at-insufficient-buffer-is-refused
        loss-at-insufficient-buffer-is-refused)

    # --- Issue #64: al_write_data / al_delete_data refusal through the public C ABI ---
    add_executable(write_delete_conversion_test
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/write_delete_conversion_test.c")
    target_link_libraries(write_delete_conversion_test PRIVATE imas_mvdd_loader ${CMAKE_DL_LIBS})
    target_compile_definitions(write_delete_conversion_test PRIVATE
        "RECORDING_STUB_PATH=\"$<TARGET_FILE:recording_stub>\"")
    add_dependencies(write_delete_conversion_test imas_mvdd_capi recording_stub)
    set_target_properties(write_delete_conversion_test PROPERTIES
        BUILD_RPATH "${IMAS_MVDD_STAGE_DIR}/lib")

    # All scenarios below need the same known mismatched equilibrium
    # occurrence. Keep that shared seam setup in one place.
    function(add_write_delete_mismatched_test name scenario)
        add_stub_test("${name}" write_delete_conversion_test "${scenario}"
            HLI_DD_VERSION 4.1.1
            STAMP_VERSION 3.39.0)
    endfunction()

    add_write_delete_mismatched_test(write-delete-write-refuses-under-known-mismatched-root-before-core-call
        write-refuses-under-known-mismatched-root-before-core-call)
    add_write_delete_mismatched_test(write-delete-delete-refuses-under-known-mismatched-root-before-core-call
        delete-refuses-under-known-mismatched-root-before-core-call)
    add_write_delete_mismatched_test(write-delete-plugin-write-refuses-under-known-mismatched-root-before-core-call
        plugin-write-refuses-under-known-mismatched-root-before-core-call)
    add_write_delete_mismatched_test(write-delete-write-nested-child-context-refuses-through-mismatched-root
        write-nested-child-context-refuses-through-mismatched-root)
    add_write_delete_mismatched_test(write-delete-delete-nested-child-context-refuses-through-mismatched-root
        delete-nested-child-context-refuses-through-mismatched-root)

    add_stub_test(write-delete-write-unstamped-context-forwards-unchanged
        write_delete_conversion_test write-unstamped-context-forwards-unchanged
        HLI_DD_VERSION 4.1.1)

    add_stub_test(write-delete-write-matching-context-forwards-unchanged
        write_delete_conversion_test write-matching-context-forwards-unchanged
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 4.1.1)

    add_stub_test(write-delete-plugin-write-matching-context-forwards-unchanged
        write_delete_conversion_test plugin-write-matching-context-forwards-unchanged
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 4.1.1)

    add_stub_test(write-delete-delete-matching-context-forwards-unchanged
        write_delete_conversion_test delete-matching-context-forwards-unchanged
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 4.1.1)

    add_stub_test(write-delete-write-unknown-context-forwards-unchanged
        write_delete_conversion_test write-unknown-context-forwards-unchanged
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_test(NAME write-delete-write-conversion-disabled-forwards-unchanged
        COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_MVDD_HLI_DD_VERSION --
            $<TARGET_FILE:write_delete_conversion_test> write-conversion-disabled-forwards-unchanged)
    set_tests_properties(write-delete-write-conversion-disabled-forwards-unchanged PROPERTIES
        ENVIRONMENT "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")

    add_stub_test(write-delete-delete-unknown-context-forwards-unchanged
        write_delete_conversion_test delete-unknown-context-forwards-unchanged
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_stub_test(write-delete-delete-unstamped-context-forwards-unchanged
        write_delete_conversion_test delete-unstamped-context-forwards-unchanged
        HLI_DD_VERSION 4.1.1)

    add_test(NAME write-delete-delete-conversion-disabled-forwards-unchanged
        COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_MVDD_HLI_DD_VERSION --
            $<TARGET_FILE:write_delete_conversion_test> delete-conversion-disabled-forwards-unchanged)
    set_tests_properties(write-delete-delete-conversion-disabled-forwards-unchanged PROPERTIES
        ENVIRONMENT "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")

    # --- Issue #61: arraystruct path conversion through the public C ABI ---
    add_executable(arraystruct_path_test
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/arraystruct_path_test.c")
    target_link_libraries(arraystruct_path_test PRIVATE imas_mvdd_loader ${CMAKE_DL_LIBS})
    target_compile_definitions(arraystruct_path_test PRIVATE
        "RECORDING_STUB_PATH=\"$<TARGET_FILE:recording_stub>\"")
    add_dependencies(arraystruct_path_test imas_mvdd_capi recording_stub)
    set_target_properties(arraystruct_path_test PROPERTIES
        BUILD_RPATH "${IMAS_MVDD_STAGE_DIR}/lib")

    add_stub_test(arraystruct-path-translates-renamed-container-and-timebase
        arraystruct_path_test translates-renamed-container-and-timebase
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_stub_test(arraystruct-path-translates-absolute-path-and-relative-timebase
        arraystruct_path_test translates-absolute-path-and-relative-timebase
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_stub_test(arraystruct-path-failed-open-propagates-without-child-record
        arraystruct_path_test failed-open-propagates-without-child-record
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0
        ENV "RECORDING_STUB_ARRAYSTRUCT_FAIL=1")

    add_stub_test(arraystruct-path-no-source-refuses-before-core
        arraystruct_path_test no-source-refuses-before-core
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_stub_test(arraystruct-path-matching-parent-forwards-unchanged
        arraystruct_path_test plain-parent-forwards-unchanged
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 4.1.1)

    add_stub_test(arraystruct-path-unstamped-parent-forwards-unchanged
        arraystruct_path_test plain-parent-forwards-unchanged
        HLI_DD_VERSION 4.1.1)

    add_stub_test(arraystruct-path-unknown-parent-forwards-unchanged
        arraystruct_path_test unknown-parent-forwards-unchanged
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_test(NAME arraystruct-path-conversion-disabled-parent-forwards-unchanged
        COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_MVDD_HLI_DD_VERSION --
            $<TARGET_FILE:arraystruct_path_test> plain-parent-forwards-unchanged)
    set_tests_properties(arraystruct-path-conversion-disabled-parent-forwards-unchanged PROPERTIES
        ENVIRONMENT "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")

    # --- Issue #62: al_read_data through a live arraystruct context -------
    add_executable(nested_context_read_test
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/nested_context_read_test.c")
    target_link_libraries(nested_context_read_test PRIVATE imas_mvdd_loader ${CMAKE_DL_LIBS})
    target_compile_definitions(nested_context_read_test PRIVATE
        "RECORDING_STUB_PATH=\"$<TARGET_FILE:recording_stub>\"")
    add_dependencies(nested_context_read_test imas_mvdd_capi recording_stub)
    set_target_properties(nested_context_read_test PROPERTIES
        BUILD_RPATH "${IMAS_MVDD_STAGE_DIR}/lib")

    add_stub_test(nested-context-read-relative-field-and-timebase-resolve-through-renamed-child
        nested_context_read_test relative-field-and-timebase-resolve-through-renamed-child
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_stub_test(nested-context-read-absolute-field-outside-child-subtree-resolves-from-ids-root
        nested_context_read_test absolute-field-outside-child-subtree-resolves-from-ids-root
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_stub_test(nested-context-read-no-source-returns-null-through-nested-child
        nested_context_read_test no-source-returns-null-through-nested-child
        HLI_DD_VERSION 3.39.0
        STAMP_VERSION 4.1.1)

    add_stub_test(nested-context-read-refusal-stops-before-core-through-nested-child
        nested_context_read_test refusal-stops-before-core-through-nested-child
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_stub_test(nested-context-read-sign-flip-applies-through-nested-child
        nested_context_read_test sign-flip-applies-through-nested-child
        HLI_DD_VERSION 3.39.0
        STAMP_VERSION 4.1.1
        ENV "RECORDING_STUB_READ_DOUBLE=1")

    # --- Issue #66: nested non-exact reads attribute to their root's loss log ---
    add_stub_test(nested-context-read-moved-read-through-nested-child-retains-lossy-verdict-on-root
        nested_context_read_test moved-read-through-nested-child-retains-lossy-verdict-on-root
        HLI_DD_VERSION 3.39.0
        STAMP_VERSION 4.1.1)

    add_stub_test(nested-context-read-merged-read-through-nested-child-retains-potentially-lossy-verdict
        nested_context_read_test merged-read-through-nested-child-retains-potentially-lossy-verdict
        HLI_DD_VERSION 3.39.0
        STAMP_VERSION 4.1.1)

    add_stub_test(nested-context-read-ending-root-before-child-destroys-the-shared-loss-log
        nested_context_read_test ending-root-before-child-destroys-the-shared-loss-log
        HLI_DD_VERSION 3.39.0
        STAMP_VERSION 4.1.1)
    # --- Issue #63: context lifecycle (al_end_action,
    # al_iterate_over_arraystruct, al_close_pulse) against the recording
    # stub --------------------------------------------------------------
    add_executable(context_lifecycle_test
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/context_lifecycle_test.c")
    target_link_libraries(context_lifecycle_test PRIVATE imas_mvdd_loader ${CMAKE_DL_LIBS})
    target_compile_definitions(context_lifecycle_test PRIVATE
        "RECORDING_STUB_PATH=\"$<TARGET_FILE:recording_stub>\"")
    add_dependencies(context_lifecycle_test imas_mvdd_capi recording_stub)
    set_target_properties(context_lifecycle_test PROPERTIES
        BUILD_RPATH "${IMAS_MVDD_STAGE_DIR}/lib")

    add_stub_test(context-lifecycle-ending-child-removes-only-its-own-record
        context_lifecycle_test ending-child-removes-only-its-own-record
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_stub_test(context-lifecycle-ending-root-removes-only-its-own-record
        context_lifecycle_test ending-root-removes-only-its-own-record
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_stub_test(context-lifecycle-failed-end-action-leaves-the-record-intact
        context_lifecycle_test failed-end-action-leaves-the-record-intact
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_stub_test(context-lifecycle-recycled-id-cannot-observe-the-released-record
        context_lifecycle_test recycled-id-cannot-observe-the-released-record
        HLI_DD_VERSION 4.1.1)

    add_stub_test(context-lifecycle-iterate-over-arraystruct-forwards-unchanged-and-mutates-nothing
        context_lifecycle_test iterate-over-arraystruct-forwards-unchanged-and-mutates-nothing
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_stub_test(context-lifecycle-close-pulse-forwards-unchanged-and-never-mutates-the-registry
        context_lifecycle_test close-pulse-forwards-unchanged-and-never-mutates-the-registry
        HLI_DD_VERSION 4.1.1)

    add_stub_test(context-lifecycle-ending-dataentry-context-leaves-live-operation-and-child-records-intact
        context_lifecycle_test ending-dataentry-context-leaves-live-operation-and-child-records-intact
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    # --- Issue #67: give the al_plugin_* reentry twins the same context
    # creation, path translation, and lifecycle behavior as their ordinary
    # counterparts --------------------------------------------------------
    add_executable(plugin_reentry_policy_test
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/plugin_reentry_policy_test.c")
    target_link_libraries(plugin_reentry_policy_test PRIVATE imas_mvdd_loader ${CMAKE_DL_LIBS})
    target_compile_definitions(plugin_reentry_policy_test PRIVATE
        "RECORDING_STUB_PATH=\"$<TARGET_FILE:recording_stub>\"")
    add_dependencies(plugin_reentry_policy_test imas_mvdd_capi recording_stub)
    set_target_properties(plugin_reentry_policy_test PROPERTIES
        BUILD_RPATH "${IMAS_MVDD_STAGE_DIR}/lib")

    add_test(NAME plugin-reentry-policy-plugin-global-hli-unset-is-plain-forward
        COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_MVDD_HLI_DD_VERSION --
            $<TARGET_FILE:plugin_reentry_policy_test> plugin-global-hli-unset-is-plain-forward)
    set_tests_properties(plugin-reentry-policy-plugin-global-hli-unset-is-plain-forward PROPERTIES
        ENVIRONMENT "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")

    add_stub_test(plugin-reentry-policy-plugin-global-unstamped-forwards-datapath-unchanged
        plugin_reentry_policy_test plugin-global-unstamped-forwards-datapath-unchanged
        HLI_DD_VERSION 4.1.1)

    add_stub_test(plugin-reentry-policy-plugin-global-matching-version-forwards-datapath-unchanged
        plugin_reentry_policy_test plugin-global-matching-version-forwards-datapath-unchanged
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 4.1.1)

    add_stub_test(plugin-reentry-policy-plugin-global-mismatch-translates-datapath-on-second-open
        plugin_reentry_policy_test plugin-global-mismatch-translates-datapath-on-second-open
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_stub_test(plugin-reentry-policy-plugin-global-malformed-stamp-refuses-and-ends-context
        plugin_reentry_policy_test plugin-global-malformed-stamp-refuses-and-ends-context
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION not-a-version)

    add_stub_test(plugin-reentry-policy-plugin-global-failure-forwards-status-unchanged
        plugin_reentry_policy_test plugin-global-failure-forwards-status-unchanged
        HLI_DD_VERSION 4.1.1
        ENV "RECORDING_STUB_PLUGIN_GLOBAL_FAIL=1")

    add_test(NAME plugin-reentry-policy-plugin-slice-hli-unset-is-plain-forward
        COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_MVDD_HLI_DD_VERSION --
            $<TARGET_FILE:plugin_reentry_policy_test> plugin-slice-hli-unset-is-plain-forward)
    set_tests_properties(plugin-reentry-policy-plugin-slice-hli-unset-is-plain-forward PROPERTIES
        ENVIRONMENT "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")

    add_stub_test(plugin-reentry-policy-plugin-slice-mismatch-registers-occurrence-for-plugin-global-action
        plugin_reentry_policy_test plugin-slice-mismatch-registers-occurrence-for-plugin-global-action
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_stub_test(plugin-reentry-policy-plugin-slice-malformed-stamp-refuses-and-ends-context
        plugin_reentry_policy_test plugin-slice-malformed-stamp-refuses-and-ends-context
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION not-a-version)

    add_stub_test(plugin-reentry-policy-plugin-slice-failure-forwards-status-unchanged
        plugin_reentry_policy_test plugin-slice-failure-forwards-status-unchanged
        HLI_DD_VERSION 4.1.1
        ENV "RECORDING_STUB_PLUGIN_SLICE_FAIL=1")

    add_stub_test(plugin-reentry-policy-plugin-arraystruct-translates-under-mismatch
        plugin_reentry_policy_test plugin-arraystruct-translates-under-mismatch
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_stub_test(plugin-reentry-policy-plugin-arraystruct-failed-open-propagates-without-child-record
        plugin_reentry_policy_test plugin-arraystruct-failed-open-propagates-without-child-record
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0
        ENV "RECORDING_STUB_PLUGIN_ARRAYSTRUCT_FAIL=1")

    add_stub_test(plugin-reentry-policy-plugin-arraystruct-no-source-refuses-before-core
        plugin_reentry_policy_test plugin-arraystruct-no-source-refuses-before-core
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_stub_test(plugin-reentry-policy-plugin-arraystruct-unknown-parent-forwards-unchanged
        plugin_reentry_policy_test plugin-arraystruct-unknown-parent-forwards-unchanged
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_stub_test(plugin-reentry-policy-plugin-end-action-removes-only-its-own-record
        plugin_reentry_policy_test plugin-end-action-removes-only-its-own-record
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_stub_test(plugin-reentry-policy-plugin-end-action-failed-leaves-the-record-intact
        plugin_reentry_policy_test plugin-end-action-failed-leaves-the-record-intact
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    # --- Issue #68: al_plugin_read_data follows al_read_data's policy exactly ---
    add_stub_test(plugin-reentry-policy-plugin-read-translates-field-under-mismatch
        plugin_reentry_policy_test plugin-read-translates-field-under-mismatch
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_stub_test(plugin-reentry-policy-plugin-read-refusal-before-core
        plugin_reentry_policy_test plugin-read-refusal-before-core
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_stub_test(plugin-reentry-policy-plugin-read-no-source-returns-null-without-core-call
        plugin_reentry_policy_test plugin-read-no-source-returns-null-without-core-call
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)

    add_stub_test(plugin-reentry-policy-plugin-read-merged-candidate-falls-through
        plugin_reentry_policy_test plugin-read-merged-candidate-falls-through
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0
        ENV "RECORDING_STUB_READ_NOT_FOUND_FIELD=time_slice/ggd/b_field_phi")

    add_stub_test(plugin-reentry-policy-plugin-read-sign-flip-negates-values
        plugin_reentry_policy_test plugin-read-sign-flip-negates-values
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0
        ENV "RECORDING_STUB_READ_DOUBLE=1")

    add_stub_test(plugin-reentry-policy-plugin-read-through-child-context-retains-loss-on-root
        plugin_reentry_policy_test plugin-read-through-child-context-retains-loss-on-root
        HLI_DD_VERSION 3.39.0
        STAMP_VERSION 4.1.1)

    # --- Issue #69: conversion stays inside its declared seam list ---------
    #
    # Every scenario runs with the same active conversion the seam tests above
    # use — HLI 4.1.1 against a 3.39.0 stamp — and asserts the *absence* of
    # translation outside that list. RECORDING_STUB_FILLED_PATHS_CSV makes
    # IMAS-Core report a path the loaded artifact has a rename rule for, so
    # "returned unchanged" is a claim about the shim rather than about a
    # placeholder string no rule could have touched either way.
    add_executable(scoped_passthrough_test
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/scoped_passthrough_test.c")
    target_link_libraries(scoped_passthrough_test PRIVATE imas_mvdd_loader ${CMAKE_DL_LIBS})
    target_compile_definitions(scoped_passthrough_test PRIVATE
        "RECORDING_STUB_PATH=\"$<TARGET_FILE:recording_stub>\""
        "EXPECTED_AL_VERSION=\"${IMAS_CORE_VERSION}\"")
    add_dependencies(scoped_passthrough_test imas_mvdd_capi recording_stub)
    set_target_properties(scoped_passthrough_test PROPERTIES
        BUILD_RPATH "${IMAS_MVDD_STAGE_DIR}/lib")

    function(add_scoped_passthrough_test name scenario)
        add_stub_test("${name}" scoped_passthrough_test "${scenario}"
            HLI_DD_VERSION 4.1.1
            STAMP_VERSION 3.39.0
            ENV "RECORDING_STUB_FILLED_PATHS_CSV=time_slice/global_quantities/beta_normal,time_slice/global_quantities/ip")
    endfunction()

    add_scoped_passthrough_test(scoped-passthrough-get-occurrences-forwards-ids-name-unchanged
        get-occurrences-forwards-ids-name-unchanged)
    add_scoped_passthrough_test(
        scoped-passthrough-list-filled-paths-forwards-name-and-returns-stored-paths-unchanged
        list-filled-paths-forwards-name-and-returns-stored-paths-unchanged)
    add_scoped_passthrough_test(
        scoped-passthrough-bind-and-unbind-plugin-forward-field-path-unchanged
        bind-and-unbind-plugin-forward-field-path-unchanged)
    add_scoped_passthrough_test(scoped-passthrough-remaining-non-seam-exports-forward-unchanged
        remaining-non-seam-exports-forward-unchanged)

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
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/real_core_test_plugin.cpp")
    target_include_directories(real_core_test_plugin PRIVATE ${_imas_core_include_dirs})
    target_compile_features(real_core_test_plugin PRIVATE cxx_std_17)
    set_target_properties(real_core_test_plugin PROPERTIES
        PREFIX ""
        OUTPUT_NAME "mvddtest_plugin"
        SUFFIX ".so"
        LIBRARY_OUTPUT_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/plugins")

    add_executable(real_core_forwarding_test
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/real_core_forwarding_test.c")
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
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/equilibrium_read_test.c")
    target_include_directories(equilibrium_read_test PRIVATE ${_imas_core_include_dirs})
    target_compile_definitions(equilibrium_read_test PRIVATE
        "EQUILIBRIUM_FIXTURE_DIR=\"${CMAKE_CURRENT_SOURCE_DIR}/imas-python-fixtures/fixtures\"")
    target_link_libraries(equilibrium_read_test PRIVATE imas_mvdd_loader)
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

    # Spec #43 tier-2 cells (c) and (g), previously covered only against the
    # recording stub: a write/delete/plugin-write refusal across a real
    # IMAS-Core boundary, and a real context lifecycle that must leave
    # conversion working through whatever is still open.
    add_test(NAME equilibrium-read-forward-write-and-delete-refuse-against-mismatch
        COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_CORE_LIBRARY --
            $<TARGET_FILE:equilibrium_read_test> forward-write-and-delete-refuse-against-mismatch)
    set_tests_properties(equilibrium-read-forward-write-and-delete-refuse-against-mismatch PROPERTIES
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

    endif()
endif()

# ---------------------------------------------------------------------------
# Install
#
# Re-run cargo-c against the real prefix rather than copying the staged tree:
# the generated .pc file embeds the prefix and must reflect where it lands.
# ---------------------------------------------------------------------------

# The prefix must be resolved when `cmake --install` runs, not now, so that
# `--install --prefix` and DESTDIR are both honoured. These two hold the
# literal text `${_prefix}/...` for the install script to expand against the
# absolute prefix it computes below.
foreach(dir LIBDIR INCLUDEDIR)
    if(IS_ABSOLUTE "${CMAKE_INSTALL_${dir}}")
        set(IMAS_MVDD_INSTALL_${dir} "${CMAKE_INSTALL_${dir}}")
    else()
        set(IMAS_MVDD_INSTALL_${dir} "\${_prefix}/${CMAKE_INSTALL_${dir}}")
    endif()
endforeach()

install(CODE "
# `cmake --install --prefix` accepts a relative path; CMake resolves it against
# the working directory of the install run. cargo-c has two rules of its own
# that a relative prefix collides with: it joins a *relative* --libdir onto
# --prefix, and it runs with WORKING_DIRECTORY set to the source tree below,
# not to wherever the user invoked `cmake --install`. Passing the prefix through
# unresolved therefore split one install in two -- CMake's own files landing in
# <prefix>/lib/cmake relative to the caller's directory, cargo-c's header and
# libraries in <prefix>/<prefix>/{include,lib} relative to the source tree. So
# resolve it here, in the install script, where get_filename_component and
# CMake's own relative-prefix rule share the same base directory.
set(_prefix \"\${CMAKE_INSTALL_PREFIX}\")
if(NOT IS_ABSOLUTE \"\${_prefix}\")
    get_filename_component(_prefix \"\${_prefix}\" ABSOLUTE)
endif()
set(_cargo_cmd [==[${CMAKE_COMMAND};-E;env;--unset=MAKEFLAGS;--unset=MFLAGS;--unset=IMAS_CORE_BUILD_RPATH;--;${CARGO_EXECUTABLE};cinstall;${CARGO_COMMON_ARGS}]==])
list(APPEND _cargo_cmd
    --prefix \"\${_prefix}\"
    --libdir \"${IMAS_MVDD_INSTALL_LIBDIR}\"
    --includedir \"${IMAS_MVDD_INSTALL_INCLUDEDIR}\")
if(NOT \"\$ENV{DESTDIR}\" STREQUAL \"\")
    list(APPEND _cargo_cmd --destdir \"\$ENV{DESTDIR}\")
endif()
message(STATUS \"Installing ${IMAS_MVDD_LIB_NAME} via cargo-c into \${_prefix}\")
execute_process(
    COMMAND \${_cargo_cmd}
    WORKING_DIRECTORY \"${CMAKE_CURRENT_SOURCE_DIR}\"
    COMMAND_ERROR_IS_FATAL ANY)
")

# ---------------------------------------------------------------------------
# CMake package config (find_package)
#
# cargo-c produces the pkg-config file but has no equivalent for a CMake
# package config, so it is authored directly (cmake/imas-mvdd-loaderConfig.cmake.in)
# and installed in the same lib/cmake/<name>/ layout IMAS-Core uses, next to
# the pkg-config file cargo-c already installs under lib/pkgconfig/.
# ---------------------------------------------------------------------------

include(CMakePackageConfigHelpers)

set(IMAS_MVDD_PACKAGE_CONFIG_DIR "${CMAKE_INSTALL_LIBDIR}/cmake/${IMAS_MVDD_PACKAGE}")

configure_package_config_file(
    "${CMAKE_CURRENT_SOURCE_DIR}/cmake/imas-mvdd-loaderConfig.cmake.in"
    "${CMAKE_CURRENT_BINARY_DIR}/${IMAS_MVDD_PACKAGE}Config.cmake"
    INSTALL_DESTINATION "${IMAS_MVDD_PACKAGE_CONFIG_DIR}"
    PATH_VARS CMAKE_INSTALL_INCLUDEDIR CMAKE_INSTALL_LIBDIR)

# SameMajorVersion keeps package requests within the ABI's supported major
# version. CMake also requires the installed package to be no older than the
# requested version, following its conventional directional package policy.
write_basic_package_version_file(
    "${CMAKE_CURRENT_BINARY_DIR}/${IMAS_MVDD_PACKAGE}ConfigVersion.cmake"
    VERSION "${PROJECT_VERSION}"
    COMPATIBILITY SameMajorVersion)

install(FILES
        "${CMAKE_CURRENT_BINARY_DIR}/${IMAS_MVDD_PACKAGE}Config.cmake"
        "${CMAKE_CURRENT_BINARY_DIR}/${IMAS_MVDD_PACKAGE}ConfigVersion.cmake"
    DESTINATION "${IMAS_MVDD_PACKAGE_CONFIG_DIR}")
