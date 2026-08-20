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

function(read_top_level_mapping mapping_name output_variable)
    read_raw_block(workflow_lines "" "${mapping_name}" ""
        "CI workflow must define a top-level ${mapping_name} mapping" mapping_raw_lines)
    flatten_block(mapping_raw_lines mapping_lines)
    set("${output_variable}" "${mapping_lines}" PARENT_SCOPE)
endfunction()

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
