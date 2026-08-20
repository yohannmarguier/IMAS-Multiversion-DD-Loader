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
    include("${CMAKE_CURRENT_SOURCE_DIR}/cmake/Tests.cmake")
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
