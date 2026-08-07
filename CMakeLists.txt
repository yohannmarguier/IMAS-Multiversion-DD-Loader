    cmake_minimum_required(VERSION 3.20)

project(imas-mvdd-loader
    VERSION 0.1.0
    DESCRIPTION "Middleware shim between the IMAS HLIs and IMAS-Core"
    LANGUAGES C)

include(GNUInstallDirs)

# CMake does not build the Rust code itself: cargo-c does, and CMake drives it.
# That keeps one build system responsible for the C ABI artefacts (library,
# generated header, pkg-config file) and lets consumers depend on this project
# the way they depend on IMAS-Core.

option(IMAS_MVDD_BUILD_TESTS "Build and register the test suite" ON)
option(IMAS_MVDD_CARGO_OFFLINE "Pass --offline to cargo (build nodes without network)" OFF)

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
    COMMAND "${CARGO_EXECUTABLE}" cinstall ${CARGO_COMMON_ARGS} ${CARGO_STAGE_ARGS}
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

    add_test(NAME rust-unit
        COMMAND "${CARGO_EXECUTABLE}" test ${CARGO_COMMON_ARGS})

    add_executable(abi_smoke "${CMAKE_CURRENT_SOURCE_DIR}/tests/abi_smoke.c")
    target_link_libraries(abi_smoke PRIVATE imas_mvdd_loader)
    add_dependencies(abi_smoke imas_mvdd_capi)
    set_target_properties(abi_smoke PROPERTIES
        BUILD_RPATH "${IMAS_MVDD_STAGE_DIR}/lib")

    add_test(NAME abi-smoke COMMAND abi_smoke)

    # --- Runtime-binding test: proves the architecture (issue #3) ---------
    #
    # A recording stub stands in for IMAS-Core: it exports the ABI symbols
    # the shim resolves at runtime (al_context_info, getALVersion) and
    # records what it received, so assertions can be made directly on what
    # crossed the boundary instead of inferring it from a data round trip.
    #
    # The stub is deliberately never linked into runtime_binding_test: doing so would
    # give the linker two candidate definitions of al_context_info (the
    # shim's and the stub's) to choose between — exactly the ambiguity
    # runtime binding exists to avoid. runtime_binding_test only dlopen's the stub
    # itself, purely to read back its recorded state.
    add_library(recording_stub SHARED
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/stub/recording_stub.c")
    # Matches the shim's bare-soname fallback (src/resolve.rs) so the
    # bare-soname scenario below can exercise the loader's normal search
    # path instead of the explicit override.
    set_target_properties(recording_stub PROPERTIES OUTPUT_NAME al)
    target_compile_definitions(recording_stub PRIVATE
        "RECORDING_STUB_DEFAULT_VERSION=\"${IMAS_CORE_VERSION}\"")

    add_executable(runtime_binding_test
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/runtime_binding_test.c")
    target_link_libraries(runtime_binding_test PRIVATE imas_mvdd_loader ${CMAKE_DL_LIBS})
    target_compile_definitions(runtime_binding_test PRIVATE
        "RECORDING_STUB_PATH=\"$<TARGET_FILE:recording_stub>\""
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

    add_test(NAME runtime-binding-bare-soname COMMAND runtime_binding_test bare-soname)
    if(APPLE)
        set(_runtime_binding_search_path_var DYLD_LIBRARY_PATH)
    else()
        set(_runtime_binding_search_path_var LD_LIBRARY_PATH)
    endif()
    set_tests_properties(runtime-binding-bare-soname PROPERTIES ENVIRONMENT
        "${_runtime_binding_search_path_var}=$<TARGET_FILE_DIR:recording_stub>")
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
set(_cargo_cmd [==[${CARGO_EXECUTABLE};cinstall;${CARGO_COMMON_ARGS}]==])
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
