cmake_minimum_required(VERSION 3.21)

project(real-core-abi-mismatch LANGUAGES CXX)

if(NOT DEFINED SHIM_INCLUDE_DIR OR NOT DEFINED CORE_INCLUDE_DIRS)
    message(FATAL_ERROR "SHIM_INCLUDE_DIR and CORE_INCLUDE_DIRS must be supplied")
endif()

add_executable(real_core_abi_check_mismatch ../real_core_abi_check.cpp)
target_include_directories(real_core_abi_check_mismatch PRIVATE
    "${SHIM_INCLUDE_DIR}"
    ${CORE_INCLUDE_DIRS})
set_target_properties(real_core_abi_check_mismatch PROPERTIES
    CXX_STANDARD 17
    CXX_STANDARD_REQUIRED YES)
