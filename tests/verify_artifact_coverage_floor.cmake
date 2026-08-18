cmake_minimum_required(VERSION 3.21)

foreach(required_variable CARGO_EXECUTABLE CARGO_MANIFEST_PATH APPROVED_ARTIFACT REDUCED_ARTIFACT
        LEFT_INVENTORY RIGHT_INVENTORY BASELINE_TSV NEAR_BOUNDARY_RULE_ID
        FORWARD_SUPPORTED_FLOOR REVERSE_SUPPORTED_FLOOR WORK_DIR)
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

# Every number this gate compares against comes from a checked-in file rather
# than from a literal repeated here: the inventories are the coverage report's
# own denominator, and the baseline TSV is the floor's. A hardcoded `49/49` once
# stood where `count_entries(BASELINE_TSV)` stands now, so adding a baseline
# mapping would have left the gate asserting the old total.
function(count_entries file count_variable)
    file(STRINGS "${file}" entries REGEX "^[^#]")
    list(FILTER entries EXCLUDE REGEX "^[ \t]*$")
    list(LENGTH entries count)
    set("${count_variable}" "${count}" PARENT_SCOPE)
endfunction()

# Pulls one direction's integers out of the report line. Asserting on the
# substring `supported=` alone -- which ends at the `=` -- passed whatever
# followed it, so `supported=5` satisfied a gate meant to protect 342.
function(read_direction output direction supported_variable total_variable)
    if(NOT output MATCHES
            "shim ${direction}: supported=([0-9]+), by rule=([0-9]+), by identity default=([0-9]+), deliberate refusal=([0-9]+), absent stored source=([0-9]+), total=([0-9]+)")
        message(FATAL_ERROR
            "the report has no parsable `shim ${direction}` line:\n${output}")
    endif()
    set(supported "${CMAKE_MATCH_1}")
    set(by_rule "${CMAKE_MATCH_2}")
    set(by_default "${CMAKE_MATCH_3}")
    set(refusal "${CMAKE_MATCH_4}")
    set(absent "${CMAKE_MATCH_5}")
    set(total "${CMAKE_MATCH_6}")

    math(EXPR claimed_sum "${by_rule} + ${by_default}")
    if(NOT claimed_sum EQUAL supported)
        message(FATAL_ERROR
            "`shim ${direction}`: by rule=${by_rule} + by identity default=${by_default} "
            "does not add up to supported=${supported}")
    endif()
    math(EXPR classified_sum "${supported} + ${refusal} + ${absent}")
    if(NOT classified_sum EQUAL total)
        message(FATAL_ERROR
            "`shim ${direction}`: the three outcome counts sum to ${classified_sum}, "
            "not total=${total}")
    endif()

    set("${supported_variable}" "${supported}" PARENT_SCOPE)
    set("${total_variable}" "${total}" PARENT_SCOPE)
endfunction()

count_entries("${LEFT_INVENTORY}" left_inventory_size)
count_entries("${RIGHT_INVENTORY}" right_inventory_size)
count_entries("${BASELINE_TSV}" baseline_size)

run_validator("${APPROVED_ARTIFACT}" approved_result approved_output)
if(NOT approved_result EQUAL 0)
    message(FATAL_ERROR
        "the approved artifact must meet the IMAS-Python autoconvert floor:\n${approved_output}")
endif()

read_direction("${approved_output}" "3.39.0 -> 4.1.1" forward_supported forward_total)
read_direction("${approved_output}" "4.1.1 -> 3.39.0" reverse_supported reverse_total)

# The report must have swept every inventory path, not a subset: its own total is
# checked against the inventory files the artifact names as its two sides.
if(NOT forward_total EQUAL left_inventory_size)
    message(FATAL_ERROR
        "the forward direction swept ${forward_total} paths but the 3.39.0 inventory "
        "holds ${left_inventory_size}:\n${approved_output}")
endif()
if(NOT reverse_total EQUAL right_inventory_size)
    message(FATAL_ERROR
        "the reverse direction swept ${reverse_total} paths but the 4.1.1 inventory "
        "holds ${right_inventory_size}:\n${approved_output}")
endif()

# Pinned floors, not equalities: coverage may rise freely, and a rule that
# converts a previously unserved path is the point. A drop is what this catches.
if(forward_supported LESS FORWARD_SUPPORTED_FLOOR)
    message(FATAL_ERROR
        "forward supported coverage fell to ${forward_supported}, below the pinned "
        "floor of ${FORWARD_SUPPORTED_FLOOR}:\n${approved_output}")
endif()
if(reverse_supported LESS REVERSE_SUPPORTED_FLOOR)
    message(FATAL_ERROR
        "reverse supported coverage fell to ${reverse_supported}, below the pinned "
        "floor of ${REVERSE_SUPPORTED_FLOOR}:\n${approved_output}")
endif()

foreach(expected_line
        "IMAS-Python rename-only floor 3.39.0 -> 4.1.1: ${baseline_size}/${baseline_size} mappings served"
        "IMAS-Python rename-only floor 4.1.1 -> 3.39.0: ${baseline_size}/${baseline_size} mappings served")
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

# The reduced fixture drops every rule at once, so it only proves rejection far
# from the boundary. This one is the approved artifact minus a single rename
# rule, generated here rather than checked in so it cannot drift from the
# artifact it is derived from: the gate has to reject a one-mapping shortfall,
# which is the smallest regression a real edit can produce.
file(READ "${APPROVED_ARTIFACT}" approved_artifact_text)
# `[^<]*` keeps this anchored to the one rule element: a rule body holds no `<`
# between its own opening tag and its `<fidelity>` child, nor between that child
# and its `</rule>`, so the match cannot run past the rule it names.
string(REGEX REPLACE
    "<rule id=\"${NEAR_BOUNDARY_RULE_ID}\"[^<]*<fidelity[^<]*</rule>"
    ""
    near_boundary_artifact_text "${approved_artifact_text}")
if(near_boundary_artifact_text STREQUAL approved_artifact_text)
    message(FATAL_ERROR
        "could not strip rule `${NEAR_BOUNDARY_RULE_ID}` from ${APPROVED_ARTIFACT}")
endif()
set(near_boundary_artifact "${WORK_DIR}/equilibrium-3.39.0--4.1.1-one-rename-short.xml")
file(WRITE "${near_boundary_artifact}" "${near_boundary_artifact_text}")

run_validator("${near_boundary_artifact}" near_boundary_result near_boundary_output)
math(EXPR one_short "${baseline_size} - 1")
if(near_boundary_result EQUAL 0)
    message(FATAL_ERROR
        "dropping rule `${NEAR_BOUNDARY_RULE_ID}` must be rejected: the artifact then serves "
        "one baseline mapping fewer than the floor:\n${near_boundary_output}")
endif()
if(NOT near_boundary_output MATCHES "3.39.0 -> 4.1.1 ${one_short}/${baseline_size}")
    message(FATAL_ERROR
        "dropping rule `${NEAR_BOUNDARY_RULE_ID}` must report exactly "
        "${one_short}/${baseline_size} mappings served forward:\n${near_boundary_output}")
endif()
