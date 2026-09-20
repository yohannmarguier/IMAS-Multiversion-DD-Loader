cmake_minimum_required(VERSION 3.21)

foreach(required_variable IN ITEMS CI_WORKFLOW_FILE MUTATION_WORKFLOW_FILE
        LINE_SCOPE_FILE MUTATION_AUDIT_FILE CHECK_SCRIPT TEST_BINARY_DIR)
    if(NOT DEFINED ${required_variable})
        message(FATAL_ERROR "${required_variable} is required")
    endif()
endforeach()

file(READ "${CI_WORKFLOW_FILE}" ci_workflow)
file(READ "${MUTATION_WORKFLOW_FILE}" mutation_workflow)
file(READ "${LINE_SCOPE_FILE}" line_scope)
file(READ "${MUTATION_AUDIT_FILE}" mutation_audit)

function(expect_rejection fixture expected_diagnostic ci_contents mutation_contents
        scope_contents audit_contents)
    set(fixture_dir "${TEST_BINARY_DIR}/rust-audit-${fixture}")
    file(MAKE_DIRECTORY "${fixture_dir}")
    file(WRITE "${fixture_dir}/ci.yml" "${ci_contents}")
    file(WRITE "${fixture_dir}/mutation.yml" "${mutation_contents}")
    file(WRITE "${fixture_dir}/scope.json" "${scope_contents}")
    file(WRITE "${fixture_dir}/mutation-audit.json" "${audit_contents}")
    execute_process(
        COMMAND "${CMAKE_COMMAND}"
            "-DCI_WORKFLOW_FILE=${fixture_dir}/ci.yml"
            "-DMUTATION_WORKFLOW_FILE=${fixture_dir}/mutation.yml"
            "-DLINE_SCOPE_FILE=${fixture_dir}/scope.json"
            "-DMUTATION_AUDIT_FILE=${fixture_dir}/mutation-audit.json"
            -P "${CHECK_SCRIPT}"
        RESULT_VARIABLE result OUTPUT_VARIABLE output ERROR_VARIABLE error)
    if(result EQUAL 0)
        message(FATAL_ERROR "The Rust audit guard accepted ${fixture}")
    endif()
    string(FIND "${output}${error}" "${expected_diagnostic}" position)
    if(position EQUAL -1)
        message(FATAL_ERROR "${fixture} failed for an unexpected reason:\n${output}${error}")
    endif()
endfunction()

string(REPLACE "run: bash scripts/audit-rust-line-coverage.sh"
    "# run: bash scripts/audit-rust-line-coverage.sh" misplaced_line_command "${ci_workflow}")
if(ci_workflow STREQUAL misplaced_line_command)
    message(FATAL_ERROR "Could not remove the line-audit command")
endif()
string(APPEND misplaced_line_command
    "\n  decoy:\n    runs-on: ubuntu-latest\n    steps:\n"
    "      - run: bash scripts/audit-rust-line-coverage.sh\n")
expect_rejection(misplaced-line-command "line_coverage_job must run the scoped line-audit command"
    "${misplaced_line_command}" "${mutation_workflow}" "${line_scope}" "${mutation_audit}")

string(REPLACE "\"aggregate_percent\": 90" "\"aggregate_percent\": 89"
    weak_line_floor "${line_scope}")
if(line_scope STREQUAL weak_line_floor)
    message(FATAL_ERROR "Could not lower the line aggregate floor")
endif()
expect_rejection(weak-line-floor "line scope must enforce 90% aggregate and 80% per group"
    "${ci_workflow}" "${mutation_workflow}" "${weak_line_floor}" "${mutation_audit}")

string(REPLACE "rust-line-coverage-scope.json" "other-scope.json"
    drifted_mutation_scope "${mutation_audit}")
if(mutation_audit STREQUAL drifted_mutation_scope)
    message(FATAL_ERROR "Could not drift the mutation scope")
endif()
expect_rejection(drifted-mutation-scope
    "mutation audit must reuse line-audit scope rust-line-coverage-scope.json"
    "${ci_workflow}" "${mutation_workflow}" "${line_scope}" "${drifted_mutation_scope}")

string(REPLACE "  workflow_dispatch:" "  # workflow_dispatch:"
    missing_manual_trigger "${mutation_workflow}")
if(mutation_workflow STREQUAL missing_manual_trigger)
    message(FATAL_ERROR "Could not remove the manual trigger")
endif()
expect_rejection(missing-manual-trigger
    "mutation workflow must have a workflow_dispatch trigger"
    "${ci_workflow}" "${missing_manual_trigger}" "${line_scope}" "${mutation_audit}")
