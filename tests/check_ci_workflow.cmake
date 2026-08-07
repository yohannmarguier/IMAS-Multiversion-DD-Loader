cmake_minimum_required(VERSION 3.21)

if(NOT DEFINED WORKFLOW_FILE)
    message(FATAL_ERROR "WORKFLOW_FILE is required")
endif()

file(READ "${WORKFLOW_FILE}" workflow)

function(require_workflow_text text description)
    string(FIND "${workflow}" "${text}" position)
    if(position EQUAL -1)
        message(FATAL_ERROR "CI workflow must ${description}")
    endif()
endfunction()

require_workflow_text("  fast:" "define a fast job")
require_workflow_text("  full:" "define a full job")

string(FIND "${workflow}" "  fast:" fast_position)
string(FIND "${workflow}" "  full:" full_position)
math(EXPR fast_length "${full_position} - ${fast_position}")
string(SUBSTRING "${workflow}" ${fast_position} ${fast_length} fast_job)
string(SUBSTRING "${workflow}" ${full_position} -1 full_job)

function(require_job_text job text description)
    string(FIND "${${job}}" "${text}" position)
    if(position EQUAL -1)
        message(FATAL_ERROR "CI ${job} job must ${description}")
    endif()
endfunction()

require_job_text(fast_job "build_type: [Debug, Release]"
    "build both CMake configurations")
require_job_text(fast_job "cargo fmt --check" "check formatting")
require_job_text(fast_job "cargo clippy --all-targets --all-features -- -D warnings"
    "reject clippy warnings")
require_job_text(fast_job "-DIMAS_MVDD_REAL_CORE_TESTS=OFF"
    "select the recording-stub test profile")

string(FIND "${fast_job}" "IMAS_CORE_DOWNLOAD_DEPENDENCIES" fast_download_position)
if(NOT fast_download_position EQUAL -1)
    message(FATAL_ERROR "CI fast job must not acquire real IMAS-Core")
endif()

require_job_text(full_job "uses: actions/cache@v4"
    "cache the acquired IMAS-Core build")
require_job_text(full_job "-DIMAS_CORE_DOWNLOAD_DEPENDENCIES=ON"
    "download the pinned real IMAS-Core")

foreach(job IN ITEMS fast_job full_job)
    require_job_text(${job} "ctest --test-dir build --output-on-failure --no-tests=error"
        "fail when its selected test profile registers no tests")
    require_job_text(${job} "cmake --install build"
        "install the shim")
    require_job_text(${job} "tests/check-installed-package.sh"
        "exercise both installed-package consumption interfaces")
endforeach()

require_workflow_text("RUST_VERSION: 1.88.0"
    "pin Rust to the deployed cluster version")
require_workflow_text("CARGO_C_VERSION: 0.10.15"
    "pin cargo-c to the deployed cluster version")

# The fast job is useful only if branch pushes cannot bypass it.
string(FIND "${workflow}" "jobs:" jobs_position)
string(SUBSTRING "${workflow}" 0 ${jobs_position} triggers)
string(FIND "${triggers}" "branches:" restricted_push_position)
if(NOT restricted_push_position EQUAL -1)
    message(FATAL_ERROR "CI workflow must run for pushes to every branch")
endif()
