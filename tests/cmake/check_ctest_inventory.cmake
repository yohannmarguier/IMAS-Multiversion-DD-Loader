cmake_minimum_required(VERSION 3.22)

foreach(required_variable IN ITEMS INVENTORY_FILE SELECTION_REGEX REQUIRED_TESTS)
    if(NOT DEFINED ${required_variable})
        message(FATAL_ERROR "${required_variable} is required")
    endif()
endforeach()
if(NOT EXISTS "${INVENTORY_FILE}")
    message(FATAL_ERROR "CTest inventory does not exist: ${INVENTORY_FILE}")
endif()

file(READ "${INVENTORY_FILE}" inventory)
string(JSON test_count LENGTH "${inventory}" tests)

set(all_tests)
set(disabled_tests)
set(selected_tests)
if(test_count GREATER 0)
    math(EXPR last_test "${test_count} - 1")
    foreach(test_index RANGE ${last_test})
        string(JSON test_name GET "${inventory}" tests ${test_index} name)
        list(APPEND all_tests "${test_name}")
        if(test_name MATCHES "${SELECTION_REGEX}")
            list(APPEND selected_tests "${test_name}")
        endif()

        string(JSON property_count LENGTH
            "${inventory}" tests ${test_index} properties)
        if(property_count GREATER 0)
            math(EXPR last_property "${property_count} - 1")
            foreach(property_index RANGE ${last_property})
                string(JSON property_name GET
                    "${inventory}" tests ${test_index} properties ${property_index} name)
                if(property_name STREQUAL "DISABLED")
                    string(JSON property_value GET
                        "${inventory}" tests ${test_index} properties ${property_index} value)
                    if(property_value)
                        list(APPEND disabled_tests "${test_name}")
                    endif()
                endif()
            endforeach()
        endif()
    endforeach()
endif()

list(LENGTH selected_tests selected_count)
if(selected_count EQUAL 0)
    message(FATAL_ERROR
        "selection '${SELECTION_REGEX}' selected no tests from ${INVENTORY_FILE}")
endif()

foreach(required_test IN LISTS REQUIRED_TESTS)
    if(NOT required_test IN_LIST all_tests)
        message(FATAL_ERROR "missing required test: ${required_test}")
    endif()
    if(required_test IN_LIST disabled_tests)
        message(FATAL_ERROR "required test is disabled: ${required_test}")
    endif()
endforeach()

set(expected_tests ${REQUIRED_TESTS})
list(SORT expected_tests)
list(SORT selected_tests)
if(NOT "${selected_tests}" STREQUAL "${expected_tests}")
    message(FATAL_ERROR
        "selected inventory differs from the required exact set\n"
        "  selected: ${selected_tests}\n"
        "  required: ${expected_tests}")
endif()

message(STATUS
    "validated ${selected_count} enabled required tests: ${selected_tests}")
