cmake_minimum_required(VERSION 3.21)

# Guards the guard: check_script_policies.cmake only earns its place if it
# actually rejects the shapes it claims to. Each fixture is a throwaway script
# directory the checker is pointed at, so the real tests/ tree stays untouched.

foreach(required_variable CHECK_SCRIPT TEST_BINARY_DIR)
    if(NOT DEFINED ${required_variable})
        message(FATAL_ERROR "${required_variable} is required")
    endif()
endforeach()

function(run_guard_on_fixture fixture_name fixture_contents result_variable
        diagnostic_variable)
    set(fixture_dir "${TEST_BINARY_DIR}/script-policy-${fixture_name}")
    file(REMOVE_RECURSE "${fixture_dir}")
    file(MAKE_DIRECTORY "${fixture_dir}")
    if(NOT fixture_contents STREQUAL "")
        file(WRITE "${fixture_dir}/fixture.cmake" "${fixture_contents}")
    endif()

    execute_process(
        COMMAND "${CMAKE_COMMAND}" "-DSCRIPT_DIR=${fixture_dir}"
            -P "${CHECK_SCRIPT}"
        RESULT_VARIABLE guard_result
        OUTPUT_VARIABLE guard_output
        ERROR_VARIABLE guard_error)
    set("${result_variable}" "${guard_result}" PARENT_SCOPE)
    set("${diagnostic_variable}" "${guard_output}${guard_error}" PARENT_SCOPE)
endfunction()

function(expect_guard_rejection fixture_name fixture_contents
        expected_diagnostic)
    run_guard_on_fixture("${fixture_name}" "${fixture_contents}"
        guard_result guard_diagnostic)
    if(guard_result EQUAL 0)
        message(FATAL_ERROR
            "The script-policy guard accepted the ${fixture_name} fixture")
    endif()
    string(FIND "${guard_diagnostic}" "${expected_diagnostic}"
        expected_diagnostic_position)
    if(expected_diagnostic_position EQUAL -1)
        message(FATAL_ERROR
            "The ${fixture_name} fixture failed for an unexpected reason:\n"
            "${guard_diagnostic}")
    endif()
endfunction()

function(expect_guard_acceptance fixture_name fixture_contents)
    run_guard_on_fixture("${fixture_name}" "${fixture_contents}"
        guard_result guard_diagnostic)
    if(NOT guard_result EQUAL 0)
        message(FATAL_ERROR
            "The script-policy guard rejected the well-formed "
            "${fixture_name} fixture:\n${guard_diagnostic}")
    endif()
endfunction()

# An empty directory must not pass quietly: a guard that checks nothing looks
# exactly like a guard that found nothing wrong.
expect_guard_rejection(empty-directory ""
    "this guard would otherwise pass without checking anything")

expect_guard_rejection(missing-minimum
    "set(example 1)\n"
    "must begin with cmake_minimum_required")

# The pin has to precede every other command; policies set after a
# policy-gated command has already run are too late to matter.
expect_guard_rejection(late-minimum
    "set(example 1)\ncmake_minimum_required(VERSION 3.21)\n"
    "must begin with cmake_minimum_required")

# A pin older than the project floor leaves later policies unset — the exact
# state that made `IN_LIST` fail on CI while passing locally.
expect_guard_rejection(stale-minimum
    "cmake_minimum_required(VERSION 3.1)\n"
    "older than the required")

# The positive control keeps the guard from passing by rejecting everything,
# and pins the leading-comment allowance the real scripts rely on.
expect_guard_acceptance(well-formed
    "# Explanatory comment above the pin.\n\ncmake_minimum_required(VERSION 3.21)\nset(example 1)\n")
