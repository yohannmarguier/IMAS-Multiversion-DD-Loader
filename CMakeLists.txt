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
            -P "${CMAKE_CURRENT_SOURCE_DIR}/tests/check_ci_workflow.cmake")
    add_test(NAME ci-workflow-guard-rejects-misplaced-commands
        COMMAND "${CMAKE_COMMAND}"
            "-DWORKFLOW_FILE=${CMAKE_CURRENT_SOURCE_DIR}/.github/workflows/ci.yml"
            "-DTOOLCHAIN_ACTION_FILE=${CMAKE_CURRENT_SOURCE_DIR}/.github/actions/setup-toolchain/action.yml"
            "-DCHECK_SCRIPT=${CMAKE_CURRENT_SOURCE_DIR}/tests/check_ci_workflow.cmake"
            "-DTEST_BINARY_DIR=${CMAKE_CURRENT_BINARY_DIR}"
            -P "${CMAKE_CURRENT_SOURCE_DIR}/tests/verify_ci_workflow_guard.cmake")

    # Scripts run with `cmake -P` inherit no policies, so each one must pin its
    # own version. CMake 4.x defaults those policies to NEW and CMake 3.x does
    # not, which once let an unpinned `IN_LIST` pass locally and fail only on
    # CI. Checking the pin here turns that skew into a local test failure.
    add_test(NAME script-policy-versions
        COMMAND "${CMAKE_COMMAND}"
            "-DSCRIPT_DIR=${CMAKE_CURRENT_SOURCE_DIR}/tests"
            -P "${CMAKE_CURRENT_SOURCE_DIR}/tests/check_script_policies.cmake")
    add_test(NAME script-policy-guard-rejects-unpinned-scripts
        COMMAND "${CMAKE_COMMAND}"
            "-DCHECK_SCRIPT=${CMAKE_CURRENT_SOURCE_DIR}/tests/check_script_policies.cmake"
            "-DTEST_BINARY_DIR=${CMAKE_CURRENT_BINARY_DIR}"
            -P "${CMAKE_CURRENT_SOURCE_DIR}/tests/verify_script_policy_guard.cmake")

    # The recording stub is the fast profile's runtime dependency and remains
    # part of the full profile so the two complementary seam suites stay honest.
    add_library(recording_stub SHARED
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/stub/recording_stub.c")
    # Matches the shim's bare-soname fallback (src/resolve.rs) so the
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

    if(IMAS_MVDD_REAL_CORE_TESTS)
        get_target_property(_imas_core_include_dirs ${IMAS_CORE_AL_TARGET}
            INTERFACE_INCLUDE_DIRECTORIES)
        if(NOT _imas_core_include_dirs)
            message(FATAL_ERROR "${IMAS_CORE_AL_TARGET} does not publish IMAS-Core headers")
        endif()
    endif()

    add_test(NAME rust-unit
        COMMAND "${CARGO_EXECUTABLE}" test ${CARGO_COMMON_ARGS})

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
        add_test(NAME abi-smoke COMMAND abi_smoke)
        set_tests_properties(abi-smoke PROPERTIES ENVIRONMENT
            "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")
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
            -P "${CMAKE_CURRENT_SOURCE_DIR}/tests/check_exports.cmake")

    # Runtime dlsym cannot type-check the hand-written signatures. Compile each
    # public header in its own C translation unit against one shared contract;
    # keeping the anonymous status structs separate lets C compare their
    # layouts and every mirrored function type without declaration collisions.
    add_executable(real_core_abi_check
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/real_core_abi_core_check.c"
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/real_core_abi_shim_check.c")
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
            "-DTEST_SOURCE_DIR=${CMAKE_CURRENT_SOURCE_DIR}/tests"
            "-DTEST_BINARY_DIR=${CMAKE_CURRENT_BINARY_DIR}"
            "-DSHIM_INCLUDE_DIR=${IMAS_MVDD_STAGED_INCLUDE_DIR}"
            "-DCORE_INCLUDE_DIRS=${_imas_core_include_dirs}"
            -P "${CMAKE_CURRENT_SOURCE_DIR}/tests/verify_abi_mismatch.cmake")
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

    add_test(NAME runtime-binding-success COMMAND runtime_binding_test success)
    set_tests_properties(runtime-binding-success PROPERTIES ENVIRONMENT
        "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")

    add_test(NAME runtime-binding-version-drift-tolerated
        COMMAND runtime_binding_test version-drift)
    set_tests_properties(runtime-binding-version-drift-tolerated PROPERTIES
        ENVIRONMENT
            "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>;RECORDING_STUB_VERSION=${IMAS_CORE_DRIFT_VERSION}"
        PASS_REGULAR_EXPRESSION
            "tolerating IMAS-Core version drift.*built against ${IMAS_CORE_VERSION}, found ${IMAS_CORE_DRIFT_VERSION}")

    add_test(NAME runtime-binding-version-mismatch COMMAND runtime_binding_test version-mismatch)
    set_tests_properties(runtime-binding-version-mismatch PROPERTIES ENVIRONMENT
        "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>;RECORDING_STUB_VERSION=${IMAS_CORE_INCOMPATIBLE_VERSION}")

    add_test(NAME runtime-binding-null-version COMMAND runtime_binding_test null-version)
    set_tests_properties(runtime-binding-null-version PROPERTIES ENVIRONMENT
        "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>;RECORDING_STUB_NULL_VERSION=1")

    add_test(NAME runtime-binding-missing-library COMMAND runtime_binding_test missing-library)
    set_tests_properties(runtime-binding-missing-library PROPERTIES ENVIRONMENT
        "IMAS_CORE_LIBRARY=${CMAKE_CURRENT_BINARY_DIR}/does-not-exist.so")

    add_test(NAME runtime-binding-verbatim-forwarding COMMAND runtime_binding_test verbatim-forwarding)
    set_tests_properties(runtime-binding-verbatim-forwarding PROPERTIES ENVIRONMENT
        "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")

    add_test(NAME runtime-binding-plugin-forwarding COMMAND runtime_binding_test plugin-forwarding)
    set_tests_properties(runtime-binding-plugin-forwarding PROPERTIES ENVIRONMENT
        "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")

    add_test(NAME runtime-binding-plugin-timerange-omitted
        COMMAND runtime_binding_test plugin-timerange-omitted)

    add_test(NAME runtime-binding-utility-forwarding
        COMMAND runtime_binding_test utility-forwarding)
    set_tests_properties(runtime-binding-utility-forwarding PROPERTIES ENVIRONMENT
        "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")

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

    add_test(NAME hli-dd-version-setter-precedes-environment
        COMMAND hli_dd_version_test setter-precedes-environment)
    set_tests_properties(hli-dd-version-setter-precedes-environment PROPERTIES ENVIRONMENT
        "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>;IMAS_MVDD_HLI_DD_VERSION=not-a-version")

    add_test(NAME hli-dd-version-valid-environment-latches-on-first-open
        COMMAND hli_dd_version_test valid-environment-latches-on-first-open)
    set_tests_properties(hli-dd-version-valid-environment-latches-on-first-open PROPERTIES
        ENVIRONMENT
            "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>;IMAS_MVDD_HLI_DD_VERSION=4.1.1")

    add_test(NAME hli-dd-version-invalid-environment-fails-first-open
        COMMAND hli_dd_version_test invalid-environment-fails-first-open)
    set_tests_properties(hli-dd-version-invalid-environment-fails-first-open PROPERTIES
        ENVIRONMENT
            "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>;IMAS_MVDD_HLI_DD_VERSION=not-a-version")

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

    add_test(NAME version-discovery-dataentry-success-forwards-uri-and-mode
        COMMAND version_discovery_test dataentry-success-forwards-uri-and-mode)
    set_tests_properties(version-discovery-dataentry-success-forwards-uri-and-mode PROPERTIES
        ENVIRONMENT "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")

    add_test(NAME version-discovery-dataentry-failure-forwards-status-unchanged
        COMMAND version_discovery_test dataentry-failure-forwards-status-unchanged)
    set_tests_properties(version-discovery-dataentry-failure-forwards-status-unchanged PROPERTIES
        ENVIRONMENT
            "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>;RECORDING_STUB_DATAENTRY_FAIL=1")

    add_test(NAME version-discovery-hli-unset-global-action-is-plain-forward
        COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_MVDD_HLI_DD_VERSION --
            $<TARGET_FILE:version_discovery_test> hli-unset-global-action-is-plain-forward)
    set_tests_properties(version-discovery-hli-unset-global-action-is-plain-forward PROPERTIES
        ENVIRONMENT "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")

    add_test(NAME version-discovery-unstamped-occurrence-forwards-datapath-unchanged
        COMMAND version_discovery_test unstamped-occurrence-forwards-datapath-unchanged)
    set_tests_properties(version-discovery-unstamped-occurrence-forwards-datapath-unchanged
        PROPERTIES ENVIRONMENT
            "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>;IMAS_MVDD_HLI_DD_VERSION=4.1.1")

    add_test(NAME version-discovery-matching-version-forwards-datapath-unchanged
        COMMAND version_discovery_test matching-version-forwards-datapath-unchanged)
    set_tests_properties(version-discovery-matching-version-forwards-datapath-unchanged PROPERTIES
        ENVIRONMENT
            "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>;IMAS_MVDD_HLI_DD_VERSION=4.1.1;RECORDING_STUB_STAMP_VERSION=4.1.1")

    add_test(NAME version-discovery-mismatch-translates-datapath-on-second-open
        COMMAND version_discovery_test mismatch-translates-datapath-on-second-open)
    set_tests_properties(version-discovery-mismatch-translates-datapath-on-second-open PROPERTIES
        ENVIRONMENT
            "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>;IMAS_MVDD_HLI_DD_VERSION=4.1.1;RECORDING_STUB_STAMP_VERSION=3.39.0")

    add_test(NAME version-discovery-unstamped-stamp-clears-an-earlier-mismatch
        COMMAND version_discovery_test unstamped-stamp-clears-an-earlier-mismatch)
    set_tests_properties(version-discovery-unstamped-stamp-clears-an-earlier-mismatch PROPERTIES
        ENVIRONMENT
            "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>;IMAS_MVDD_HLI_DD_VERSION=4.1.1")

    add_test(NAME version-discovery-failed-stamp-read-clears-an-earlier-mismatch
        COMMAND version_discovery_test failed-stamp-read-clears-an-earlier-mismatch)
    set_tests_properties(version-discovery-failed-stamp-read-clears-an-earlier-mismatch PROPERTIES
        ENVIRONMENT
            "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>;IMAS_MVDD_HLI_DD_VERSION=4.1.1")

    add_test(NAME version-discovery-malformed-stamp-refuses-and-ends-context
        COMMAND version_discovery_test malformed-stamp-refuses-and-ends-context)
    set_tests_properties(version-discovery-malformed-stamp-refuses-and-ends-context PROPERTIES
        ENVIRONMENT
            "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>;IMAS_MVDD_HLI_DD_VERSION=4.1.1;RECORDING_STUB_STAMP_VERSION=not-a-version")

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
    target_include_directories(real_core_forwarding_test PRIVATE ${_imas_core_include_dirs})
    target_compile_definitions(real_core_forwarding_test PRIVATE
        "REAL_CORE_TEST_PLUGIN_DIR=\"$<TARGET_FILE_DIR:real_core_test_plugin>\""
        "REAL_CORE_TEST_PLUGIN_NAME=\"mvddtest\"")
    target_link_libraries(real_core_forwarding_test PRIVATE imas_mvdd_loader)
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
# literal text `${CMAKE_INSTALL_PREFIX}/...` for the install script to expand.
foreach(dir LIBDIR INCLUDEDIR)
    if(IS_ABSOLUTE "${CMAKE_INSTALL_${dir}}")
        set(IMAS_MVDD_INSTALL_${dir} "${CMAKE_INSTALL_${dir}}")
    else()
        set(IMAS_MVDD_INSTALL_${dir} "\${CMAKE_INSTALL_PREFIX}/${CMAKE_INSTALL_${dir}}")
    endif()
endforeach()

install(CODE "
set(_cargo_cmd [==[${CMAKE_COMMAND};-E;env;--unset=MAKEFLAGS;--unset=MFLAGS;--unset=IMAS_CORE_BUILD_RPATH;--;${CARGO_EXECUTABLE};cinstall;${CARGO_COMMON_ARGS}]==])
list(APPEND _cargo_cmd
    --prefix \"\${CMAKE_INSTALL_PREFIX}\"
    --libdir \"${IMAS_MVDD_INSTALL_LIBDIR}\"
    --includedir \"${IMAS_MVDD_INSTALL_INCLUDEDIR}\")
if(NOT \"\$ENV{DESTDIR}\" STREQUAL \"\")
    list(APPEND _cargo_cmd --destdir \"\$ENV{DESTDIR}\")
endif()
message(STATUS \"Installing ${IMAS_MVDD_LIB_NAME} via cargo-c into \${CMAKE_INSTALL_PREFIX}\")
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
