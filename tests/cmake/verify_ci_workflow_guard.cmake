cmake_minimum_required(VERSION 3.21)

foreach(required_variable WORKFLOW_FILE TOOLCHAIN_ACTION_FILE GRAPH_SETUP_ACTION_FILE CHECK_SCRIPT
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
            "-DGRAPH_SETUP_ACTION_FILE=${GRAPH_SETUP_ACTION_FILE}"
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

string(REPLACE
    "ctest --test-dir build -L graph-runtime-map --output-on-failure --no-tests=error"
    "ctest --test-dir build -L graph-runtime-map-missing --output-on-failure --no-tests=error"
    graph_abi_missing_selection "${workflow}")
if(workflow STREQUAL graph_abi_missing_selection)
    message(FATAL_ERROR "Could not replace the graph ABI CTest label")
endif()
expect_guard_rejection(
    graph-abi-missing-selection "${graph_abi_missing_selection}"
    "graph_abi_job must run the nonempty graph-selected C ABI matrix")

string(REPLACE
    "run: cargo test pinned_graph_returns_a_complete_equilibrium_scope --lib -- --ignored"
    "# run: cargo test pinned_graph_returns_a_complete_equilibrium_scope --lib -- --ignored"
    graph_abi_without_acquisition "${workflow}")
if(workflow STREQUAL graph_abi_without_acquisition)
    message(FATAL_ERROR "Could not comment out the graph ABI acquisition check")
endif()
expect_guard_rejection(
    graph-abi-without-acquisition "${graph_abi_without_acquisition}"
    "graph_abi_job must fail when live graph acquisition")

string(REPLACE
    "          ctest --test-dir build -R \"$pattern\" --output-on-failure --no-tests=error"
    "          # ctest --test-dir build -R \"$pattern\" --output-on-failure --no-tests=error"
    missing_coexistence_run "${workflow}")
if(workflow STREQUAL missing_coexistence_run)
    message(FATAL_ERROR "Could not comment out the graph coexistence test command")
endif()
string(APPEND missing_coexistence_run
    "\n  decoy:\n    runs-on: ubuntu-latest\n    steps:\n"
    "      - run: ctest --test-dir build -R \"$pattern\" --output-on-failure --no-tests=error\n")
expect_guard_rejection(
    missing-coexistence-run "${missing_coexistence_run}"
    "full_job must execute the graph coexistence real-Core scenarios")

string(REPLACE
    "ref=$(head -n1 \"$GITHUB_WORKSPACE/IMAS_CORE_REF\" | tr -d '[:space:]')"
    "ref=690f5392a58e4c73131d6b723c72105e9fbdcc9f"
    inline_pin "${workflow}")
if(workflow STREQUAL inline_pin)
    message(FATAL_ERROR "Could not inline the IMAS-Core pin")
endif()
expect_guard_rejection(
    inline-pin "${inline_pin}"
    "workflow must not inline an IMAS-Core commit SHA")

string(REPLACE
    "ref=$(head -n1 \"$GITHUB_WORKSPACE/IMAS_CORE_REF\" | tr -d '[:space:]')"
    "ref=$(head -n1 \"$GITHUB_WORKSPACE/PINNED_COMMIT\" | tr -d '[:space:]')"
    wrong_pin_file "${workflow}")
if(workflow STREQUAL wrong_pin_file)
    message(FATAL_ERROR "Could not replace the IMAS-Core pin-file read")
endif()
expect_guard_rejection(
    wrong-pin-file "${wrong_pin_file}"
    "full_job must write a value read from IMAS_CORE_REF to GITHUB_OUTPUT")

set(upstream_repository "${workflow}")
string(APPEND upstream_repository
    "\n  decoy:\n    runs-on: ubuntu-latest\n    steps:\n"
    "      - run: git clone https://github.com/iterorganization/IMAS-Core.git\n")
if(workflow STREQUAL upstream_repository)
    message(FATAL_ERROR "Could not append an upstream IMAS-Core clone")
endif()
expect_guard_rejection(
    upstream-repository "${upstream_repository}"
    "workflow must not name the upstream IMAS-Core repository")

string(REPLACE
    "key: al-core-\${{ runner.os }}-Release-\${{ steps.imas_core_ref.outputs.commit }}"
    "key: al-core-\${{ runner.os }}-Release-\${{ hashFiles('IMAS_CORE_VERSION') }}"
    cache_key_misses_resolved_pin "${workflow}")
if(workflow STREQUAL cache_key_misses_resolved_pin)
    message(FATAL_ERROR "Could not replace the resolved IMAS-Core cache key")
endif()
expect_guard_rejection(
    cache-key-misses-resolved-pin "${cache_key_misses_resolved_pin}"
    "full_job must key the acquired IMAS-Core cache on the resolved pin")

string(REPLACE
    "key: al-core-"
    "key: al-core-\${{ hashFiles('IMAS_CORE_VERSION') }}-"
    cache_key_uses_release_version "${workflow}")
if(workflow STREQUAL cache_key_uses_release_version)
    message(FATAL_ERROR "Could not add IMAS_CORE_VERSION to the cache key")
endif()
expect_guard_rejection(
    cache-key-uses-release-version "${cache_key_uses_release_version}"
    "full_job must not key the acquired IMAS-Core cache on IMAS_CORE_VERSION")
