cmake_minimum_required(VERSION 3.21)

foreach(required_variable IN ITEMS CI_WORKFLOW_FILE MUTATION_WORKFLOW_FILE
        LINE_AUDIT_SCRIPT PRODUCTION_TESTS_FILE LINE_SCOPE_FILE XML_LINE_SCOPE_FILE
        MUTATION_AUDIT_FILE CHECK_SCRIPT TEST_BINARY_DIR)
    if(NOT DEFINED ${required_variable})
        message(FATAL_ERROR "${required_variable} is required")
    endif()
endforeach()

file(READ "${CI_WORKFLOW_FILE}" ci_workflow)
file(READ "${MUTATION_WORKFLOW_FILE}" mutation_workflow)
file(READ "${LINE_AUDIT_SCRIPT}" line_audit_script)
file(READ "${PRODUCTION_TESTS_FILE}" production_tests)
file(READ "${LINE_SCOPE_FILE}" line_scope)
file(READ "${XML_LINE_SCOPE_FILE}" xml_line_scope)
file(READ "${MUTATION_AUDIT_FILE}" mutation_audit)

function(expect_rejection fixture expected_diagnostic ci_contents mutation_contents
        script_contents tests_contents scope_contents xml_scope_contents audit_contents)
    set(fixture_dir "${TEST_BINARY_DIR}/rust-audit-${fixture}")
    file(MAKE_DIRECTORY "${fixture_dir}")
    file(WRITE "${fixture_dir}/ci.yml" "${ci_contents}")
    file(WRITE "${fixture_dir}/mutation.yml" "${mutation_contents}")
    file(WRITE "${fixture_dir}/audit-line.sh" "${script_contents}")
    file(WRITE "${fixture_dir}/production-tests.json" "${tests_contents}")
    file(WRITE "${fixture_dir}/scope.json" "${scope_contents}")
    file(WRITE "${fixture_dir}/xml-scope.json" "${xml_scope_contents}")
    file(WRITE "${fixture_dir}/mutation-audit.json" "${audit_contents}")
    execute_process(
        COMMAND "${CMAKE_COMMAND}"
            "-DCI_WORKFLOW_FILE=${fixture_dir}/ci.yml"
            "-DMUTATION_WORKFLOW_FILE=${fixture_dir}/mutation.yml"
            "-DLINE_AUDIT_SCRIPT=${fixture_dir}/audit-line.sh"
            "-DPRODUCTION_TESTS_FILE=${fixture_dir}/production-tests.json"
            "-DLINE_SCOPE_FILE=${fixture_dir}/scope.json"
            "-DXML_LINE_SCOPE_FILE=${fixture_dir}/xml-scope.json"
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
    "${misplaced_line_command}" "${mutation_workflow}" "${line_audit_script}"
    "${production_tests}" "${line_scope}" "${xml_line_scope}" "${mutation_audit}")

string(REPLACE "\"aggregate_percent\": 90" "\"aggregate_percent\": 89"
    weak_line_floor "${line_scope}")
if(line_scope STREQUAL weak_line_floor)
    message(FATAL_ERROR "Could not lower the line aggregate floor")
endif()
expect_rejection(weak-line-floor "line scope must enforce 90% aggregate and 80% per group"
    "${ci_workflow}" "${mutation_workflow}" "${line_audit_script}"
    "${production_tests}" "${weak_line_floor}" "${xml_line_scope}" "${mutation_audit}")

string(REPLACE "rust-xml-line-coverage-scope.json" "other-scope.json"
    drifted_mutation_scope "${mutation_audit}")
if(mutation_audit STREQUAL drifted_mutation_scope)
    message(FATAL_ERROR "Could not drift the mutation scope")
endif()
expect_rejection(drifted-mutation-scope
    "mutation audit must reuse the XML line-audit scope"
    "${ci_workflow}" "${mutation_workflow}" "${line_audit_script}"
    "${production_tests}" "${line_scope}" "${xml_line_scope}" "${drifted_mutation_scope}")

string(REPLACE "  workflow_dispatch:" "  # workflow_dispatch:"
    missing_manual_trigger "${mutation_workflow}")
if(mutation_workflow STREQUAL missing_manual_trigger)
    message(FATAL_ERROR "Could not remove the manual trigger")
endif()
expect_rejection(missing-manual-trigger
    "mutation workflow must have a workflow_dispatch trigger"
    "${ci_workflow}" "${missing_manual_trigger}" "${line_audit_script}"
    "${production_tests}" "${line_scope}" "${xml_line_scope}" "${mutation_audit}")

set(production_command
    "cargo llvm-cov --no-default-features --lib --lcov --output-path \"$production_report\"")
string(REPLACE "${production_command}" "# ${production_command}"
    missing_production_run "${line_audit_script}")
if(line_audit_script STREQUAL missing_production_run)
    message(FATAL_ERROR "Could not remove the production line-coverage run")
endif()
expect_rejection(missing-production-run
    "line_audit_lines must measure the production build"
    "${ci_workflow}" "${mutation_workflow}" "${missing_production_run}"
    "${production_tests}" "${line_scope}" "${xml_line_scope}" "${mutation_audit}")

string(REPLACE "cargo llvm-cov clean --workspace"
    "# cargo llvm-cov clean --workspace" missing_fresh_profiles "${line_audit_script}")
if(line_audit_script STREQUAL missing_fresh_profiles)
    message(FATAL_ERROR "Could not remove the fresh-profile commands")
endif()
expect_rejection(missing-fresh-profiles
    "line_audit_lines must run exactly two fresh-profile cleans"
    "${ci_workflow}" "${mutation_workflow}" "${missing_fresh_profiles}"
    "${production_tests}" "${line_scope}" "${xml_line_scope}" "${mutation_audit}")

string(REPLACE "set +e" "# set +e" early_threshold_exit "${line_audit_script}")
if(line_audit_script STREQUAL early_threshold_exit)
    message(FATAL_ERROR "Could not restore fail-fast behavior around the scoped checks")
endif()
expect_rejection(early-threshold-exit
    "line_audit_lines must continue after a threshold failure"
    "${ci_workflow}" "${mutation_workflow}" "${early_threshold_exit}"
    "${production_tests}" "${line_scope}" "${xml_line_scope}" "${mutation_audit}")

string(REPLACE ": >\"$coverage_report\""
    "# : >\"$coverage_report\"" missing_report_truncation
    "${line_audit_script}")
if(line_audit_script STREQUAL missing_report_truncation)
    message(FATAL_ERROR "Could not remove the stale-report truncation")
endif()
expect_rejection(missing-report-truncation
    "line_audit_lines must truncate stale evidence before fallible setup"
    "${ci_workflow}" "${mutation_workflow}" "${missing_report_truncation}"
    "${production_tests}" "${line_scope}" "${xml_line_scope}" "${mutation_audit}")

string(REPLACE
    "python3 \"$root_dir/scripts/check-rust-line-coverage.py\" --initialize-report --report-output \"$coverage_report\""
    "# python3 \"$root_dir/scripts/check-rust-line-coverage.py\" --initialize-report --report-output \"$coverage_report\""
    missing_initial_report "${line_audit_script}")
if(line_audit_script STREQUAL missing_initial_report)
    message(FATAL_ERROR "Could not remove the initial line-coverage report")
endif()
expect_rejection(missing-initial-report
    "line_audit_lines must initialize the report through the checker"
    "${ci_workflow}" "${mutation_workflow}" "${missing_initial_report}"
    "${production_tests}" "${line_scope}" "${xml_line_scope}" "${mutation_audit}")

string(REPLACE "          target/rust-line-coverage-report.md"
    "          # target/rust-line-coverage-report.md" missing_markdown_report "${ci_workflow}")
if(ci_workflow STREQUAL missing_markdown_report)
    message(FATAL_ERROR "Could not remove the Markdown line-coverage report upload")
endif()
expect_rejection(missing-markdown-report
    "line_coverage_job must upload the human-readable line-coverage report"
    "${missing_markdown_report}" "${mutation_workflow}" "${line_audit_script}"
    "${production_tests}" "${line_scope}" "${xml_line_scope}" "${mutation_audit}")

string(REPLACE "${production_command}"
    "cargo llvm-cov --all-features --lib --lcov --output-path \"$production_report\""
    all_features_production_run "${line_audit_script}")
if(line_audit_script STREQUAL all_features_production_run)
    message(FATAL_ERROR "Could not replace the production run with all-features")
endif()
expect_rejection(all-features-production-run
    "line_audit_lines must measure the production build"
    "${ci_workflow}" "${mutation_workflow}" "${all_features_production_run}"
    "${production_tests}" "${line_scope}" "${xml_line_scope}" "${mutation_audit}")

string(REPLACE
    "python3 scripts/check-rust-line-coverage.py --root \"$root_dir\" --scope \"$scope\" --lcov \"$production_report\""
    "python3 scripts/check-rust-line-coverage.py --root \"$root_dir\" --scope \"$scope\" --lcov \"$xml_report\""
    production_without_runtime_measurement "${line_audit_script}")
if(line_audit_script STREQUAL production_without_runtime_measurement)
    message(FATAL_ERROR "Could not replace the production measurement")
endif()
expect_rejection(production-without-runtime-measurement
    "line_audit_lines must score and report the production configuration"
    "${ci_workflow}" "${mutation_workflow}" "${production_without_runtime_measurement}"
    "${production_tests}" "${line_scope}" "${xml_line_scope}" "${mutation_audit}")

string(REPLACE
    "    \"retry\": \"conversion::runtime_map::tests::an_expired_attempt_cannot_replace_a_later_retry\",\n"
    "" missing_retry_witness "${production_tests}")
if(production_tests STREQUAL missing_retry_witness)
    message(FATAL_ERROR "Could not remove the retry test witness")
endif()
expect_rejection(missing-production-test-witness
    "production test witnesses must retain the retry runtime-map scenario"
    "${ci_workflow}" "${mutation_workflow}" "${line_audit_script}"
    "${missing_retry_witness}" "${line_scope}" "${xml_line_scope}" "${mutation_audit}")

set(ignored_inventory_command
    "cargo test --no-default-features --lib -- --ignored --list >\"$ignored_production_tests\"")
string(REPLACE "${ignored_inventory_command}" "# ${ignored_inventory_command}"
    missing_ignored_inventory "${line_audit_script}")
if(line_audit_script STREQUAL missing_ignored_inventory)
    message(FATAL_ERROR "Could not remove the ignored production-test inventory")
endif()
expect_rejection(missing-ignored-production-test-inventory
    "line_audit_lines must inventory ignored production tests"
    "${ci_workflow}" "${mutation_workflow}" "${missing_ignored_inventory}"
    "${production_tests}" "${line_scope}" "${xml_line_scope}" "${mutation_audit}")

string(REPLACE
    "        {\"path\": \"src/conversion/runtime_map/neo4j_graph.rs\", \"start_line\": 202, \"end_line\": 552},\n"
    "" missing_graph_source "${line_scope}")
if(line_scope STREQUAL missing_graph_source)
    message(FATAL_ERROR "Could not remove the production graph source assignment")
endif()
expect_rejection(missing-production-graph-source
    "production line scope must measure runtime_map/neo4j_graph.rs"
    "${ci_workflow}" "${mutation_workflow}" "${line_audit_script}"
    "${production_tests}" "${missing_graph_source}" "${xml_line_scope}" "${mutation_audit}")
