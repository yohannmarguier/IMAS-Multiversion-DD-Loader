cmake_minimum_required(VERSION 3.21)

foreach(required_variable CARGO_EXECUTABLE CARGO_MANIFEST_PATH APPROVED_ARTIFACT REDUCED_ARTIFACT)
    if(NOT DEFINED ${required_variable})
        message(FATAL_ERROR "${required_variable} must be supplied")
    endif()
endforeach()

function(run_validator artifact result_variable output_variable)
    execute_process(
        COMMAND "${CARGO_EXECUTABLE}" run
            --manifest-path "${CARGO_MANIFEST_PATH}"
            --bin validate_equilibrium_coverage
            -- --artifact "${artifact}"
        RESULT_VARIABLE result
        OUTPUT_VARIABLE output
        ERROR_VARIABLE error)
    set("${result_variable}" "${result}" PARENT_SCOPE)
    set("${output_variable}" "${output}${error}" PARENT_SCOPE)
endfunction()

run_validator("${APPROVED_ARTIFACT}" approved_result approved_output)
if(NOT approved_result EQUAL 0)
    message(FATAL_ERROR
        "the approved artifact must meet the IMAS-Python autoconvert floor:\n${approved_output}")
endif()
foreach(expected_line
        "shim 3.39.0 -> 4.1.1: supported="
        "shim 4.1.1 -> 3.39.0: supported="
        "deliberate refusal="
        "absent stored source="
        "IMAS-Python rename-only floor 3.39.0 -> 4.1.1: 49/49 mappings served"
        "IMAS-Python rename-only floor 4.1.1 -> 3.39.0: 49/49 mappings served")
    string(FIND "${approved_output}" "${expected_line}" expected_line_offset)
    if(expected_line_offset EQUAL -1)
        message(FATAL_ERROR
            "the approved-artifact report is missing `${expected_line}`:\n${approved_output}")
    endif()
endforeach()

run_validator("${REDUCED_ARTIFACT}" reduced_result reduced_output)
if(reduced_result EQUAL 0 OR NOT reduced_output MATCHES "falls below the IMAS-Python rename-only floor")
    message(FATAL_ERROR
        "a deliberately reduced rule set must be rejected below the IMAS-Python floor:\n${reduced_output}")
endif()
