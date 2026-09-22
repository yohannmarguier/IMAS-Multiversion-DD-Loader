cmake_minimum_required(VERSION 3.21)

foreach(required_variable IN ITEMS CI_WORKFLOW_FILE MUTATION_WORKFLOW_FILE
        LINE_AUDIT_SCRIPT PRODUCTION_TESTS_FILE LINE_SCOPE_FILE XML_LINE_SCOPE_FILE
        MUTATION_AUDIT_FILE)
    if(NOT DEFINED ${required_variable})
        message(FATAL_ERROR "${required_variable} is required")
    endif()
endforeach()

function(read_lines path output_variable)
    file(READ "${path}" contents)
    string(REPLACE "\r\n" "\n" contents "${contents}")
    string(REPLACE "[" "@LBRACKET@" contents "${contents}")
    string(REPLACE "]" "@RBRACKET@" contents "${contents}")
    string(REPLACE "\n" ";" lines "${contents}")
    set("${output_variable}" "${lines}" PARENT_SCOPE)
endfunction()

function(job_lines workflow_lines_variable job_name output_variable)
    set(in_jobs FALSE)
    set(in_job FALSE)
    set(lines)
    foreach(line IN LISTS ${workflow_lines_variable})
        if(line STREQUAL "jobs:")
            set(in_jobs TRUE)
            continue()
        endif()
        if(NOT in_jobs)
            continue()
        endif()
        if(line MATCHES "^  ([A-Za-z0-9_-]+):[ \\t]*$")
            if(in_job)
                break()
            endif()
            if(CMAKE_MATCH_1 STREQUAL "${job_name}")
                set(in_job TRUE)
            endif()
            continue()
        endif()
        if(in_job)
            string(STRIP "${line}" line)
            if(NOT line STREQUAL "" AND NOT line MATCHES "^#")
                list(APPEND lines "${line}")
            endif()
        endif()
    endforeach()
    if(NOT in_job)
        message(FATAL_ERROR "audit workflow must define a ${job_name} job")
    endif()
    set("${output_variable}" "${lines}" PARENT_SCOPE)
endfunction()

function(require_line lines_variable expected description)
    string(REPLACE "[" "@LBRACKET@" expected "${expected}")
    string(REPLACE "]" "@RBRACKET@" expected "${expected}")
    foreach(actual IN LISTS ${lines_variable})
        string(STRIP "${actual}" actual)
        if(actual STREQUAL expected)
            return()
        endif()
    endforeach()
    message(FATAL_ERROR "${lines_variable} must ${description}")
endfunction()

read_lines("${CI_WORKFLOW_FILE}" ci_lines)
read_lines("${MUTATION_WORKFLOW_FILE}" mutation_lines)
read_lines("${LINE_AUDIT_SCRIPT}" line_audit_lines)
job_lines(ci_lines rust-line-coverage line_coverage_job)
job_lines(ci_lines rust-integration-coverage integration_coverage_job)
job_lines(ci_lines full full_job)
job_lines(mutation_lines rust-mutation-audit mutation_job)
set(mutation_flat)
foreach(line IN LISTS mutation_lines)
    string(STRIP "${line}" line)
    if(NOT line STREQUAL "" AND NOT line MATCHES "^#")
        list(APPEND mutation_flat "${line}")
    endif()
endforeach()

require_line(line_coverage_job "- uses: ./.github/actions/setup-toolchain"
    "use the pinned Rust toolchain")
require_line(line_coverage_job
    "rustup component add llvm-tools-preview --toolchain \"$RUST_VERSION\""
    "install llvm-tools for the pinned Rust toolchain")
require_line(line_coverage_job
    "cargo install cargo-llvm-cov --version \"$CARGO_LLVM_COV_VERSION\" --locked"
    "install the pinned cargo-llvm-cov release")
require_line(line_coverage_job "run: bash scripts/audit-rust-line-coverage.sh"
    "run the scoped line-audit command")
require_line(line_audit_lines
    "cargo test --no-default-features --lib -- --list >\"$production_tests\""
    "inventory the production tests before the unfiltered coverage run")
require_line(line_audit_lines
    "cargo test --no-default-features --lib -- --ignored --list >\"$ignored_production_tests\""
    "inventory ignored production tests before the unfiltered coverage run")
require_line(line_audit_lines
    "python3 scripts/check-rust-test-witnesses.py --manifest \"$production_test_witnesses\" --discovered \"$production_tests\" --ignored \"$ignored_production_tests\""
    "reject missing or ignored production runtime-map witnesses")
require_line(line_audit_lines
    "cargo llvm-cov --no-default-features --lib --lcov --output-path \"$production_report\""
    "measure the production build without private fixture features")
require_line(line_audit_lines
    "cargo llvm-cov --no-default-features --features xml-fixture-source --lib --lcov --output-path \"$xml_report\""
    "measure the private XML regression fixture separately")
require_line(line_audit_lines
    "python3 scripts/check-rust-line-coverage.py --root \"$root_dir\" --scope \"$scope\" --lcov \"$production_report\""
    "score the production report against the production scope")
require_line(line_audit_lines
    "python3 scripts/check-rust-line-coverage.py --root \"$root_dir\" --scope \"$xml_scope\" --lcov \"$xml_report\""
    "score the XML report against the XML scope")
require_line(line_coverage_job "if: always()"
    "retain the line report after a failed audit")
require_line(line_coverage_job "uses: actions/upload-artifact@v4"
    "upload the line report")
require_line(line_coverage_job "path: target/rust-line-coverage-*.lcov"
    "upload both generated LCOV reports")
require_line(ci_lines "CARGO_LLVM_COV_VERSION: 0.9.1"
    "pin cargo-llvm-cov to the locally validated release")

require_line(integration_coverage_job "- uses: ./.github/actions/setup-toolchain"
    "use the pinned Rust toolchain for C ABI coverage")
require_line(integration_coverage_job
    "cargo install cargo-llvm-cov --version \"$CARGO_LLVM_COV_VERSION\" --locked"
    "install the pinned cargo-llvm-cov release for C ABI coverage")
require_line(integration_coverage_job
    "run: bash scripts/collect-rust-integration-coverage.sh recording-stub"
    "collect recording-stub C ABI coverage")
require_line(integration_coverage_job "if: always()"
    "retain recording-stub evidence after a failed scenario or report")
require_line(integration_coverage_job "uses: actions/upload-artifact@v4"
    "upload recording-stub C ABI coverage")
require_line(integration_coverage_job
    "path: target/rust-integration-coverage/recording-stub/"
    "upload the complete recording-stub evidence directory")

require_line(full_job
    "run: bash scripts/collect-rust-integration-coverage.sh real-core"
    "collect production-shim coverage through real Core")
require_line(full_job
    [=[IMAS_MVDD_INTEGRATION_CMAKE_PREFIX_PATH: ${{ github.workspace }}/al-core-dist]=]
    "reuse the pinned installed IMAS-Core for instrumented coverage")
require_line(full_job "if: always()"
    "retain real-Core evidence after a failed scenario or report")
require_line(full_job "uses: actions/upload-artifact@v4"
    "upload real-Core C ABI coverage")
require_line(full_job "path: target/rust-integration-coverage/real-core/"
    "upload the complete real-Core evidence directory")

if(NOT "workflow_dispatch:" IN_LIST mutation_flat)
    message(FATAL_ERROR "mutation workflow must have a workflow_dispatch trigger")
endif()
foreach(forbidden_trigger IN ITEMS "push:" "pull_request:" "schedule:")
    if("${forbidden_trigger}" IN_LIST mutation_flat)
        message(FATAL_ERROR "mutation workflow must not have a ${forbidden_trigger} trigger")
    endif()
endforeach()
require_line(mutation_job "- uses: ./.github/actions/setup-toolchain"
    "use the pinned Rust toolchain")
require_line(mutation_job
    "run: cargo install cargo-mutants --version \"$CARGO_MUTANTS_VERSION\" --locked"
    "install the pinned cargo-mutants release")
require_line(mutation_job "run: bash scripts/audit-rust-mutation.sh"
    "run the full scoped mutation-audit command")
require_line(mutation_job "if: always()"
    "retain the mutation report after a failed audit")
require_line(mutation_job "uses: actions/upload-artifact@v4"
    "upload the mutation report")
require_line(mutation_job "path: target/rust-mutation-audit.*/"
    "upload the complete mutation-audit directory")
require_line(mutation_lines "CARGO_MUTANTS_VERSION: 27.1.0"
    "pin cargo-mutants to the locally validated release")

file(READ "${LINE_SCOPE_FILE}" line_scope)
file(READ "${XML_LINE_SCOPE_FILE}" xml_line_scope)
file(READ "${PRODUCTION_TESTS_FILE}" production_tests)
file(READ "${MUTATION_AUDIT_FILE}" mutation_audit)
string(JSON line_aggregate GET "${line_scope}" minimums aggregate_percent)
string(JSON line_group GET "${line_scope}" minimums group_percent)
if(NOT line_aggregate EQUAL 90 OR NOT line_group EQUAL 80)
    message(FATAL_ERROR "line scope must enforce 90% aggregate and 80% per group")
endif()
string(JSON xml_line_aggregate GET "${xml_line_scope}" minimums aggregate_percent)
string(JSON xml_line_group GET "${xml_line_scope}" minimums group_percent)
if(NOT xml_line_aggregate EQUAL 90 OR NOT xml_line_group EQUAL 80)
    message(FATAL_ERROR "XML line scope must enforce 90% aggregate and 80% per group")
endif()
function(require_conversion_source scope_contents source_path expected description)
    string(JSON group_count LENGTH "${scope_contents}" groups)
    set(found FALSE)
    math(EXPR last_group "${group_count} - 1")
    foreach(group_index RANGE 0 ${last_group})
        string(JSON group_name GET "${scope_contents}" groups ${group_index} name)
        if(NOT group_name STREQUAL "conversion")
            continue()
        endif()
        string(JSON source_count LENGTH "${scope_contents}" groups ${group_index} sources)
        math(EXPR last_source "${source_count} - 1")
        foreach(source_index RANGE 0 ${last_source})
            string(JSON actual_path GET "${scope_contents}" groups ${group_index} sources ${source_index} path)
            if(actual_path STREQUAL "${source_path}")
                set(found TRUE)
            endif()
        endforeach()
    endforeach()
    if(expected AND NOT found)
        message(FATAL_ERROR "${description}")
    elseif(NOT expected AND found)
        message(FATAL_ERROR "${description}")
    endif()
endfunction()

foreach(runtime_source IN ITEMS runtime_map.rs runtime_map/neo4j_facts.rs
        runtime_map/neo4j_graph.rs)
    require_conversion_source("${line_scope}" "src/conversion/${runtime_source}" TRUE
        "production line scope must measure ${runtime_source}")
    require_conversion_source("${xml_line_scope}" "src/conversion/${runtime_source}" FALSE
        "XML line scope must not claim ${runtime_source}")
endforeach()
require_conversion_source("${line_scope}" "src/conversion/known_artifacts.rs" FALSE
    "production line scope must not claim known_artifacts.rs")
require_conversion_source("${xml_line_scope}" "src/conversion/known_artifacts.rs" TRUE
    "XML line scope must retain known_artifacts.rs")

set(expected_witnesses
    supported_conversion
    absence
    unresolved_data
    failed_acquisition
    retry
    concurrent_same_key)
foreach(scenario IN LISTS expected_witnesses)
    string(JSON witness ERROR_VARIABLE witness_error GET "${production_tests}" witnesses ${scenario})
    if(witness_error OR NOT witness MATCHES "^conversion::runtime_map::")
        message(FATAL_ERROR
            "production test witnesses must retain the ${scenario} runtime-map scenario")
    endif()
endforeach()
string(JSON witness_count LENGTH "${production_tests}" witnesses)
if(NOT witness_count EQUAL 6)
    message(FATAL_ERROR "production test witnesses must retain exactly the six required scenarios")
endif()
string(JSON mutation_scope GET "${mutation_audit}" line_scope)
if(NOT mutation_scope STREQUAL "rust-xml-line-coverage-scope.json")
    message(FATAL_ERROR "mutation audit must reuse the XML line-audit scope")
endif()
string(JSON mutation_aggregate GET "${mutation_audit}" minimums aggregate_percent)
string(JSON mutation_group GET "${mutation_audit}" minimums group_percent)
if(NOT mutation_aggregate EQUAL 85 OR NOT mutation_group EQUAL 75)
    message(FATAL_ERROR "mutation audit must enforce 85% aggregate and 75% per group")
endif()
string(JSON group_count LENGTH "${line_scope}" groups)
if(NOT group_count EQUAL 6)
    message(FATAL_ERROR "line scope must retain exactly the six decision-bearing groups")
endif()
foreach(index RANGE 0 5)
    string(JSON source_count LENGTH "${line_scope}" groups ${index} sources)
    if(source_count EQUAL 0)
        message(FATAL_ERROR "line scope group ${index} must retain at least one source")
    endif()
endforeach()
