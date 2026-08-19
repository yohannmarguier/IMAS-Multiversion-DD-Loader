# Pin the policy version like every other script this suite runs with -P, so a
# later edit reaching for a policy-gated command behaves the same under the
# CMake 3.x on CI as under a 4.x local toolchain.
cmake_minimum_required(VERSION 3.21)

if(NOT DEFINED CMAKE_COMMAND OR NOT DEFINED TEST_SOURCE_DIR OR
   NOT DEFINED TEST_BINARY_DIR OR NOT DEFINED SHIM_INCLUDE_DIR OR
   NOT DEFINED CORE_INCLUDE_DIRS)
    message(FATAL_ERROR "ABI mismatch test arguments are incomplete")
endif()

set(mismatch_binary_dir "${TEST_BINARY_DIR}/real_core/real_core_abi_mismatch")
set(mismatch_include_dir "${mismatch_binary_dir}/include")
set(shim_header "${SHIM_INCLUDE_DIR}/imas_mvdd_loader.h")
set(mismatched_header "${mismatch_include_dir}/imas_mvdd_loader.h")

file(READ "${shim_header}" shim_header_contents)
string(REPLACE "#define MAXDIM 7" "#define MAXDIM 8"
    mismatched_header_contents "${shim_header_contents}")
if(shim_header_contents STREQUAL mismatched_header_contents)
    message(FATAL_ERROR "Could not inject the intentional MAXDIM mismatch")
endif()
file(MAKE_DIRECTORY "${mismatch_include_dir}")
file(WRITE "${mismatched_header}" "${mismatched_header_contents}")

execute_process(
    COMMAND "${CMAKE_COMMAND}"
        -S "${TEST_SOURCE_DIR}"
        -B "${mismatch_binary_dir}"
        "-DSHIM_INCLUDE_DIR=${mismatch_include_dir}"
        "-DCORE_INCLUDE_DIRS=${CORE_INCLUDE_DIRS}"
    RESULT_VARIABLE configure_result
    OUTPUT_VARIABLE configure_output
    ERROR_VARIABLE configure_error)
if(NOT configure_result EQUAL 0)
    message(FATAL_ERROR
        "Could not configure the intentional ABI-mismatch check:\n"
        "${configure_output}${configure_error}")
endif()

execute_process(
    COMMAND "${CMAKE_COMMAND}" --build "${mismatch_binary_dir}"
    RESULT_VARIABLE build_result
    OUTPUT_VARIABLE build_output
    ERROR_VARIABLE build_error)
set(build_diagnostic "${build_output}${build_error}")
if(build_result EQUAL 0)
    message(FATAL_ERROR
        "The ABI checker accepted an intentionally mismatched MAXDIM.\n"
        "${build_diagnostic}")
endif()

string(FIND "${build_diagnostic}" "ABI constant mismatch: MAXDIM"
    expected_diagnostic_position)
if(expected_diagnostic_position EQUAL -1)
    message(FATAL_ERROR
        "The intentional ABI-mismatch build failed for an unexpected reason; "
        "the injected MAXDIM assertion was not reported.\n${build_diagnostic}")
endif()
