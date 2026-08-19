cmake_minimum_required(VERSION 3.21)

project(real-core-abi-mismatch LANGUAGES C)

if(NOT DEFINED SHIM_INCLUDE_DIR OR NOT DEFINED CORE_INCLUDE_DIRS)
    message(FATAL_ERROR "SHIM_INCLUDE_DIR and CORE_INCLUDE_DIRS must be supplied")
endif()

add_executable(real_core_abi_check_mismatch
    real_core_abi_core_check.c
    real_core_abi_shim_check.c)
target_include_directories(real_core_abi_check_mismatch PRIVATE
    "${SHIM_INCLUDE_DIR}"
    ${CORE_INCLUDE_DIRS})
set_target_properties(real_core_abi_check_mismatch PROPERTIES
    C_STANDARD 11
    C_STANDARD_REQUIRED YES)
