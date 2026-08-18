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
# Each direction's counts must be numbers, and the three buckets must account
# for every path the inventory holds. The previous form of this check searched
# for the substrings "supported=", "deliberate refusal=" and "absent stored
# source=" — each ending at the `=` — so it passed on any report merely shaped
# like a report, `supported=0` included. That is the kind of easily-satisfied
# summary check ADR 0013 exists to reject.
#
# What this deliberately still does not do is pin the counts to a floor, or
# prove rejection near the boundary rather than at 0/49. Both are finding P3 of
# the read-path review, whose judgement about what the floor should be is not
# settled here; this only stops the check from passing on a degenerate report.
foreach(direction "3.39.0 -> 4.1.1" "4.1.1 -> 3.39.0")
    # The version dots are literal, not regex wildcards; a gate must not match a
    # direction it was not asked about.
    string(REPLACE "." "\\." direction_pattern "${direction}")
    set(counts_pattern
        "shim ${direction_pattern}: supported=([0-9]+), deliberate refusal=([0-9]+), absent stored source=([0-9]+), total=([0-9]+)")
    if(NOT approved_output MATCHES "${counts_pattern}")
        message(FATAL_ERROR
            "the approved-artifact report has no numeric counts for ${direction}:\n${approved_output}")
    endif()
    set(supported "${CMAKE_MATCH_1}")
    set(refused "${CMAKE_MATCH_2}")
    set(absent "${CMAKE_MATCH_3}")
    set(total "${CMAKE_MATCH_4}")

    math(EXPR accounted "${supported} + ${refused} + ${absent}")
    if(NOT accounted EQUAL total)
        message(FATAL_ERROR
            "${direction}: supported=${supported} + deliberate refusal=${refused} + absent stored "
            "source=${absent} is ${accounted}, which does not account for total=${total}:\n"
            "${approved_output}")
    endif()
    if(total EQUAL 0)
        message(FATAL_ERROR
            "${direction}: the report claims an empty inventory:\n${approved_output}")
    endif()
endforeach()

foreach(expected_line
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
