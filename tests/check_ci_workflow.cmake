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

# Return the non-comment contents of one top-level job. Bounding the slice at
# the next job keeps a command in a comment or another job from satisfying the
# contract for the requested job.
function(read_job job_name output_variable)
    set(in_jobs FALSE)
    set(in_requested_job FALSE)
    set(found_requested_job FALSE)
    set(job_lines)

    foreach(line IN LISTS workflow_lines)
        if(line STREQUAL "jobs:")
            set(in_jobs TRUE)
            continue()
        endif()
        if(NOT in_jobs)
            continue()
        endif()

        if(line MATCHES "^  ([A-Za-z0-9_-]+):[ \t]*$")
            if(in_requested_job)
                break()
            endif()
            if(CMAKE_MATCH_1 STREQUAL job_name)
                set(in_requested_job TRUE)
                set(found_requested_job TRUE)
            endif()
            continue()
        endif()

        if(in_requested_job)
            string(STRIP "${line}" stripped_line)
            if(NOT stripped_line STREQUAL "" AND
                    NOT stripped_line MATCHES "^#")
                list(APPEND job_lines "${stripped_line}")
            endif()
        endif()
    endforeach()

    if(NOT found_requested_job)
        message(FATAL_ERROR "CI workflow must define a ${job_name} job")
    endif()
    set("${output_variable}" "${job_lines}" PARENT_SCOPE)
endfunction()

function(require_line container line description)
    if(NOT "${line}" IN_LIST ${container})
        message(FATAL_ERROR "CI ${container} must ${description}")
    endif()
endfunction()

function(require_file_line lines_variable line description)
    set(stripped_lines)
    foreach(raw_line IN LISTS ${lines_variable})
        string(STRIP "${raw_line}" stripped_line)
        if(NOT stripped_line STREQUAL "" AND
                NOT stripped_line MATCHES "^#")
            list(APPEND stripped_lines "${stripped_line}")
        endif()
    endforeach()
    if(NOT "${line}" IN_LIST stripped_lines)
        message(FATAL_ERROR "CI ${lines_variable} must ${description}")
    endif()
endfunction()

function(read_top_level_mapping mapping_name output_variable)
    set(in_mapping FALSE)
    set(found_mapping FALSE)
    set(mapping_lines)

    foreach(line IN LISTS workflow_lines)
        if(line MATCHES "^([A-Za-z0-9_-]+):[ \t]*$")
            if(in_mapping)
                break()
            endif()
            if(CMAKE_MATCH_1 STREQUAL mapping_name)
                set(in_mapping TRUE)
                set(found_mapping TRUE)
            endif()
            continue()
        endif()
        if(in_mapping)
            string(STRIP "${line}" stripped_line)
            if(NOT stripped_line STREQUAL "" AND
                    NOT stripped_line MATCHES "^#")
                list(APPEND mapping_lines "${stripped_line}")
            endif()
        endif()
    endforeach()

    if(NOT found_mapping)
        message(FATAL_ERROR "CI workflow must define a top-level ${mapping_name} mapping")
    endif()
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
    require_line(${job} "bash tests/check-installed-package.sh build dist \"$core\""
        "exercise both installed-package consumption interfaces")
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

# The fast job is useful only if branch pushes cannot bypass it. Inspect only
# the push trigger's mapping so an unrelated pull_request filter is permitted.
set(in_push FALSE)
set(found_push FALSE)
foreach(line IN LISTS workflow_lines)
    if(line STREQUAL "jobs:")
        break()
    endif()
    if(line STREQUAL "  push:")
        set(in_push TRUE)
        set(found_push TRUE)
        continue()
    endif()
    if(in_push AND line MATCHES "^  [A-Za-z0-9_-]+:")
        set(in_push FALSE)
    endif()
    if(in_push)
        string(STRIP "${line}" stripped_line)
        if(stripped_line STREQUAL "" OR stripped_line MATCHES "^#")
            continue()
        endif()
        if(stripped_line MATCHES "^branches(-ignore)?:")
            message(FATAL_ERROR "CI workflow must run for pushes to every branch")
        endif()
    endif()
endforeach()
if(NOT found_push)
    message(FATAL_ERROR "CI workflow must define a push trigger")
endif()
