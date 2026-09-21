cmake_minimum_required(VERSION 3.22)

if(NOT DEFINED CHECK_SCRIPT OR NOT DEFINED TEST_BINARY_DIR)
    message(FATAL_ERROR "CHECK_SCRIPT and TEST_BINARY_DIR are required")
endif()

function(run_inventory_case name inventory expect_success expected_error)
    set(inventory_file "${TEST_BINARY_DIR}/ctest-inventory-${name}.json")
    file(WRITE "${inventory_file}" "${inventory}")
    execute_process(
        COMMAND "${CMAKE_COMMAND}"
            "-DINVENTORY_FILE=${inventory_file}"
            "-DSELECTION_REGEX=^required-"
            "-DREQUIRED_TESTS=required-one;required-two"
            -P "${CHECK_SCRIPT}"
        RESULT_VARIABLE result
        OUTPUT_VARIABLE output
        ERROR_VARIABLE error)
    set(combined "${output}${error}")

    if(expect_success)
        if(NOT result EQUAL 0)
            message(FATAL_ERROR "${name} should pass, but failed:\n${combined}")
        endif()
    else()
        if(result EQUAL 0)
            message(FATAL_ERROR "${name} should fail, but passed:\n${combined}")
        endif()
        if(NOT combined MATCHES "${expected_error}")
            message(FATAL_ERROR
                "${name} failed without '${expected_error}':\n${combined}")
        endif()
    endif()
endfunction()

set(valid [=[
{"tests":[
  {"name":"required-one","properties":[]},
  {"name":"required-two","properties":[]},
  {"name":"unrelated","properties":[]}
]}
]=])
run_inventory_case(valid "${valid}" TRUE "")

set(empty [=[{"tests":[]}]=])
run_inventory_case(empty "${empty}" FALSE "selected no tests")

set(missing [=[
{"tests":[{"name":"required-one","properties":[]}]}
]=])
run_inventory_case(missing "${missing}" FALSE "missing required test: required-two")

set(disabled [=[
{"tests":[
  {"name":"required-one","properties":[{"name":"DISABLED","value":true}]},
  {"name":"required-two","properties":[]}
]}
]=])
run_inventory_case(disabled "${disabled}" FALSE "required test is disabled: required-one")

set(unexpected [=[
{"tests":[
  {"name":"required-one","properties":[]},
  {"name":"required-two","properties":[]},
  {"name":"required-extra","properties":[]}
]}
]=])
run_inventory_case(unexpected "${unexpected}" FALSE "selected inventory differs")
