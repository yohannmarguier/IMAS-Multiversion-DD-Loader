cmake_minimum_required(VERSION 3.21)

foreach(required_variable WORKFLOW_FILE TOOLCHAIN_ACTION_FILE)
    if(NOT DEFINED ${required_variable})
        message(FATAL_ERROR "${required_variable} is required")
    endif()
endforeach()

function(read_file_lines path output_variable)
    file(READ "${path}" contents)
    string(REPLACE "\r\n" "\n" contents "${contents}")
    # Protect characters that CMake lists interpret specially before turning
    # physical file lines into list elements. In particular, a shell `\` at
    # end of line must not escape the list separator and hide a comment on the
    # following line from the checks below.
    string(REPLACE "\\" "@IMAS_CI_BACKSLASH@" contents "${contents}")
    string(REPLACE ";" "@IMAS_CI_SEMICOLON@" contents "${contents}")
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
    if(NOT "${line}" IN_LIST ${container})
        message(FATAL_ERROR "CI ${container} must ${description}")
    endif()
endfunction()

function(require_file_line lines_variable line description)
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

# Every check below reads one of these two: `workflow_lines` keeps its
# indentation, for the nested-key parsing read_raw_block does; `workflow` is
# the flat, comment-free form the containment checks want.
flatten_block(workflow_lines workflow)

if(DEFINED PINNED_CORE_JOB)
    check_pinned_core_linkage(${PINNED_CORE_JOB} workflow)
    return()
endif()

read_job(fast fast_job)
read_job(full full_job)
read_top_level_mapping(env workflow_env)

require_line(fast_job "build_type: [Debug, Release]"
    "build both CMake configurations")
require_line(fast_job "run: cargo fmt --check" "check formatting")
require_line(fast_job
    "run: cargo clippy --all-targets --all-features -- -D warnings"
    "reject clippy warnings")
require_line(fast_job "-DIMAS_MVDD_REAL_CORE_TESTS=OFF"
    "select the recording-stub test profile")

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
