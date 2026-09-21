cmake_minimum_required(VERSION 3.21)

foreach(required_variable WORKFLOW_FILE TOOLCHAIN_ACTION_FILE)
    if(NOT DEFINED ${required_variable})
        message(FATAL_ERROR "${required_variable} is required")
    endif()
endforeach()

# Protect the characters CMake lists interpret specially, so that one physical
# file line stays one list element. A shell `\` at end of line must not escape
# the list separator and hide the following line's comment. An *unmatched*
# square bracket is the nastier case: CMake treats `[`/`]` as grouping when it
# splits a value into list elements, so a single `grep -q '...\[libal...'` in a
# workflow collapses every line after it into one element -- silently, with no
# error, hiding whole jobs from every check below.
#
# Exact-match helpers put their expected text through this too, so call sites
# keep writing brackets naturally. A regex helper cannot: its pattern is
# matched against already-protected lines, so a pattern that needs a *literal*
# bracket must spell the placeholder.
function(protect_list_characters value output_variable)
    string(REPLACE "\\" "@IMAS_CI_BACKSLASH@" value "${value}")
    string(REPLACE ";" "@IMAS_CI_SEMICOLON@" value "${value}")
    string(REPLACE "[" "@IMAS_CI_LBRACKET@" value "${value}")
    string(REPLACE "]" "@IMAS_CI_RBRACKET@" value "${value}")
    set("${output_variable}" "${value}" PARENT_SCOPE)
endfunction()

function(read_file_lines path output_variable)
    file(READ "${path}" contents)
    string(REPLACE "\r\n" "\n" contents "${contents}")
    protect_list_characters("${contents}" contents)
    string(REPLACE "\n" ";" lines "${contents}")
    set("${output_variable}" "${lines}" PARENT_SCOPE)
endfunction()

read_file_lines("${WORKFLOW_FILE}" workflow_lines)
read_file_lines("${TOOLCHAIN_ACTION_FILE}" toolchain_action_lines)

# Return the raw (unstripped) lines nested under a mapping key at `indent`
# inside `lines_variable` (e.g. "" for a top-level key, "  " for one level
# in), stopping at the next sibling key at that indent. Scanning starts
# immediately unless `gate_line` is non-empty, in which case lines up to and
# including that exact line are skipped first — used to bound job-name
# matching to inside "jobs:" without also matching a same-named key
# elsewhere. Keeping indentation (rather than stripping it here) lets a
# caller recurse into a further-nested key, as the push-trigger check below
# does for "on:" -> "push:"; callers that only need flat containment checks
# should follow up with flatten_block().
function(read_raw_block lines_variable indent key_name gate_line not_found_message output_variable)
    if(gate_line STREQUAL "")
        set(gated TRUE)
    else()
        set(gated FALSE)
    endif()
    set(in_block FALSE)
    set(found_block FALSE)
    set(block_lines)

    foreach(line IN LISTS ${lines_variable})
        if(NOT gated)
            if(line STREQUAL "${gate_line}")
                set(gated TRUE)
            endif()
            continue()
        endif()

        if(line MATCHES "^${indent}([A-Za-z0-9_-]+):[ \t]*$")
            if(in_block)
                break()
            endif()
            if(CMAKE_MATCH_1 STREQUAL key_name)
                set(in_block TRUE)
                set(found_block TRUE)
            endif()
            continue()
        endif()

        if(in_block)
            list(APPEND block_lines "${line}")
        endif()
    endforeach()

    if(NOT found_block)
        message(FATAL_ERROR "${not_found_message}")
    endif()
    set("${output_variable}" "${block_lines}" PARENT_SCOPE)
endfunction()

# Strip and drop blank/comment lines from a raw block, for callers that only
# need a flat containment check (require_line/IN_LIST) rather than further
# nested-key parsing.
function(flatten_block lines_variable output_variable)
    set(flat_lines)
    foreach(raw_line IN LISTS ${lines_variable})
        string(STRIP "${raw_line}" stripped_line)
        if(NOT stripped_line STREQUAL "" AND
                NOT stripped_line MATCHES "^#")
            list(APPEND flat_lines "${stripped_line}")
        endif()
    endforeach()
    set("${output_variable}" "${flat_lines}" PARENT_SCOPE)
endfunction()

function(read_job job_name output_variable)
    read_raw_block(workflow_lines "  " "${job_name}" "jobs:"
        "CI workflow must define a ${job_name} job" job_raw_lines)
    flatten_block(job_raw_lines job_lines)
    set("${output_variable}" "${job_lines}" PARENT_SCOPE)
endfunction()

function(require_line container line description)
    protect_list_characters("${line}" line)
    if(NOT "${line}" IN_LIST ${container})
        message(FATAL_ERROR "CI ${container} must ${description}")
    endif()
endfunction()

function(require_file_line lines_variable line description)
    protect_list_characters("${line}" line)
    flatten_block(${lines_variable} stripped_lines)
    if(NOT "${line}" IN_LIST stripped_lines)
        message(FATAL_ERROR "CI ${lines_variable} must ${description}")
    endif()
endfunction()

function(require_matching_line lines_variable pattern description)
    foreach(line IN LISTS ${lines_variable})
        if(line MATCHES "${pattern}")
            return()
        endif()
    endforeach()
    message(FATAL_ERROR "CI ${lines_variable} must ${description}")
endfunction()

function(forbid_matching_line lines_variable pattern description)
    foreach(line IN LISTS ${lines_variable})
        if(line MATCHES "${pattern}")
            message(FATAL_ERROR "CI ${lines_variable} must not ${description}")
        endif()
    endforeach()
endfunction()

function(forbid_commit_sha lines_variable description)
    foreach(line IN LISTS ${lines_variable})
        string(LENGTH "${line}" line_length)
        math(EXPR last_start "${line_length} - 40")
        if(last_start LESS 0)
            continue()
        endif()
        foreach(start RANGE 0 ${last_start})
            string(SUBSTRING "${line}" ${start} 40 candidate)
            if(candidate MATCHES "^[0-9a-fA-F]+$")
                message(FATAL_ERROR "CI ${lines_variable} must not ${description}")
            endif()
        endforeach()
    endforeach()
endfunction()

function(require_pin_file_output lines_variable output_variable)
    set(current_step_id)
    set(pin_value_variable)
    foreach(line IN LISTS ${lines_variable})
        string(REGEX MATCH "^id: ([A-Za-z0-9_-]+)$" step_id "${line}")
        if(NOT step_id STREQUAL "")
            set(current_step_id "${CMAKE_MATCH_1}")
        endif()

        set(pin_assignment "")
        string(REGEX MATCH
            "^([A-Za-z_][A-Za-z0-9_]*)=\\$\\([^)]*IMAS_CORE_REF[^)]*\\)"
            pin_assignment "${line}")
        if(NOT pin_assignment STREQUAL "")
            set(pin_value_variable "${CMAKE_MATCH_1}")
        endif()

        string(LENGTH "${pin_value_variable}" pin_value_length)
        if(pin_value_length GREATER 0)
            string(FIND "${line}" "$${pin_value_variable}" value_reference)
            string(FIND "${line}" "GITHUB_OUTPUT" output_reference)
            set(output_assignment "")
            string(REGEX MATCH "([A-Za-z0-9_-]+)=.*GITHUB_OUTPUT"
                output_assignment "${line}")
            if(value_reference GREATER -1 AND output_reference GREATER -1 AND
                    NOT current_step_id STREQUAL "" AND NOT output_assignment STREQUAL "")
                set("${output_variable}"
                    "steps.${current_step_id}.outputs.${CMAKE_MATCH_1}" PARENT_SCOPE)
                return()
            endif()
        endif()
    endforeach()
    message(FATAL_ERROR
        "CI ${lines_variable} must write a value read from IMAS_CORE_REF to GITHUB_OUTPUT")
endfunction()

function(read_top_level_mapping mapping_name output_variable)
    read_raw_block(workflow_lines "" "${mapping_name}" ""
        "CI workflow must define a top-level ${mapping_name} mapping" mapping_raw_lines)
    flatten_block(mapping_raw_lines mapping_lines)
    set("${output_variable}" "${mapping_lines}" PARENT_SCOPE)
endfunction()

# Assert that `job_name` is genuinely wired to the committed pin. The first
# two checks are given the whole workflow rather than the job: a decoy commit
# SHA or upstream URL anywhere in the file is still a second source of truth
# for what CI builds. The cache-key checks are bounded to the job, since only
# that job has an IMAS-Core cache. `workflow_lines_variable` is passed rather
# than reached for, so the two scopes a check runs over are both visible in
# its signature; both names also serve as the diagnostic's subject.
function(check_pinned_core_linkage job_name workflow_lines_variable)
    set(job_lines_variable "${job_name}_job")
    read_job(${job_name} ${job_lines_variable})
    forbid_commit_sha(${workflow_lines_variable}
        "inline an IMAS-Core commit SHA")
    require_pin_file_output(${job_lines_variable} pin_output_reference)
    forbid_matching_line(${workflow_lines_variable}
        "https://github\\.com/iterorganization/IMAS-Core\\.git"
        "name the upstream IMAS-Core repository")
    require_matching_line(${job_lines_variable}
        "key: .*${pin_output_reference}"
        "key the acquired IMAS-Core cache on the resolved pin")
    forbid_matching_line(${job_lines_variable} "key: .*IMAS_CORE_VERSION"
        "key the acquired IMAS-Core cache on IMAS_CORE_VERSION")
endfunction()

# The C++, MATLAB and Java HLIs each resolve their own and IMAS-Core's pin in
# one loop and have no Core cache. Their static contract is therefore distinct
# from the Fortran HLI's cached ExternalProject contract above: ensure each
# reads both committed pin files, passes Core's resolved output to CMake, and
# verifies the checkout it acquired. `component` is the token that job's
# resolve loop iterates -- CPP, MATLAB or JAVA -- so a job wired to the wrong
# pin file is a failure rather than a silent pass.
function(check_component_pinned_core_linkage job_name component workflow_lines_variable)
    set(job_lines_variable "${job_name}_job")
    read_job(${job_name} ${job_lines_variable})
    forbid_commit_sha(${workflow_lines_variable}
        "inline an IMAS-Core commit SHA")
    forbid_matching_line(${workflow_lines_variable}
        "https://github\\.com/iterorganization/IMAS-Core\\.git"
        "name the upstream IMAS-Core repository")
    require_matching_line(${job_lines_variable}
        "^for component in ${component} CORE"
        "resolve the committed ${component} and IMAS-Core pins")
    require_matching_line(${job_lines_variable}
        "^ref=\\$\\(head -n1 \"IMAS_.*_REF\""
        "read each HLI component pin from its committed file")
    require_matching_line(${job_lines_variable}
        "^echo \".*=\\$ref\" >> \"\\$GITHUB_OUTPUT\"$"
        "publish each resolved component pin")
    require_matching_line(${job_lines_variable}
        "^-DAL_CORE_GIT_REPOSITORY=https://github\\.com/yohannmarguier/IMAS-Core\\.git"
        "acquire IMAS-Core from the pinned fork")
    require_matching_line(${job_lines_variable}
        "^-DAL_CORE_VERSION=.*steps\\.pins\\.outputs\\.core"
        "configure the ${component} HLI with the resolved IMAS-Core pin")
    require_matching_line(${job_lines_variable}
        "^test \"\\$actual\" = \".*steps\\.pins\\.outputs\\.core"
        "verify the acquired IMAS-Core revision")
endfunction()

# Every check below reads one of these two: `workflow_lines` keeps its
# indentation, for the nested-key parsing read_raw_block does; `workflow` is
# the flat, comment-free form the containment checks want.
flatten_block(workflow_lines workflow)

if(DEFINED PINNED_FORTRAN_CORE_JOB OR DEFINED PINNED_CPP_CORE_JOB
        OR DEFINED PINNED_MATLAB_CORE_JOB OR DEFINED PINNED_JAVA_CORE_JOB)
    foreach(required_variable IN ITEMS PINNED_FORTRAN_CORE_JOB PINNED_CPP_CORE_JOB
            PINNED_MATLAB_CORE_JOB PINNED_JAVA_CORE_JOB)
        if(NOT DEFINED ${required_variable})
            message(FATAL_ERROR
                "CI HLI workflow validation requires ${required_variable}")
        endif()
    endforeach()

    forbid_matching_line(workflow "^- 'tests/\\*\\*'$"
        "ignore workflow-owned HLI tests")
    forbid_matching_line(workflow "^- 'scripts/\\*\\*'$"
        "ignore workflow-owned HLI helper scripts")

    check_pinned_core_linkage(${PINNED_FORTRAN_CORE_JOB} workflow)
    read_job(${PINNED_FORTRAN_CORE_JOB} fortran_hli_job)
    require_matching_line(fortran_hli_job "libhdf5-dev hdf5-tools"
            "install h5diff for fixture provenance")
    require_line(fortran_hli_job "python -m venv hli/imas-python-fixtures/.venv"
            "create the HLI fixture Python environment")
    require_line(fortran_hli_job
            "hli/imas-python-fixtures/.venv/bin/python -m pip install -r .github/hli-fixture-requirements.txt"
            "install the HLI fixture dependencies")
    require_line(fortran_hli_job "ctest --output-on-failure --no-tests=error"
            "retain the complete legacy XML-fixture HLI suite")
    require_matching_line(fortran_hli_job
            "HLI_TOTAL_TESTS - HLI_DISABLED_TESTS"
            "retain the ordinary HLI enabled-test count assertion")
    require_line(fortran_hli_job "- uses: ./.github/actions/setup-dd-graph"
        "start the pinned graph before the graph-backed Fortran scenario")
    require_matching_line(fortran_hli_job
            "prepare-private-xml-fixture-package\.sh.*dist-production.*dist-xml-fixture"
            "stage a private XML-fixture package without changing production")
    require_matching_line(fortran_hli_job
            "-DCMAKE_PREFIX_PATH=.*dist-xml-fixture"
            "configure the complete legacy Fortran suite against the private XML fixture")
    require_matching_line(fortran_hli_job
            "-DCMAKE_PREFIX_PATH=.*dist-production"
            "use the installed production shim for the Fortran scenario")
    require_matching_line(fortran_hli_job
            "-DAL_SHIM_GRAPH_RUNTIME_SCENARIO=ON"
            "configure the graph-backed Fortran scenario explicitly")
    foreach(required_fortran_production_scenario IN ITEMS
            al-fortran-test-shim-graph-runtime
            al-fortran-test-shim-version-unset
            al-fortran-test-shim-stamp-equal
            al-fortran-test-shim-stamp-absent
            al-fortran-test-shim-stamp-malformed)
        require_matching_line(fortran_hli_job "${required_fortran_production_scenario}"
            "run the production Fortran scenario ${required_fortran_production_scenario}")
    endforeach()
    require_matching_line(fortran_hli_job "check_ctest_inventory\.cmake"
        "reject missing, disabled or extra production Fortran selections")
    check_component_pinned_core_linkage(${PINNED_CPP_CORE_JOB} CPP workflow)
    read_job(${PINNED_CPP_CORE_JOB} cpp_hli_job)
    require_line(cpp_hli_job "- uses: ./.github/actions/setup-dd-graph"
        "start the pinned graph before C++ cross-DD conformance scenarios")
    require_line(cpp_hli_job "IMAS_MVDD_GRAPH_DEADLINE_SECONDS: 120"
        "give the C++ HLI graph acquisition enough time for its fixture scope")
    require_matching_line(cpp_hli_job
            "prepare-private-xml-fixture-package\.sh.*dist-production.*dist-xml-fixture"
            "stage the private XML fixture for the complete C++ conformance suite")
    require_matching_line(cpp_hli_job
            "-DCMAKE_PREFIX_PATH=.*dist-xml-fixture"
            "configure the complete C++ suite against the private XML fixture")
    foreach(required_cpp_production_scenario IN ITEMS
            cpp-test-shim-roundtrip-cross-dd
            cpp-test-shim-version-unset
            cpp-test-shim-stamp-equal
            cpp-test-shim-stamp-absent
            cpp-test-shim-stamp-malformed)
        require_matching_line(cpp_hli_job "${required_cpp_production_scenario}"
            "run the production C++ scenario ${required_cpp_production_scenario}")
    endforeach()
    require_matching_line(cpp_hli_job "check_ctest_inventory\.cmake"
        "reject missing, disabled or extra production C++ selections")
    require_matching_line(cpp_hli_job "cpp_graph_acquisition_refusal\.cpp"
        "compile the dedicated production C++ acquisition-refusal probe")
    require_matching_line(cpp_hli_job "--unset=NEO4J_PASSWORD"
        "make the acquisition-refusal condition deterministic")
    foreach(required_refusal_text IN ITEMS
            "conversion map acquisition failed" "equilibrium" "3.40.0" "4.1.1")
        require_matching_line(cpp_hli_job "${required_refusal_text}"
            "assert the production acquisition refusal identifies ${required_refusal_text}")
    endforeach()
    check_component_pinned_core_linkage(${PINNED_MATLAB_CORE_JOB} MATLAB workflow)
    check_component_pinned_core_linkage(${PINNED_JAVA_CORE_JOB} JAVA workflow)

    # MATLAB is the one HLI whose toolchain this workflow installs rather than
    # receiving from the runner image, and the whole job is pointless without
    # it: al-mex-test and every example invoke the MATLAB interpreter.
    read_job(${PINNED_MATLAB_CORE_JOB} matlab_hli_job)
    require_matching_line(matlab_hli_job "uses: matlab-actions/setup-matlab"
        "install MATLAB for the MEX suite")
    require_matching_line(matlab_hli_job "^-DMatlab_ROOT_DIR="
        "point find_package(Matlab) at the installed MATLAB")

    # Both jobs need MDSplus: the MATLAB unit tests parameterise over it and
    # the Java examples ask al-mdsplus-model for their model directory. Neither
    # skips without it -- MATLAB halves al-mex-test, Java fails to configure --
    # so dropping the models would quietly shrink what a green run proves.
    read_job(${PINNED_JAVA_CORE_JOB} java_hli_job)
    require_matching_line(matlab_hli_job "-DAL_BUILD_MDSPLUS_MODELS=ON"
        "build the MDSplus DD models its unit tests open")
    require_matching_line(java_hli_job "-DAL_BUILD_MDSPLUS_MODELS=ON"
        "build the MDSplus DD models its examples open")
    return()
endif()

if(NOT DEFINED GRAPH_SETUP_ACTION_FILE)
    message(FATAL_ERROR "GRAPH_SETUP_ACTION_FILE is required for the CI workflow")
endif()
read_file_lines("${GRAPH_SETUP_ACTION_FILE}" graph_setup_action_lines)

read_job(fast fast_job)
read_job(full full_job)
read_job(graph-provisioning graph_provisioning_job)
read_job(graph-abi graph_abi_job)
read_top_level_mapping(env workflow_env)

require_line(fast_job "build_type: [Debug, Release]"
    "build both CMake configurations")
require_line(fast_job "run: cargo fmt --check" "check formatting")
require_line(fast_job
    "cargo clippy --all-targets -- -D warnings"
    "lint the production graph source")
require_line(fast_job
    "cargo clippy --all-targets --features xml-fixture-source -- -D warnings"
    "lint the private XML fixture path")
require_line(fast_job "-DIMAS_MVDD_REAL_CORE_TESTS=OFF"
    "select the recording-stub test profile")
require_line(graph_provisioning_job "- uses: ./.github/actions/setup-dd-graph"
    "provision the pinned DD graph through the shared setup action")
require_line(graph_provisioning_job "run: echo 'DD graph provisioning smoke check passed'"
    "label graph provisioning as a smoke check rather than conversion coverage")
require_line(graph_abi_job "- uses: ./.github/actions/setup-dd-graph"
    "provision the pinned DD graph before graph-backed ABI checks")
require_matching_line(graph_abi_job "-DIMAS_MVDD_GRAPH_TEST_SOURCE=live"
    "select the live source for graph-required ABI checks")
require_line(graph_abi_job "- uses: ./.github/actions/setup-toolchain"
    "use the pinned toolchain for graph-backed ABI checks")
require_line(graph_abi_job
    "run: bash tests/scripts/check-live-acquisition.sh"
    "fail when live graph acquisition cannot produce the pinned complete scope")
require_line(graph_abi_job
    "run: ctest --test-dir build -L live-graph --output-on-failure --no-tests=error"
    "run the nonempty graph-selected C ABI matrix")

foreach(job IN ITEMS fast_job full_job)
    require_line(${job} "- uses: ./.github/actions/setup-toolchain"
        "use the shared pinned-toolchain setup")
    require_line(${job}
        "run: ctest --test-dir build --output-on-failure --no-tests=error"
        "fail when its selected test profile registers no tests")
    require_line(${job} "run: cmake --install build --prefix \"$PWD/dist\""
        "install the shim")
    require_line(${job} "bash tests/scripts/check-installed-package.sh build dist \"$core\""
        "exercise both installed-package consumption interfaces")
    require_line(${job} "run: bash tests/scripts/check-staged-install.sh build"
        "verify a staged (DESTDIR) install as well as a plain prefix")
    # The install step above deliberately passes an absolute prefix, which is
    # the one form that hides the relative-prefix defect entirely, so this
    # cannot be left to the steps that already exist.
    require_line(${job} "run: bash tests/scripts/check-relative-prefix-install.sh build"
        "verify an install under a relative prefix as well as an absolute one")
endforeach()

if("-DIMAS_CORE_DOWNLOAD_DEPENDENCIES=ON" IN_LIST fast_job)
    message(FATAL_ERROR "CI fast job must not acquire real IMAS-Core")
endif()
require_line(full_job "uses: actions/cache@v4"
    "cache the acquired IMAS-Core build")
require_line(full_job "uses: ./.github/actions/setup-dd-graph"
    "provision the pinned DD graph before production real-Core coverage")
require_matching_line(full_job "-DIMAS_MVDD_GRAPH_TEST_SOURCE=live"
    "point real-Core graph scenarios at the production artifact")
require_line(full_job "- name: Test graph coexistence real-Core scenarios"
    "run the graph coexistence real-Core scenarios explicitly")
require_line(full_job "-P tests/cmake/check_ctest_inventory.cmake"
    "validate exact enabled live and controlled real-Core inventories")
foreach(required_live_scenario IN ITEMS
        live-graph-core-coexistence-forward
        live-graph-core-coexistence-reverse
        live-graph-core-coexistence-nested)
    require_matching_line(full_job "${required_live_scenario}"
        "require the live real-Core coexistence scenario ${required_live_scenario}")
endforeach()
require_line(full_job
    "ctest --test-dir build -R \"$pattern\" --output-on-failure --no-tests=error"
    "execute the graph coexistence real-Core scenarios")
require_line(full_job "- name: Test controlled graph coexistence real-Core scenarios"
    "run the five controlled coexistence scenarios in an isolated profile")
require_matching_line(full_job "-DIMAS_MVDD_GRAPH_TEST_SOURCE=controlled"
    "select controlled facts only for the isolated coexistence profile")
foreach(required_controlled_scenario IN ITEMS
        read-coexistence-forward-selects-primary-then-falls-back
        read-coexistence-forward-arraystruct-falls-back-between-j-candidates
        read-coexistence-reverse-selects-the-4.1-successor
        write-delete-coexistence-forward-is-primary-only-and-fans-out
        write-coexistence-reverse-non-primary-refuses)
    require_matching_line(full_job "${required_controlled_scenario}"
        "retain the controlled real-Core scenario ${required_controlled_scenario}")
endforeach()
require_line(full_job "-DIMAS_CORE_DOWNLOAD_DEPENDENCIES=ON"
    "download the pinned real IMAS-Core")
check_pinned_core_linkage(full workflow)

require_line(workflow_env "RUST_VERSION: 1.88.0"
    "pin Rust to the deployed cluster version")
require_line(workflow_env "CARGO_C_VERSION: 0.10.15"
    "pin cargo-c to the deployed cluster version")
require_file_line(toolchain_action_lines "using: composite"
    "define the shared toolchain setup as a composite action")
require_file_line(toolchain_action_lines
    "rustup toolchain install \"$RUST_VERSION\" --profile minimal -c rustfmt -c clippy"
    "install the pinned Rust toolchain")
require_file_line(graph_setup_action_lines "using: composite"
    "define the DD graph setup as a composite action")
require_file_line(graph_setup_action_lines "uses: actions/cache/restore@v4"
    "restore the immutable DD graph archive cache")
require_file_line(graph_setup_action_lines "uses: actions/cache/save@v4"
    "save an acquired DD graph archive cache")
require_file_line(graph_setup_action_lines "uses: oras-project/setup-oras@v1"
    "install ORAS for a cache miss")
require_file_line(graph_setup_action_lines "scripts/dd-graph.sh setup"
    "load and start a fresh Neo4j database")
require_matching_line(graph_setup_action_lines "scripts/dd-graph\.sh query"
    "run the graph query smoke check")
require_file_line(toolchain_action_lines
    "rustup default \"$RUST_VERSION\""
    "select the pinned Rust toolchain")
require_file_line(toolchain_action_lines
    "| tar -xz -C \"$HOME/.cargo/bin\""
    "install the pinned cargo-c archive")

# The fast job is useful only if branch pushes cannot bypass it. Bound the
# search to "on:" -> "push:" specifically, so an unrelated pull_request
# filter elsewhere in the workflow is permitted.
read_raw_block(workflow_lines "" "on" ""
    "CI workflow must define a top-level on mapping" on_raw_lines)
read_raw_block(on_raw_lines "  " "push" ""
    "CI workflow must define a push trigger" push_raw_lines)
flatten_block(push_raw_lines push_lines)
foreach(line IN LISTS push_lines)
    if(line MATCHES "^branches(-ignore)?:")
        message(FATAL_ERROR "CI workflow must run for pushes to every branch")
    endif()
endforeach()
