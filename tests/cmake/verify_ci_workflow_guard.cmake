cmake_minimum_required(VERSION 3.21)

foreach(required_variable WORKFLOW_FILE TOOLCHAIN_ACTION_FILE CHECK_SCRIPT
        TEST_BINARY_DIR)
    if(NOT DEFINED ${required_variable})
        message(FATAL_ERROR "${required_variable} is required")
    endif()
endforeach()

file(READ "${WORKFLOW_FILE}" workflow)

function(expect_guard_rejection fixture_name fixture_contents
        expected_diagnostic)
    set(mutated_workflow
        "${TEST_BINARY_DIR}/ci-workflow-${fixture_name}.yml")
    file(WRITE "${mutated_workflow}" "${fixture_contents}")

    execute_process(
        COMMAND "${CMAKE_COMMAND}"
            "-DWORKFLOW_FILE=${mutated_workflow}"
            "-DTOOLCHAIN_ACTION_FILE=${TOOLCHAIN_ACTION_FILE}"
            -P "${CHECK_SCRIPT}"
        RESULT_VARIABLE check_result
        OUTPUT_VARIABLE check_output
        ERROR_VARIABLE check_error)
    set(check_diagnostic "${check_output}${check_error}")
    if(check_result EQUAL 0)
        message(FATAL_ERROR
            "The CI guard accepted the ${fixture_name} fixture")
    endif()
    string(FIND "${check_diagnostic}" "${expected_diagnostic}"
        expected_diagnostic_position)
    if(expected_diagnostic_position EQUAL -1)
        message(FATAL_ERROR
            "The ${fixture_name} fixture failed for an unexpected reason:\n"
            "${check_diagnostic}")
    endif()
endfunction()

string(REPLACE
    "        run: cargo fmt --check"
    "        # run: cargo fmt --check"
    misplaced_fast_command "${workflow}")
if(workflow STREQUAL misplaced_fast_command)
    message(FATAL_ERROR "Could not comment out the fast format command")
endif()

# Put the missing command in a later job: an unbounded substring check would
# accept it even though the fast job no longer formats anything.
string(APPEND misplaced_fast_command
    "\n  decoy:\n    runs-on: ubuntu-latest\n    steps:\n"
    "      - run: cargo fmt --check\n")
expect_guard_rejection(
    misplaced-fast-command "${misplaced_fast_command}"
    "fast_job must check formatting")

set(full_test_step
    "      - name: Test drift and real-Core seams\n        run: ctest --test-dir build --output-on-failure --no-tests=error")
set(commented_full_test_step
    "      - name: Test drift and real-Core seams\n        # run: ctest --test-dir build --output-on-failure --no-tests=error")
string(REPLACE "${full_test_step}" "${commented_full_test_step}"
    misplaced_full_command "${workflow}")
if(workflow STREQUAL misplaced_full_command)
    message(FATAL_ERROR "Could not comment out the full test command")
endif()
string(APPEND misplaced_full_command
    "\n  decoy:\n    runs-on: ubuntu-latest\n    steps:\n"
    "      - run: ctest --test-dir build --output-on-failure --no-tests=error\n")
expect_guard_rejection(
    misplaced-full-command "${misplaced_full_command}"
    "full_job must fail when its selected test profile registers no tests")
